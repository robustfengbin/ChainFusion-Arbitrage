//! 套利执行器核心实现
//!
//! 支持三种交易发送模式：
//! 1. 普通模式：通过公开 mempool 发送交易
//! 2. Flashbots 模式：通过 Flashbots 私密发送，防止 MEV 攻击
//! 3. Both 模式：同时通过 Flashbots 和公开 mempool 发送，提高成功率

use anyhow::Result;
use ethers::prelude::*;
use ethers::types::{Address, H256, U256};
use ethers::types::transaction::eip2718::TypedTransaction;
use ethers::utils::keccak256;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn, debug, error};

use crate::flash_arbitrage::{FlashArbitrageContract, ArbitrageContractParams};
use crate::flashbots::{FlashbotsClient, FlashbotsConfig, FlashbotsSendResult, BundleBuilder};
use crate::types::{ArbitrageParams, ExecutionResult, ExecutionError, GasStrategy};
use crate::debug_info::{ExecutionDebugger, TokenInfoSnapshot, TokenDetail, log_execution_start};
use crate::revert_decoder::RevertDecoder;
use services::SharedPriceService;

/// 交易发送模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendMode {
    /// 普通模式：通过公开 mempool 发送
    Normal,
    /// Flashbots 模式：私密发送，防止 MEV 攻击
    Flashbots,
    /// Both 模式：同时通过 Flashbots 和公开 mempool 发送，提高成功率
    Both,
}

impl Default for SendMode {
    fn default() -> Self {
        Self::Normal
    }
}

/// 执行器配置
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// 合约地址
    pub contract_address: Address,
    /// 链 ID
    pub chain_id: u64,
    /// Gas 策略
    pub gas_strategy: GasStrategy,
    /// 交易确认超时 (秒)
    pub confirmation_timeout_secs: u64,
    /// 需要的确认数
    pub confirmations: usize,
    /// 是否启用模拟执行
    pub simulate_before_execute: bool,
    /// 私钥 (用于签名交易)
    pub private_key: Option<String>,
    /// 交易发送模式
    pub send_mode: SendMode,
    /// Flashbots 配置
    pub flashbots_config: FlashbotsConfig,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            contract_address: Address::zero(),
            chain_id: 1,
            gas_strategy: GasStrategy::default(),
            confirmation_timeout_secs: 120,
            confirmations: 1,
            simulate_before_execute: true,
            private_key: None,
            send_mode: SendMode::Normal,
            flashbots_config: FlashbotsConfig::default(),
        }
    }
}

/// 套利执行器
pub struct ArbitrageExecutor<M: Middleware + 'static> {
    config: ExecutorConfig,
    provider: Arc<M>,
    contract: FlashArbitrageContract<M>,
    wallet: Option<LocalWallet>,
    price_service: Option<SharedPriceService>,
    /// Flashbots 客户端（如果启用）
    flashbots_client: Option<FlashbotsClient<M>>,
    /// 执行调试器
    debugger: ExecutionDebugger<M>,
}

impl<M: Middleware + 'static> ArbitrageExecutor<M> {
    /// 创建新的执行器
    pub fn new(config: ExecutorConfig, provider: Arc<M>) -> Result<Self> {
        let contract = FlashArbitrageContract::new(config.contract_address, provider.clone());

        let wallet = if let Some(ref key) = config.private_key {
            Some(key.parse::<LocalWallet>()?.with_chain_id(config.chain_id))
        } else {
            None
        };

        // 如果启用 Flashbots 或 Both 模式，初始化客户端
        let flashbots_client = if config.send_mode == SendMode::Flashbots || config.send_mode == SendMode::Both {
            if let Some(ref key) = config.private_key {
                let mut fb_config = config.flashbots_config.clone();
                fb_config.enabled = true;
                fb_config.chain_id = config.chain_id;
                // 如果未配置 relay_url，则自动选择对应链的中继 URL
                if fb_config.relay_url.is_empty() {
                    fb_config.relay_url = FlashbotsConfig::relay_url_for_chain(config.chain_id).to_string();
                }
                info!("📡 Flashbots relay URL: {}", fb_config.relay_url);

                match FlashbotsClient::new(fb_config, provider.clone(), key) {
                    Ok(client) => {
                        info!("Flashbots 客户端已初始化，链 ID: {}, 模式: {:?}", config.chain_id, config.send_mode);
                        Some(client)
                    }
                    Err(e) => {
                        warn!("Flashbots 客户端初始化失败: {:?}，将使用普通模式", e);
                        None
                    }
                }
            } else {
                warn!("Flashbots/Both 模式需要私钥，将使用普通模式");
                None
            }
        } else {
            None
        };

        // 创建执行调试器
        let debugger = ExecutionDebugger::new(provider.clone(), config.chain_id);

        Ok(Self {
            config,
            provider,
            contract,
            wallet,
            price_service: None,
            flashbots_client,
            debugger,
        })
    }

    /// 创建带价格服务的执行器
    pub fn with_price_service(mut self, price_service: SharedPriceService) -> Self {
        self.price_service = Some(price_service);
        self
    }

    /// 执行套利
    pub async fn execute(&self, params: ArbitrageParams) -> Result<ExecutionResult, ExecutionError> {
        // 打印执行开始信息
        log_execution_start(&params);

        // ========== 关键校验：验证钱包地址是否为合约 owner ==========
        // 错误码 0x118cdaa7 (OwnableUnauthorizedAccount) 表示调用者不是 owner
        if let Some(ref wallet) = self.wallet {
            match self.check_owner().await {
                Ok(contract_owner) => {
                    let wallet_address = wallet.address();
                    if contract_owner != wallet_address {
                        warn!("⚠️ 钱包地址 {:?} 不是合约 owner {:?}", wallet_address, contract_owner);
                        warn!("⚠️ 这将导致 onlyOwner 权限检查失败 (错误码 0x118cdaa7)");
                        warn!("⚠️ 请检查：1) 私钥是否正确 2) 是否需要转移 owner 权限");
                        return Err(ExecutionError::ContractError(
                            format!("钱包地址 {:?} 不是合约 owner {:?}，无法执行套利", wallet_address, contract_owner)
                        ));
                    }
                    debug!("✅ Owner 校验通过: {:?}", wallet_address);
                }
                Err(e) => {
                    warn!("⚠️ 无法获取合约 owner: {:?}，继续执行", e);
                }
            }
        } else {
            return Err(ExecutionError::WalletError("未配置钱包，无法执行套利".to_string()));
        }

        // 获取代币信息用于调试
        let token_info = self.build_token_info(&params).await;

        // 创建执行快照
        let mut snapshot = self.debugger.create_snapshot(&params, Some(token_info)).await;

        // 构建合约调用参数
        let contract_params = ArbitrageContractParams {
            flash_pool: params.flash_pool,
            token_a: params.token_a,
            token_b: params.token_b,
            token_c: params.token_c,
            fee1: params.fee1,
            fee2: params.fee2,
            fee3: params.fee3,
            amount_in: params.amount_in,
            min_profit: params.min_profit,
            profit_token: params.profit_token.unwrap_or(Address::zero()),
            profit_convert_fee: params.profit_convert_fee,
        };

        // 模拟执行 (仅 Flashbots 和 Both 模式需要)
        // - Normal 模式：不需要模拟，直接发送到 mempool
        // - Flashbots 模式：必须模拟成功才能发送
        // - Both 模式：模拟失败时仍可发送 mempool，只跳过 Flashbots
        let simulation_passed = if self.config.simulate_before_execute
            && (self.config.send_mode == SendMode::Flashbots || self.config.send_mode == SendMode::Both)
        {
            match self.simulate_execution(&contract_params).await {
                Ok(estimated_profit) => {
                    info!(target: "arbitrage_execution", "模拟执行成功, 预估利润: {}", estimated_profit);
                    if estimated_profit < params.min_profit {
                        let err = ExecutionError::InsufficientProfit {
                            expected: params.min_profit,
                            actual: estimated_profit,
                        };
                        self.debugger.record_error(&mut snapshot, &format!("{:?}", err), None, None);

                        // Flashbots 模式下模拟失败直接返回错误
                        if self.config.send_mode == SendMode::Flashbots {
                            return Err(err);
                        }
                        // Both 模式下继续执行，但标记模拟失败
                        warn!(target: "arbitrage_execution", "⚠️ 模拟利润不足，Both 模式将仅使用 Mempool 发送");
                        false
                    } else {
                        true
                    }
                }
                Err(e) => {
                    // 解析并记录详细错误信息
                    let error_str = format!("{:?}", e);
                    self.debugger.record_error(&mut snapshot, &error_str, None, None);

                    // 额外打印解码后的错误
                    let decoded = RevertDecoder::decode_from_error_string(&error_str);
                    warn!(target: "arbitrage_execution", "模拟执行失败 - 详细错误:");
                    warn!(target: "arbitrage_execution", "{}", decoded);

                    // Flashbots 模式下模拟失败直接返回错误
                    if self.config.send_mode == SendMode::Flashbots {
                        return Err(ExecutionError::ContractError(format!("Simulation failed: {}", decoded.message)));
                    }
                    // Both 模式下继续执行，但标记模拟失败
                    warn!(target: "arbitrage_execution", "⚠️ 模拟失败，Both 模式将仅使用 Mempool 发送");
                    false
                }
            }
        } else {
            // Normal 模式不需要模拟，或者未启用模拟
            true
        };

        // 执行实际交易
        let tx_hash = match self.send_transaction(&contract_params, simulation_passed).await {
            Ok(hash) => {
                info!("交易已发送: {:?}", hash);
                hash
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                self.debugger.record_error(&mut snapshot, &error_str, None, None);
                return Err(e);
            }
        };

        // 等待确认
        let receipt = match self.wait_for_confirmation(tx_hash).await {
            Ok(r) => r,
            Err(e) => {
                let error_str = format!("{:?}", e);
                self.debugger.record_error(&mut snapshot, &error_str, None, None);
                return Err(e);
            }
        };

        // 解析结果
        self.parse_execution_result(tx_hash, receipt, &params).await
    }

    /// 构建代币信息用于调试
    async fn build_token_info(&self, params: &ArbitrageParams) -> TokenInfoSnapshot {
        let price_a = if let Some(ref ps) = self.price_service {
            ps.get_price_by_address(&params.token_a).await.unwrap_or(Decimal::ZERO)
        } else {
            Decimal::ZERO
        };

        let price_b = if let Some(ref ps) = self.price_service {
            ps.get_price_by_address(&params.token_b).await.unwrap_or(Decimal::ZERO)
        } else {
            Decimal::ZERO
        };

        let price_c = if let Some(ref ps) = self.price_service {
            ps.get_price_by_address(&params.token_c).await.unwrap_or(Decimal::ZERO)
        } else {
            Decimal::ZERO
        };

        // 获取代币符号和精度
        let (symbol_a, decimals_a) = self.get_token_metadata(params.token_a).await;
        let (symbol_b, decimals_b) = self.get_token_metadata(params.token_b).await;
        let (symbol_c, decimals_c) = self.get_token_metadata(params.token_c).await;

        TokenInfoSnapshot {
            token_a: TokenDetail {
                address: format!("{:?}", params.token_a),
                symbol: symbol_a,
                decimals: decimals_a,
                price_usd: price_a,
                price_source: "price_service".to_string(),
            },
            token_b: TokenDetail {
                address: format!("{:?}", params.token_b),
                symbol: symbol_b,
                decimals: decimals_b,
                price_usd: price_b,
                price_source: "price_service".to_string(),
            },
            token_c: TokenDetail {
                address: format!("{:?}", params.token_c),
                symbol: symbol_c,
                decimals: decimals_c,
                price_usd: price_c,
                price_source: "price_service".to_string(),
            },
        }
    }

    /// 获取代币元数据 (符号和精度)
    async fn get_token_metadata(&self, token: Address) -> (String, u8) {
        // 常见代币的精度映射 (避免 RPC 调用)
        let known_decimals: [(Address, u8); 5] = [
            ("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".parse().unwrap(), 6),  // USDC
            ("0xdAC17F958D2ee523a2206206994597C13D831ec7".parse().unwrap(), 6),  // USDT
            ("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".parse().unwrap(), 18), // WETH
            ("0x6B175474E89094C44Da98b954EedeAC495271d0F".parse().unwrap(), 18), // DAI
            ("0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599".parse().unwrap(), 8),  // WBTC
        ];

        // 先从已知映射获取
        let decimals = known_decimals.iter()
            .find(|(addr, _)| *addr == token)
            .map(|(_, d)| *d)
            .unwrap_or_else(|| {
                // 尝试从链上获取
                self.get_token_decimals_from_chain(token)
            });

        let symbol = self.get_token_symbol(token).await.unwrap_or_else(|| "UNKNOWN".to_string());

        (symbol, decimals)
    }

    /// 从链上获取代币精度 (同步版本，用于回退)
    fn get_token_decimals_from_chain(&self, _token: Address) -> u8 {
        // 默认返回 18，因为大多数 ERC20 代币是 18 位精度
        // 如果需要精确值，可以在这里添加链上调用
        18
    }

    /// 获取代币符号
    async fn get_token_symbol(&self, token: Address) -> Option<String> {
        abigen!(
            IERC20Metadata,
            r#"[function symbol() external view returns (string)]"#
        );

        let erc20 = IERC20Metadata::new(token, self.provider.clone());
        match erc20.symbol().call().await {
            Ok(symbol) => Some(symbol),
            Err(_) => None,
        }
    }

    /// 模拟执行 (静态调用)
    ///
    /// 使用 eth_call 进行模拟，不会真正上链：
    /// - `.call()` 是 ethers-rs 的静态调用方法，对应以太坊的 eth_call RPC
    /// - 在节点本地模拟执行，不消耗 gas，不改变链上状态
    /// - 返回合约函数的返回值（这里是 profit）
    /// - 如果执行会 revert，这里也会返回错误
    ///
    /// 与正式执行 `.send()` 的区别：
    /// - `.call()`: eth_call，只读模拟，不上链，不花钱
    /// - `.send()`: eth_sendTransaction，真正发送交易，消耗 gas
    ///
    /// 重要：必须设置 from 参数为 owner 地址，否则 onlyOwner 等权限检查会失败
    async fn simulate_execution(&self, params: &ArbitrageContractParams) -> Result<U256, ExecutionError> {
        debug!("模拟执行套利 (eth_call)...");

        // 获取发送者地址（必须是合约 owner）
        // 如果没有 wallet，则无法进行有效的模拟
        let from_address = self.wallet.as_ref()
            .map(|w| w.address())
            .ok_or_else(|| ExecutionError::WalletError("模拟执行需要配置钱包以设置 from 地址".to_string()))?;

        debug!("模拟执行 from 地址: {:?}", from_address);

        let call = self.contract.execute_arbitrage(params.clone().into_tuple())
            .from(from_address);  // 关键：设置 from 为 owner 地址

        // .call() 是静态调用，不会上链，只是模拟执行获取返回值
        match call.call().await {
            Ok(profit) => Ok(profit),
            Err(e) => Err(ExecutionError::ContractError(format!("{:?}", e))),
        }
    }

    /// 发送交易
    ///
    /// 根据配置选择发送模式：
    /// - Normal: 通过公开 mempool 发送
    /// - Flashbots: 通过 Flashbots 私密发送
    /// - Both: 同时通过 Flashbots 和公开 mempool 发送
    ///
    /// simulation_passed: 模拟是否通过，用于 Both 模式决定是否发送 Flashbots
    async fn send_transaction(&self, params: &ArbitrageContractParams, simulation_passed: bool) -> Result<H256, ExecutionError> {
        // 根据发送模式选择不同的发送方式
        match self.config.send_mode {
            SendMode::Flashbots => {
                if self.flashbots_client.is_some() {
                    self.send_via_flashbots(params).await
                } else {
                    warn!("Flashbots 客户端未初始化，回退到普通模式");
                    self.send_via_mempool(params).await
                }
            }
            SendMode::Both => {
                self.send_via_both(params, simulation_passed).await
            }
            SendMode::Normal => {
                self.send_via_mempool(params).await
            }
        }
    }

    /// 同时通过 Flashbots 和公开 mempool 发送交易
    ///
    /// 并行发送模式：同时发送 Mempool (nonce N) 和 Flashbots (nonce N+1)
    /// 两个交易都会被真正执行，用于测试两个通道是否正常
    ///
    /// simulation_passed: 模拟是否通过
    /// - true: 并行发送到两个通道
    /// - false: 仅发送到 Mempool，跳过 Flashbots
    async fn send_via_both(&self, params: &ArbitrageContractParams, simulation_passed: bool) -> Result<H256, ExecutionError> {
        let wallet = self.wallet.as_ref()
            .ok_or_else(|| ExecutionError::WalletError("No wallet configured".to_string()))?;
        let from_address = wallet.address();

        // 获取当前 nonce
        let base_nonce = self.provider.get_transaction_count(from_address, None).await
            .map_err(|e| ExecutionError::NonceError(format!("{:?}", e)))?;

        // 如果模拟失败或 Flashbots 客户端未初始化，仅使用 Mempool 发送
        if !simulation_passed {
            info!(target: "arbitrage_execution", "🚀 Both 模式：模拟失败，仅使用 Mempool 发送 (nonce={})", base_nonce);
            return self.send_via_mempool_with_nonce(params, base_nonce).await;
        }

        if self.flashbots_client.is_none() {
            warn!(target: "arbitrage_execution", "⚠️ Flashbots 客户端未初始化，仅使用 Mempool 发送");
            return self.send_via_mempool_with_nonce(params, base_nonce).await;
        }

        info!(target: "arbitrage_execution", "🚀 Both 模式：并行发送到 Mempool 和 Flashbots（两边都执行）");

        let mempool_nonce = base_nonce;
        let flashbots_nonce = base_nonce + 1;

        info!(target: "arbitrage_execution", "📋 Nonce 分配:");
        info!(target: "arbitrage_execution", "   - Mempool:   nonce = {}", mempool_nonce);
        info!(target: "arbitrage_execution", "   - Flashbots: nonce = {}", flashbots_nonce);

        // ========== 并行发送到两个通道 ==========
        info!(target: "arbitrage_execution", "📤 并行发送交易到 Mempool 和 Flashbots...");

        // 并行发送
        let mempool_future = self.send_via_mempool_with_nonce(params, mempool_nonce);
        let flashbots_future = self.send_via_flashbots_with_nonce(params, flashbots_nonce);

        let (mempool_result, flashbots_result) = tokio::join!(mempool_future, flashbots_future);

        // 处理 Mempool 结果
        let mempool_hash = match mempool_result {
            Ok(hash) => {
                info!(target: "arbitrage_execution", "✅ Mempool 广播成功: {:?}", hash);
                Some(hash)
            }
            Err(e) => {
                error!(target: "arbitrage_execution", "❌ Mempool 发送失败: {:?}", e);
                None
            }
        };

        // 处理 Flashbots 结果
        let flashbots_hash = match flashbots_result {
            Ok(hash) => {
                info!(target: "arbitrage_execution", "✅ Flashbots 发送成功: {:?}", hash);
                Some(hash)
            }
            Err(e) => {
                error!(target: "arbitrage_execution", "❌ Flashbots 发送失败: {:?}", e);
                None
            }
        };

        // 返回结果
        match (mempool_hash, flashbots_hash) {
            (Some(m_hash), Some(f_hash)) => {
                info!(target: "arbitrage_execution", "🎉 Both 模式执行完成! 两个通道都已发送");
                info!(target: "arbitrage_execution", "   Mempool 交易:   {:?}", m_hash);
                info!(target: "arbitrage_execution", "   Flashbots 交易: {:?}", f_hash);
                // 返回 Mempool 的 hash (nonce 较小，会先被确认)
                Ok(m_hash)
            }
            (Some(m_hash), None) => {
                info!(target: "arbitrage_execution", "📦 仅 Mempool 成功，返回: {:?}", m_hash);
                Ok(m_hash)
            }
            (None, Some(f_hash)) => {
                info!(target: "arbitrage_execution", "📦 仅 Flashbots 成功，返回: {:?}", f_hash);
                Ok(f_hash)
            }
            (None, None) => {
                Err(ExecutionError::ContractError("Both 模式：两个通道都发送失败".to_string()))
            }
        }
    }

    /// 通过公开 mempool 发送交易（指定 nonce）
    async fn send_via_mempool_with_nonce(&self, params: &ArbitrageContractParams, nonce: U256) -> Result<H256, ExecutionError> {
        let wallet = self.wallet.as_ref()
            .ok_or_else(|| ExecutionError::WalletError("No wallet configured".to_string()))?;

        let from_address = wallet.address();
        debug!("发送交易 from 地址: {:?}, nonce: {}", from_address, nonce);

        // 构建交易调用
        let call = self.contract.execute_arbitrage(params.clone().into_tuple())
            .from(from_address);

        // 获取 gas limit
        let gas_limit = if let Some(fixed_limit) = self.config.gas_strategy.fixed_gas_limit {
            U256::from(fixed_limit)
        } else {
            let gas_estimate = call.estimate_gas().await
                .map_err(|e| ExecutionError::GasEstimationFailed(format!("{:?}", e)))?;
            U256::from((gas_estimate.as_u64() as f64 * self.config.gas_strategy.gas_limit_multiplier) as u64)
        };

        // 获取 gas price
        let gas_price = self.get_gas_price().await?;

        // 检查 gas price 上限
        let max_gas_price = U256::from((self.config.gas_strategy.max_gas_price_gwei * 1_000_000_000.0) as u128);
        if gas_price > max_gas_price {
            return Err(ExecutionError::GasEstimationFailed(
                format!("Gas price {} exceeds max {}", gas_price, max_gas_price)
            ));
        }

        // 构建交易，指定 nonce
        let tx = call
            .gas(gas_limit)
            .gas_price(gas_price)
            .nonce(nonce);

        // 发送交易
        let pending_tx = tx.send().await
            .map_err(|e| ExecutionError::ContractError(format!("{:?}", e)))?;

        Ok(pending_tx.tx_hash())
    }

    /// 通过 Flashbots 发送交易（指定 nonce）
    async fn send_via_flashbots_with_nonce(&self, params: &ArbitrageContractParams, nonce: U256) -> Result<H256, ExecutionError> {
        let flashbots = self.flashbots_client.as_ref()
            .ok_or_else(|| ExecutionError::FlashbotsError("Flashbots client not initialized".to_string()))?;

        let wallet = self.wallet.as_ref()
            .ok_or_else(|| ExecutionError::WalletError("No wallet configured".to_string()))?;

        let from_address = wallet.address();
        info!("通过 Flashbots 发送私密交易, from: {:?}, nonce: {}", from_address, nonce);

        // 构建交易调用
        let call = self.contract.execute_arbitrage(params.clone().into_tuple())
            .from(from_address);

        // 获取 gas limit
        let gas_limit = if let Some(fixed_limit) = self.config.gas_strategy.fixed_gas_limit {
            U256::from(fixed_limit)
        } else {
            let gas_estimate = call.estimate_gas().await
                .map_err(|e| ExecutionError::GasEstimationFailed(format!("{:?}", e)))?;
            U256::from((gas_estimate.as_u64() as f64 * self.config.gas_strategy.gas_limit_multiplier) as u64)
        };

        // 获取 gas price
        let gas_price = self.get_gas_price().await?;

        // 构建完整交易
        let tx_request = TransactionRequest::new()
            .to(self.config.contract_address)
            .from(from_address)
            .data(call.calldata().unwrap_or_default())
            .gas(gas_limit)
            .gas_price(gas_price)
            .nonce(nonce)
            .chain_id(self.config.chain_id);

        // 签名交易
        let typed_tx: TypedTransaction = tx_request.into();
        let signed_tx = flashbots.sign_transaction(&typed_tx).await
            .map_err(|e| ExecutionError::FlashbotsError(format!("Failed to sign transaction: {:?}", e)))?;

        // 构建 Bundle 并发送
        let bundle = BundleBuilder::new()
            .push_transaction(signed_tx);

        let result = flashbots.send_bundle(bundle).await;

        match result {
            FlashbotsSendResult::Included { tx_hash, block_number, .. } => {
                info!("Flashbots 交易成功打包！区块: {}, 交易哈希: {:?}", block_number, tx_hash);
                Ok(tx_hash)
            }
            FlashbotsSendResult::NotIncluded { reason, .. } => {
                warn!("Flashbots Bundle 未被打包: {}", reason);
                Err(ExecutionError::FlashbotsNotIncluded(reason))
            }
            FlashbotsSendResult::SimulationFailed { error } => {
                Err(ExecutionError::FlashbotsError(format!("Simulation failed: {}", error)))
            }
            FlashbotsSendResult::SendFailed { error } => {
                Err(ExecutionError::FlashbotsError(format!("Send failed: {}", error)))
            }
        }
    }

    /// 通过公开 mempool 发送交易（普通模式）
    async fn send_via_mempool(&self, params: &ArbitrageContractParams) -> Result<H256, ExecutionError> {
        let wallet = self.wallet.as_ref()
            .ok_or_else(|| ExecutionError::WalletError("No wallet configured".to_string()))?;

        let from_address = wallet.address();
        debug!("发送交易 from 地址: {:?}", from_address);

        // 构建交易，必须设置 from 地址
        let call = self.contract.execute_arbitrage(params.clone().into_tuple())
            .from(from_address);  // 关键：设置 from 为 owner 地址

        // 获取 gas limit (固定值或动态估算)
        let gas_limit = if let Some(fixed_limit) = self.config.gas_strategy.fixed_gas_limit {
            // 使用固定 gas limit，跳过估算
            debug!("使用固定 Gas Limit: {} (跳过估算)", fixed_limit);
            U256::from(fixed_limit)
        } else {
            // 动态估算 gas (estimate_gas 底层也是 eth_call，需要正确的 from)
            let gas_estimate = call.estimate_gas().await
                .map_err(|e| ExecutionError::GasEstimationFailed(format!("{:?}", e)))?;

            let limit = U256::from(
                (gas_estimate.as_u64() as f64 * self.config.gas_strategy.gas_limit_multiplier) as u64
            );
            debug!("Gas 估算: {} | Gas 限制: {}", gas_estimate, limit);
            limit
        };

        // 获取 gas price
        let gas_price = self.get_gas_price().await?;

        // 检查 gas price 是否超过最大限制 (支持小数 Gwei)
        let max_gas_price = U256::from((self.config.gas_strategy.max_gas_price_gwei * 1_000_000_000.0) as u128);
        if gas_price > max_gas_price {
            return Err(ExecutionError::GasEstimationFailed(
                format!("Gas price {} exceeds max {}", gas_price, max_gas_price)
            ));
        }

        // 构建并签名交易
        let tx = call
            .gas(gas_limit)
            .gas_price(gas_price);

        // 发送交易
        let pending_tx = tx.send().await
            .map_err(|e| ExecutionError::ContractError(format!("{:?}", e)))?;

        Ok(pending_tx.tx_hash())
    }

    /// 通过 Flashbots 私密发送交易
    ///
    /// 流程：
    /// 1. 构建并签名交易
    /// 2. 包装成 Bundle
    /// 3. 发送到 Flashbots 中继
    /// 4. 等待打包确认
    async fn send_via_flashbots(&self, params: &ArbitrageContractParams) -> Result<H256, ExecutionError> {
        let flashbots = self.flashbots_client.as_ref()
            .ok_or_else(|| ExecutionError::FlashbotsError("Flashbots client not initialized".to_string()))?;

        let wallet = self.wallet.as_ref()
            .ok_or_else(|| ExecutionError::WalletError("No wallet configured".to_string()))?;

        let from_address = wallet.address();
        info!("通过 Flashbots 发送私密交易, from: {:?}", from_address);

        // 构建交易调用，必须设置 from 地址
        let call = self.contract.execute_arbitrage(params.clone().into_tuple())
            .from(from_address);  // 关键：设置 from 为 owner 地址

        // 获取 gas limit (固定值或动态估算)
        let gas_limit = if let Some(fixed_limit) = self.config.gas_strategy.fixed_gas_limit {
            // 使用固定 gas limit，跳过估算
            debug!("Flashbots: 使用固定 Gas Limit: {} (跳过估算)", fixed_limit);
            U256::from(fixed_limit)
        } else {
            // 动态估算 gas (estimate_gas 底层也是 eth_call，需要正确的 from)
            let gas_estimate = call.estimate_gas().await
                .map_err(|e| ExecutionError::GasEstimationFailed(format!("{:?}", e)))?;

            let limit = U256::from(
                (gas_estimate.as_u64() as f64 * self.config.gas_strategy.gas_limit_multiplier) as u64
            );
            debug!("Flashbots: Gas 估算: {} | Gas 限制: {}", gas_estimate, limit);
            limit
        };

        // 获取 gas price
        let gas_price = self.get_gas_price().await?;

        // 获取 nonce
        let nonce = self.provider.get_transaction_count(from_address, None).await
            .map_err(|e| ExecutionError::NonceError(format!("{:?}", e)))?;

        // 构建完整的交易，显式设置 from 地址
        let tx_request = TransactionRequest::new()
            .to(self.config.contract_address)
            .from(from_address)  // 关键：显式设置 from
            .data(call.calldata().unwrap_or_default())
            .gas(gas_limit)
            .gas_price(gas_price)
            .nonce(nonce)
            .chain_id(self.config.chain_id);

        // 签名交易
        let typed_tx: TypedTransaction = tx_request.into();
        let signed_tx = flashbots.sign_transaction(&typed_tx).await
            .map_err(|e| ExecutionError::FlashbotsError(format!("Failed to sign transaction: {:?}", e)))?;

        // 构建 Bundle 并发送
        let bundle = BundleBuilder::new()
            .push_transaction(signed_tx);

        let result = flashbots.send_bundle(bundle).await;

        match result {
            FlashbotsSendResult::Included { tx_hash, block_number, .. } => {
                info!("Flashbots 交易成功打包！区块: {}, 交易哈希: {:?}", block_number, tx_hash);
                Ok(tx_hash)
            }
            FlashbotsSendResult::NotIncluded { reason, .. } => {
                warn!("Flashbots Bundle 未被打包: {}", reason);
                Err(ExecutionError::FlashbotsNotIncluded(reason))
            }
            FlashbotsSendResult::SimulationFailed { error } => {
                warn!("Flashbots 模拟失败: {}", error);
                Err(ExecutionError::FlashbotsSimulationFailed(error))
            }
            FlashbotsSendResult::SendFailed { error } => {
                warn!("Flashbots 发送失败: {}", error);
                Err(ExecutionError::FlashbotsError(error))
            }
        }
    }

    /// 获取 gas price
    async fn get_gas_price(&self) -> Result<U256, ExecutionError> {
        let base_price = self.provider.get_gas_price().await
            .map_err(|e| ExecutionError::ProviderError(format!("{:?}", e)))?;

        let adjusted_price = U256::from(
            (base_price.as_u128() as f64 * self.config.gas_strategy.gas_price_multiplier) as u128
        );

        Ok(adjusted_price)
    }

    /// 等待交易确认
    async fn wait_for_confirmation(&self, tx_hash: H256) -> Result<TransactionReceipt, ExecutionError> {
        let timeout = Duration::from_secs(self.config.confirmation_timeout_secs);
        let start = std::time::Instant::now();

        loop {
            if start.elapsed() > timeout {
                return Err(ExecutionError::Timeout);
            }

            match self.provider.get_transaction_receipt(tx_hash).await {
                Ok(Some(receipt)) => {
                    // 检查交易状态
                    if receipt.status == Some(U64::from(1)) {
                        info!("交易确认成功: {:?} | Block: {:?}", tx_hash, receipt.block_number);
                        return Ok(receipt);
                    } else {
                        // 交易 revert，尝试获取详细原因
                        let revert_reason = self.get_revert_reason(tx_hash, receipt.block_number).await;
                        let block_num = receipt.block_number.map(|n| n.as_u64()).unwrap_or(0);

                        error!("❌ 交易 Revert!");
                        error!("   交易哈希: {:?}", tx_hash);
                        error!("   区块号: {}", block_num);
                        error!("   Revert 原因: {}", revert_reason);

                        // 解码 revert 原因
                        let decoded = RevertDecoder::decode_from_error_string(&revert_reason);
                        error!("   解码后: {}", decoded);

                        return Err(ExecutionError::TransactionReverted(
                            format!("Transaction reverted in block {}: {}", block_num, decoded)
                        ));
                    }
                }
                Ok(None) => {
                    debug!("等待交易确认: {:?}", tx_hash);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(e) => {
                    warn!("获取交易回执失败: {:?}", e);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    /// 获取交易 revert 原因
    async fn get_revert_reason(&self, tx_hash: H256, block_number: Option<U64>) -> String {
        // 获取原始交易
        let tx = match self.provider.get_transaction(tx_hash).await {
            Ok(Some(tx)) => tx,
            Ok(None) => return "Unable to fetch transaction".to_string(),
            Err(e) => return format!("Error fetching transaction: {:?}", e),
        };

        // 构建 call 请求，在 revert 的区块重放交易
        let call_request = TransactionRequest {
            from: tx.from.into(),
            to: tx.to.map(|a| a.into()),
            gas: tx.gas.into(),
            gas_price: tx.gas_price.map(|p| p.into()),
            value: tx.value.into(),
            data: tx.input.clone().into(),
            nonce: tx.nonce.into(),
            chain_id: tx.chain_id.map(|c| c.as_u64().into()),
            ..Default::default()
        };

        // 在 revert 的区块号上 eth_call 重放
        let block_id = block_number.map(|n| BlockId::Number(BlockNumber::Number(n)));

        match self.provider.call(&call_request.into(), block_id).await {
            Ok(_) => "Transaction succeeded in replay (unexpected)".to_string(),
            Err(e) => {
                // 错误信息中包含 revert reason
                let error_str = format!("{:?}", e);

                // 尝试从错误中提取 revert data
                if let Some(data_start) = error_str.find("0x") {
                    // 找到 hex data 的结束位置
                    let data_end = error_str[data_start..]
                        .find(|c: char| !c.is_ascii_hexdigit() && c != 'x')
                        .map(|pos| data_start + pos)
                        .unwrap_or(error_str.len());

                    let hex_data = &error_str[data_start..data_end];
                    if hex_data.len() > 10 {
                        // 返回完整错误，让 RevertDecoder 解析
                        return error_str;
                    }
                }

                error_str
            }
        }
    }

    /// 解析执行结果
    async fn parse_execution_result(
        &self,
        tx_hash: H256,
        receipt: TransactionReceipt,
        params: &ArbitrageParams,
    ) -> Result<ExecutionResult, ExecutionError> {
        let gas_used = receipt.gas_used.unwrap_or_default();
        let effective_gas_price = receipt.effective_gas_price.unwrap_or_default();

        // 计算 gas 成本
        let gas_cost_wei = gas_used * effective_gas_price;
        let gas_cost_eth = Decimal::from_u128(gas_cost_wei.as_u128())
            .unwrap_or(Decimal::ZERO) / Decimal::from(1_000_000_000_000_000_000u64);

        // 从价格服务获取 ETH 价格
        let eth_price = self.get_native_token_price().await;
        let gas_cost_usd = gas_cost_eth * eth_price;

        // 解析事件日志获取实际利润
        let profit = self.parse_profit_from_logs(&receipt);

        // 计算利润 USD - 根据 token_a 获取价格并转换
        let profit_usd = self.calculate_profit_usd(params.token_a, profit).await;

        let net_profit_usd = profit_usd - gas_cost_usd;

        Ok(ExecutionResult {
            tx_hash,
            profit,
            profit_usd,
            gas_used,
            gas_cost_usd,
            net_profit_usd,
            success: true,
            block_number: receipt.block_number.map(|n| n.as_u64()).unwrap_or(0),
        })
    }

    /// 获取原生代币价格 (ETH/BNB)
    async fn get_native_token_price(&self) -> Decimal {
        if let Some(ref price_service) = self.price_service {
            // 根据链 ID 判断是 ETH 还是 BNB
            match self.config.chain_id {
                56 | 97 => price_service.get_bnb_price().await,  // BSC Mainnet / Testnet
                _ => price_service.get_eth_price().await,        // 默认 ETH
            }
        } else {
            // 无价格服务时使用默认值
            match self.config.chain_id {
                56 | 97 => Decimal::from(300),  // BNB 默认价格
                _ => Decimal::from(2000),       // ETH 默认价格
            }
        }
    }

    /// 计算利润的 USD 价值
    async fn calculate_profit_usd(&self, token_a: Address, profit: U256) -> Decimal {
        if profit.is_zero() {
            return Decimal::ZERO;
        }

        // 将 profit (wei) 转换为代币数量 (假设 18 位小数)
        let profit_decimal = Decimal::from_u128(profit.as_u128())
            .unwrap_or(Decimal::ZERO) / Decimal::from(1_000_000_000_000_000_000u64);

        // 获取 token_a 的 USD 价格
        let token_price = if let Some(ref price_service) = self.price_service {
            price_service.get_price_by_address(&token_a).await
                .unwrap_or(Decimal::ONE)  // 找不到价格默认 1 USD (可能是稳定币)
        } else {
            Decimal::ONE
        };

        profit_decimal * token_price
    }

    /// 从日志中解析利润
    fn parse_profit_from_logs(&self, receipt: &TransactionReceipt) -> U256 {
        // ArbitrageExecuted 事件签名
        // event ArbitrageExecuted(address indexed tokenA, address indexed tokenB, address indexed tokenC, uint256 amountIn, uint256 amountOut, uint256 profit)
        // keccak256("ArbitrageExecuted(address,address,address,uint256,uint256,uint256)")
        let event_signature: H256 = H256::from(keccak256(
            b"ArbitrageExecuted(address,address,address,uint256,uint256,uint256)"
        ));

        for log in &receipt.logs {
            if log.topics.first() == Some(&event_signature) {
                // 事件数据布局 (非索引参数):
                // - bytes 0-32: amountIn (uint256)
                // - bytes 32-64: amountOut (uint256)
                // - bytes 64-96: profit (uint256)
                if log.data.len() >= 96 {
                    // 解析 profit (第三个非索引参数，偏移 64 字节)
                    return U256::from_big_endian(&log.data[64..96]);
                }
            }
        }

        U256::zero()
    }

    /// 提取利润
    pub async fn withdraw_profit(
        &self,
        token: Address,
        to: Address,
        amount: U256,
    ) -> Result<H256, ExecutionError> {
        info!("提取利润: token={:?}, to={:?}, amount={}", token, to, amount);

        let call = self.contract.withdraw_profit(token, to, amount);

        let pending_tx = call.send().await
            .map_err(|e| ExecutionError::ContractError(format!("{:?}", e)))?;

        Ok(pending_tx.tx_hash())
    }

    /// 提取所有利润
    pub async fn withdraw_all_profit(
        &self,
        token: Address,
        to: Address,
    ) -> Result<H256, ExecutionError> {
        info!("提取所有利润: token={:?}, to={:?}", token, to);

        let call = self.contract.withdraw_all_profit(token, to);

        let pending_tx = call.send().await
            .map_err(|e| ExecutionError::ContractError(format!("{:?}", e)))?;

        Ok(pending_tx.tx_hash())
    }

    /// 检查合约所有者
    pub async fn check_owner(&self) -> Result<Address, ExecutionError> {
        self.contract.owner().call().await
            .map_err(|e| ExecutionError::ContractError(format!("{:?}", e)))
    }

    /// 获取合约中的代币余额
    pub async fn get_token_balance(&self, token: Address) -> Result<U256, ExecutionError> {
        // 使用 ERC20 balanceOf
        abigen!(
            IERC20,
            r#"[function balanceOf(address account) external view returns (uint256)]"#
        );

        let erc20 = IERC20::new(token, self.provider.clone());
        erc20.balance_of(self.config.contract_address).call().await
            .map_err(|e| ExecutionError::ContractError(format!("{:?}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ExecutorConfig::default();
        assert_eq!(config.chain_id, 1);
        assert!(config.simulate_before_execute);
    }
}
