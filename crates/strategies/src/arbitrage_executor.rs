use anyhow::{anyhow, Result};
use ethers::prelude::*;
use ethers::abi::{Token, encode};
use ethers::types::{Address, Bytes, H256, U256, TransactionRequest};
use ethers::types::transaction::eip2718::TypedTransaction;
use ethers::signers::Signer as EthersSigner;
use models::{ArbitrageOpportunity, ArbitragePath, ArbitrageResult, ArbitrageStatus, DexType};
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn, debug};
use serde::{Deserialize, Serialize};

/// 套利执行器配置
#[derive(Debug, Clone)]
pub struct ArbitrageExecutorConfig {
    pub arbitrage_contract: Option<Address>,
    /// 最大 Gas 价格 (Gwei) - 支持小数，如 0.08
    pub max_gas_price_gwei: f64,
    pub use_flashbots: bool,
    pub flashbots_rpc_url: Option<String>,
    pub dry_run: bool,
    /// 优先费（Gwei）- 支持小数，如 0.005
    pub priority_fee_gwei: f64,
    /// 交易截止时间偏移量 (秒)
    pub deadline_offset_secs: u64,
    /// 最大滑点百分比 (例如 0.5 = 0.5%)
    pub max_slippage_percent: f64,
    /// 是否在执行前进行模拟
    pub simulate_before_execute: bool,
    /// 最低利润阈值 (USD)
    pub min_profit_threshold_usd: f64,
}

impl Default for ArbitrageExecutorConfig {
    fn default() -> Self {
        Self {
            arbitrage_contract: None,
            max_gas_price_gwei: 100.0,
            use_flashbots: false,
            flashbots_rpc_url: Some("https://relay.flashbots.net".to_string()),
            dry_run: true,
            priority_fee_gwei: 2.0,
            deadline_offset_secs: 300, // 5 分钟
            max_slippage_percent: 0.5,
            simulate_before_execute: true,
            min_profit_threshold_usd: 10.0,
        }
    }
}

/// 原子套利执行参数
#[derive(Debug, Clone)]
pub struct AtomicArbitrageParams {
    /// 各跳交换参数
    pub swaps: Vec<SwapParams>,
    /// 最终最小输出金额 (原子性保护)
    pub min_final_output: U256,
    /// 交易截止时间
    pub deadline: U256,
    /// 闪电贷金额 (如果使用)
    pub flash_loan_amount: Option<U256>,
    /// 闪电贷提供者 (0=None, 1=Aave, 2=Uniswap)
    pub flash_loan_provider: u8,
}

/// Swap 参数结构 (用于合约调用)
#[derive(Debug, Clone)]
pub struct SwapParams {
    pub dex_type: u8,
    pub pool_address: Address,
    pub token_in: Address,
    pub token_out: Address,
    pub amount_in: U256,
    pub min_amount_out: U256,
    pub fee: u32,
}

/// 模拟执行结果
#[derive(Debug, Clone)]
pub struct SimulationResult {
    pub success: bool,
    pub output_amount: U256,
    pub gas_used: U256,
    pub error_message: Option<String>,
}

/// Flashbots bundle 请求
#[derive(Debug, Serialize)]
struct FlashbotsBundle {
    jsonrpc: String,
    id: u64,
    method: String,
    params: Vec<FlashbotsBundleParams>,
}

#[derive(Debug, Serialize)]
struct FlashbotsBundleParams {
    txs: Vec<String>,
    #[serde(rename = "blockNumber")]
    block_number: String,
}

/// Flashbots 响应
#[derive(Debug, Deserialize)]
struct FlashbotsResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: u64,
    result: Option<FlashbotsResult>,
    error: Option<FlashbotsError>,
}

#[derive(Debug, Deserialize)]
struct FlashbotsResult {
    #[serde(rename = "bundleHash")]
    bundle_hash: String,
}

#[derive(Debug, Deserialize)]
struct FlashbotsError {
    message: String,
}

/// 套利执行器
pub struct ArbitrageExecutor<M: Middleware, S: EthersSigner> {
    provider: Arc<M>,
    signer: Option<Arc<SignerMiddleware<Arc<M>, S>>>,
    wallet: Option<S>,
    config: ArbitrageExecutorConfig,
    pending_txs: RwLock<Vec<H256>>,
    results: RwLock<Vec<ArbitrageResult>>,
    http_client: reqwest::Client,
}

impl<M: Middleware + 'static, S: EthersSigner + Clone + 'static> ArbitrageExecutor<M, S> {
    pub fn new(provider: Arc<M>, wallet: Option<S>, config: ArbitrageExecutorConfig) -> Self {
        let signer = wallet.as_ref().map(|w| {
            Arc::new(SignerMiddleware::new(provider.clone(), w.clone()))
        });

        Self {
            provider,
            signer,
            wallet,
            config,
            pending_txs: RwLock::new(Vec::new()),
            results: RwLock::new(Vec::new()),
            http_client: reqwest::Client::new(),
        }
    }

    /// 执行套利交易
    pub async fn execute(&self, opportunity: ArbitrageOpportunity) -> Result<ArbitrageResult> {
        info!(
            "执行套利: id={}, 预期利润=${:.2}, 路径长度={}",
            opportunity.id,
            opportunity.net_profit_usd,
            opportunity.path.hops.len()
        );

        // 1. 基础检查
        self.perform_basic_checks(&opportunity).await?;

        // 2. 获取当前 gas 价格并检查
        let gas_price = self.provider.get_gas_price().await?;
        if let Some(result) = self.check_gas_price(&opportunity, gas_price).await? {
            return Ok(result);
        }

        // 3. 构建原子执行参数
        let atomic_params = self.build_atomic_params(&opportunity)?;
        let calldata = self.build_atomic_calldata(&atomic_params)?;

        // 4. 干运行模式
        if self.config.dry_run {
            return self.execute_dry_run(&opportunity, &calldata).await;
        }

        // 5. 执行前模拟 (eth_call)
        if self.config.simulate_before_execute {
            let simulation = self.simulate_execution(&opportunity, &calldata).await?;
            if !simulation.success {
                warn!("模拟执行失败: {:?}", simulation.error_message);
                return Ok(ArbitrageResult {
                    opportunity: opportunity.clone(),
                    tx_hash: None,
                    status: ArbitrageStatus::Failed,
                    actual_profit: None,
                    actual_gas_used: Some(simulation.gas_used),
                    error_message: simulation.error_message,
                    executed_at: chrono::Utc::now(),
                });
            }

            // 检查模拟输出是否满足最低利润
            if simulation.output_amount < opportunity.input_amount {
                return Ok(ArbitrageResult {
                    opportunity: opportunity.clone(),
                    tx_hash: None,
                    status: ArbitrageStatus::Failed,
                    actual_profit: None,
                    actual_gas_used: Some(simulation.gas_used),
                    error_message: Some("模拟显示输出低于输入".to_string()),
                    executed_at: chrono::Utc::now(),
                });
            }

            info!("模拟执行成功: 预计输出={}, gas={}", simulation.output_amount, simulation.gas_used);
        }

        // 6. 发送实际交易
        let result = if self.config.use_flashbots {
            self.send_via_flashbots(&opportunity, calldata, gas_price).await
        } else {
            self.send_transaction(&opportunity, calldata, gas_price).await
        };

        self.handle_execution_result(&opportunity, result).await
    }

    /// 基础检查
    async fn perform_basic_checks(&self, _opportunity: &ArbitrageOpportunity) -> Result<()> {
        // 检查是否有签名者
        if self.signer.is_none() && !self.config.dry_run {
            return Err(anyhow!("未配置签名者，无法执行交易"));
        }

        // 检查合约地址
        if self.config.arbitrage_contract.is_none() && !self.config.dry_run {
            return Err(anyhow!("未配置套利合约地址"));
        }

        Ok(())
    }

    /// 检查 gas 价格
    async fn check_gas_price(&self, opportunity: &ArbitrageOpportunity, gas_price: U256) -> Result<Option<ArbitrageResult>> {
        // 支持小数 gwei，如 0.08
        let max_gas_wei = (self.config.max_gas_price_gwei * 1e9) as u128;
        let max_gas = U256::from(max_gas_wei);

        if gas_price > max_gas {
            warn!(
                "Gas 价格过高: {} > {} Gwei",
                gas_price / U256::exp10(9),
                self.config.max_gas_price_gwei
            );
            return Ok(Some(ArbitrageResult {
                opportunity: opportunity.clone(),
                tx_hash: None,
                status: ArbitrageStatus::Failed,
                actual_profit: None,
                actual_gas_used: None,
                error_message: Some(format!(
                    "Gas 价格过高: {} > {} Gwei",
                    gas_price / U256::exp10(9),
                    self.config.max_gas_price_gwei
                )),
                executed_at: chrono::Utc::now(),
            }));
        }

        Ok(None)
    }

    /// 构建原子执行参数
    fn build_atomic_params(&self, opportunity: &ArbitrageOpportunity) -> Result<AtomicArbitrageParams> {
        let mut swaps = Vec::new();
        let mut current_amount = opportunity.input_amount;
        let slippage_factor = U256::from(((1.0 - self.config.max_slippage_percent / 100.0) * 10000.0) as u64);

        for hop in &opportunity.path.hops {
            let dex_type = dex_type_to_u8(hop.dex_type);

            // 根据 DEX 类型计算预期输出
            let expected_output = self.estimate_hop_output(hop.dex_type, current_amount, hop.fee);

            // 计算最小输出 (考虑滑点)
            let min_amount_out = expected_output * slippage_factor / U256::from(10000);

            swaps.push(SwapParams {
                dex_type,
                pool_address: hop.pool_address,
                token_in: hop.token_in,
                token_out: hop.token_out,
                amount_in: current_amount,
                min_amount_out,
                fee: hop.fee,
            });

            // 更新下一跳的输入金额
            current_amount = min_amount_out;
        }

        // 计算最终最小输出 (必须大于输入以确保盈利)
        let min_profit_wei = self.usd_to_wei(self.config.min_profit_threshold_usd);
        let min_final_output = opportunity.input_amount + min_profit_wei;

        // 计算截止时间
        let deadline = U256::from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() + self.config.deadline_offset_secs
        );

        Ok(AtomicArbitrageParams {
            swaps,
            min_final_output,
            deadline,
            flash_loan_amount: Some(opportunity.input_amount), // 默认使用闪电贷
            flash_loan_provider: 1, // 1 = Aave
        })
    }

    /// 估算单跳输出
    fn estimate_hop_output(&self, dex_type: DexType, amount_in: U256, fee: u32) -> U256 {
        match dex_type {
            DexType::UniswapV2 | DexType::SushiSwap | DexType::SushiSwapV2 | DexType::PancakeSwapV2 => {
                // V2: 0.3% fee (997/1000)
                amount_in * U256::from(997) / U256::from(1000)
            }
            DexType::UniswapV3 | DexType::PancakeSwapV3 | DexType::SushiSwapV3 => {
                // V3: fee in 1/1000000
                let fee_factor = U256::from(1_000_000 - fee);
                amount_in * fee_factor / U256::from(1_000_000)
            }
            DexType::UniswapV4 => {
                // V4: 更高效
                let fee_factor = U256::from(1_000_000 - fee);
                amount_in * fee_factor / U256::from(1_000_000)
            }
            DexType::Curve => {
                // Curve: 假设 0.04% fee
                amount_in * U256::from(9996) / U256::from(10000)
            }
        }
    }

    /// USD 转 Wei (简化)
    fn usd_to_wei(&self, usd: f64) -> U256 {
        // 假设 ETH = $2000, 转换为 wei
        let eth_amount = usd / 2000.0;
        let wei = eth_amount * 1e18;
        U256::from(wei as u64)
    }

    /// 构建原子执行 calldata
    ///
    /// 套利合约接口:
    /// function executeAtomicArbitrage(
    ///     SwapParams[] calldata swaps,
    ///     uint256 minFinalOutput,
    ///     uint256 deadline,
    ///     uint256 flashLoanAmount,
    ///     uint8 flashLoanProvider
    /// ) external returns (uint256 actualOutput)
    fn build_atomic_calldata(&self, params: &AtomicArbitrageParams) -> Result<Bytes> {
        // 函数选择器
        let function_selector = &ethers::utils::keccak256(
            "executeAtomicArbitrage(bytes[],uint256,uint256,uint256,uint8)"
        )[..4];

        // 构建 swap 参数列表
        let mut swap_data_list: Vec<Token> = Vec::new();

        for swap in &params.swaps {
            // 编码单个 swap 参数
            let swap_encoded = encode(&[
                Token::Uint(U256::from(swap.dex_type)),
                Token::Address(swap.pool_address),
                Token::Address(swap.token_in),
                Token::Address(swap.token_out),
                Token::Uint(swap.amount_in),
                Token::Uint(swap.min_amount_out),
                Token::Uint(U256::from(swap.fee)),
            ]);

            swap_data_list.push(Token::Bytes(swap_encoded));
        }

        // 编码完整的 calldata
        let full_params = encode(&[
            Token::Array(swap_data_list),
            Token::Uint(params.min_final_output),
            Token::Uint(params.deadline),
            Token::Uint(params.flash_loan_amount.unwrap_or(U256::zero())),
            Token::Uint(U256::from(params.flash_loan_provider)),
        ]);

        let mut calldata = Vec::with_capacity(4 + full_params.len());
        calldata.extend_from_slice(function_selector);
        calldata.extend_from_slice(&full_params);

        Ok(Bytes::from(calldata))
    }

    /// 干运行模式执行
    async fn execute_dry_run(&self, opportunity: &ArbitrageOpportunity, calldata: &Bytes) -> Result<ArbitrageResult> {
        info!("干运行模式: 跳过实际执行");
        debug!("Calldata 长度: {} bytes", calldata.len());

        Ok(ArbitrageResult {
            opportunity: opportunity.clone(),
            tx_hash: None,
            status: ArbitrageStatus::Pending,
            actual_profit: None,
            actual_gas_used: None,
            error_message: Some("干运行模式".to_string()),
            executed_at: chrono::Utc::now(),
        })
    }

    /// 模拟执行 (使用 eth_call)
    ///
    /// 重要：必须设置正确的 from 地址，否则合约的权限检查（如 onlyOwner）会失败
    async fn simulate_execution(&self, opportunity: &ArbitrageOpportunity, calldata: &Bytes) -> Result<SimulationResult> {
        let contract_address = self.config.arbitrage_contract
            .ok_or_else(|| anyhow!("未配置套利合约地址"))?;

        // 获取发送者地址（必须是合约 owner）
        // 重要：如果没有配置签名者，使用 Address::zero() 会导致 onlyOwner 等权限检查失败
        let from_address = if let Some(ref signer) = self.signer {
            signer.address()
        } else {
            // 干运行模式：警告用户可能的权限问题
            warn!("⚠️ 模拟执行未配置签名者，使用 Address::zero() 作为 from 地址");
            warn!("⚠️ 如果合约有 onlyOwner 等权限检查，模拟可能会失败");
            warn!("⚠️ 错误码 0x118cdaa7 (OwnableUnauthorizedAccount) 表示权限不足");
            Address::zero()
        };

        // 构建模拟交易
        let tx = TransactionRequest::new()
            .to(contract_address)
            .from(from_address)  // 关键：必须设置为 owner 地址
            .data(calldata.clone())
            .gas(opportunity.gas_estimate * U256::from(2)); // 使用更高的 gas limit 进行模拟

        debug!("模拟执行: to={:?}, from={:?}", contract_address, from_address);

        // 执行 eth_call
        match self.provider.call(&tx.clone().into(), None).await {
            Ok(result) => {
                // 解析返回值 (假设返回 uint256 actualOutput)
                let output_amount = if result.len() >= 32 {
                    U256::from_big_endian(&result[0..32])
                } else {
                    U256::zero()
                };

                // 估算 gas
                let gas_used = match self.provider.estimate_gas(&tx.into(), None).await {
                    Ok(gas) => gas,
                    Err(_) => opportunity.gas_estimate,
                };

                Ok(SimulationResult {
                    success: true,
                    output_amount,
                    gas_used,
                    error_message: None,
                })
            }
            Err(e) => {
                let error_msg = e.to_string();

                // 尝试解析 revert 原因
                let reason = self.parse_revert_reason(&error_msg);

                Ok(SimulationResult {
                    success: false,
                    output_amount: U256::zero(),
                    gas_used: opportunity.gas_estimate,
                    error_message: Some(reason),
                })
            }
        }
    }

    /// 解析 revert 原因
    fn parse_revert_reason(&self, error: &str) -> String {
        // 尝试从错误消息中提取 revert 原因
        if error.contains("execution reverted") {
            if error.contains("InsufficientOutput") {
                return "输出金额不足".to_string();
            }
            if error.contains("Expired") || error.contains("deadline") {
                return "交易已过期".to_string();
            }
            if error.contains("SlippageExceeded") {
                return "滑点超出限制".to_string();
            }
            if error.contains("InsufficientLiquidity") {
                return "流动性不足".to_string();
            }
        }

        // 返回原始错误
        error.to_string()
    }

    /// 发送普通交易
    async fn send_transaction(
        &self,
        opportunity: &ArbitrageOpportunity,
        calldata: Bytes,
        gas_price: U256,
    ) -> Result<H256> {
        let signer = self.signer.as_ref()
            .ok_or_else(|| anyhow!("未配置签名者"))?;

        let contract_address = self.config.arbitrage_contract
            .ok_or_else(|| anyhow!("未配置套利合约地址"))?;

        // 获取 nonce
        let from_address = signer.address();
        let nonce = self.provider.get_transaction_count(from_address, None).await?;

        // 计算 EIP-1559 gas 参数 (支持小数 gwei，如 0.005)
        let priority_fee_wei = (self.config.priority_fee_gwei * 1e9) as u128;
        let priority_fee = U256::from(priority_fee_wei);
        let max_fee = gas_price + priority_fee;

        // 构建 EIP-1559 交易
        let tx = Eip1559TransactionRequest::new()
            .to(contract_address)
            .data(calldata)
            .gas(opportunity.gas_estimate)
            .max_fee_per_gas(max_fee)
            .max_priority_fee_per_gas(priority_fee)
            .nonce(nonce)
            .chain_id(opportunity.path.chain_id);

        info!(
            "发送交易: to={:?}, gas={}, max_fee={} Gwei, priority_fee={} Gwei",
            contract_address,
            opportunity.gas_estimate,
            max_fee / U256::exp10(9),
            priority_fee / U256::exp10(9)
        );

        // 发送交易
        let pending_tx = signer.send_transaction(tx, None).await?;
        let tx_hash = pending_tx.tx_hash();

        info!("交易已发送: {:?}", tx_hash);

        Ok(tx_hash)
    }

    /// 通过 Flashbots 发送交易 (MEV 保护)
    async fn send_via_flashbots(
        &self,
        opportunity: &ArbitrageOpportunity,
        calldata: Bytes,
        gas_price: U256,
    ) -> Result<H256> {
        let wallet = self.wallet.as_ref()
            .ok_or_else(|| anyhow!("未配置钱包"))?;

        let contract_address = self.config.arbitrage_contract
            .ok_or_else(|| anyhow!("未配置套利合约地址"))?;

        let flashbots_url = self.config.flashbots_rpc_url.as_ref()
            .ok_or_else(|| anyhow!("未配置 Flashbots RPC URL"))?;

        // 获取当前区块号
        let current_block = self.provider.get_block_number().await?;
        let target_block = current_block + 1;

        // 获取 nonce
        let from_address = wallet.address();
        let nonce = self.provider.get_transaction_count(from_address, None).await?;

        // 计算 gas 参数 (支持小数 gwei，如 0.005)
        let priority_fee_wei = (self.config.priority_fee_gwei * 1e9) as u128;
        let priority_fee = U256::from(priority_fee_wei);
        let max_fee = gas_price + priority_fee;

        // 构建交易
        let tx = TransactionRequest::new()
            .to(contract_address)
            .data(calldata)
            .gas(opportunity.gas_estimate)
            .gas_price(max_fee)
            .nonce(nonce)
            .chain_id(opportunity.path.chain_id);

        // 签名交易
        let typed_tx: TypedTransaction = tx.into();
        let signature = wallet.sign_transaction(&typed_tx).await?;
        let signed_tx = typed_tx.rlp_signed(&signature);
        let signed_tx_hex = format!("0x{}", hex::encode(&signed_tx));

        info!("Flashbots: 目标区块 {}, priority_fee={} Gwei", target_block, self.config.priority_fee_gwei);

        // 构建 Flashbots bundle 请求
        let bundle = FlashbotsBundle {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "eth_sendBundle".to_string(),
            params: vec![FlashbotsBundleParams {
                txs: vec![signed_tx_hex],
                block_number: format!("0x{:x}", target_block),
            }],
        };

        // 发送到 Flashbots relay
        let response = self.http_client
            .post(flashbots_url)
            .header("Content-Type", "application/json")
            .json(&bundle)
            .send()
            .await?;

        let flashbots_response: FlashbotsResponse = response.json().await?;

        if let Some(error) = flashbots_response.error {
            return Err(anyhow!("Flashbots 错误: {}", error.message));
        }

        if let Some(result) = flashbots_response.result {
            info!("Flashbots bundle 已提交: {}", result.bundle_hash);

            // 计算交易哈希
            let tx_hash = ethers::utils::keccak256(&signed_tx);
            return Ok(H256::from_slice(&tx_hash));
        }

        Err(anyhow!("Flashbots 响应无效"))
    }

    /// 处理执行结果
    async fn handle_execution_result(&self, opportunity: &ArbitrageOpportunity, result: Result<H256>) -> Result<ArbitrageResult> {
        match result {
            Ok(tx_hash) => {
                info!("交易已提交: {:?}", tx_hash);

                let mut pending = self.pending_txs.write().await;
                pending.push(tx_hash);

                Ok(ArbitrageResult {
                    opportunity: opportunity.clone(),
                    tx_hash: Some(tx_hash),
                    status: ArbitrageStatus::Submitted,
                    actual_profit: None,
                    actual_gas_used: None,
                    error_message: None,
                    executed_at: chrono::Utc::now(),
                })
            }
            Err(e) => {
                error!("交易执行失败: {}", e);
                Ok(ArbitrageResult {
                    opportunity: opportunity.clone(),
                    tx_hash: None,
                    status: ArbitrageStatus::Failed,
                    actual_profit: None,
                    actual_gas_used: None,
                    error_message: Some(e.to_string()),
                    executed_at: chrono::Utc::now(),
                })
            }
        }
    }

    /// 检查待确认的交易
    pub async fn check_pending_transactions(&self) -> Result<Vec<ArbitrageResult>> {
        let mut pending = self.pending_txs.write().await;
        let mut confirmed_results = Vec::new();
        let mut still_pending = Vec::new();

        for tx_hash in pending.iter() {
            match self.provider.get_transaction_receipt(*tx_hash).await? {
                Some(receipt) => {
                    let status = if receipt.status == Some(1.into()) {
                        ArbitrageStatus::Confirmed
                    } else {
                        ArbitrageStatus::Reverted
                    };

                    info!(
                        "交易 {:?} 状态: {:?}, gas_used: {:?}",
                        tx_hash,
                        status,
                        receipt.gas_used
                    );

                    // 解析实际利润 (从事件日志)
                    let actual_profit = self.parse_profit_from_logs(&receipt);

                    let result = ArbitrageResult {
                        opportunity: create_placeholder_opportunity(tx_hash.to_string()),
                        tx_hash: Some(*tx_hash),
                        status,
                        actual_profit,
                        actual_gas_used: receipt.gas_used,
                        error_message: None,
                        executed_at: chrono::Utc::now(),
                    };

                    confirmed_results.push(result);
                }
                None => {
                    // 交易仍在 pending
                    still_pending.push(*tx_hash);
                }
            }
        }

        *pending = still_pending;

        Ok(confirmed_results)
    }

    /// 从交易日志解析实际利润
    fn parse_profit_from_logs(&self, receipt: &TransactionReceipt) -> Option<U256> {
        // 查找 ArbitrageExecuted 事件
        // event ArbitrageExecuted(address indexed executor, uint256 profit, uint256 gasUsed)
        let event_signature = ethers::utils::keccak256("ArbitrageExecuted(address,uint256,uint256)");

        for log in &receipt.logs {
            if log.topics.first() == Some(&H256::from_slice(&event_signature)) {
                // 解析利润 (第一个非索引参数)
                if log.data.len() >= 32 {
                    let profit = U256::from_big_endian(&log.data[0..32]);
                    return Some(profit);
                }
            }
        }

        None
    }

    /// 获取执行结果
    pub async fn take_results(&self) -> Vec<ArbitrageResult> {
        let mut results = self.results.write().await;
        std::mem::take(&mut *results)
    }

    /// 估算交易 gas
    pub async fn estimate_gas(&self, opportunity: &ArbitrageOpportunity) -> Result<U256> {
        if self.config.arbitrage_contract.is_none() {
            return Ok(opportunity.gas_estimate);
        }

        let params = self.build_atomic_params(opportunity)?;
        let calldata = self.build_atomic_calldata(&params)?;

        let contract_address = self.config.arbitrage_contract.unwrap();
        let tx = TransactionRequest::new()
            .to(contract_address)
            .data(calldata);

        match self.provider.estimate_gas(&tx.into(), None).await {
            Ok(gas) => {
                // 添加 20% 缓冲
                Ok(gas * U256::from(120) / U256::from(100))
            }
            Err(_) => Ok(opportunity.gas_estimate),
        }
    }

    /// 获取配置
    pub fn config(&self) -> &ArbitrageExecutorConfig {
        &self.config
    }
}

/// DEX 类型转换为 u8
fn dex_type_to_u8(dex_type: DexType) -> u8 {
    match dex_type {
        DexType::UniswapV2 => 0,
        DexType::UniswapV3 => 1,
        DexType::UniswapV4 => 2,
        DexType::SushiSwap => 3,
        DexType::SushiSwapV2 => 3,  // 与 SushiSwap 相同
        DexType::SushiSwapV3 => 7,  // SushiSwap V3
        DexType::PancakeSwapV2 => 4,
        DexType::PancakeSwapV3 => 5,
        DexType::Curve => 6,
    }
}

/// 创建默认的 ArbitrageOpportunity (用于结果中的占位符)
fn create_placeholder_opportunity(id: String) -> ArbitrageOpportunity {
    ArbitrageOpportunity {
        id,
        path: ArbitragePath::new(Address::zero(), 1),
        input_amount: U256::zero(),
        expected_output: U256::zero(),
        expected_profit: U256::zero(),
        expected_profit_usd: Decimal::ZERO,
        gas_estimate: U256::zero(),
        gas_cost_usd: Decimal::ZERO,
        net_profit_usd: Decimal::ZERO,
        profit_percentage: Decimal::ZERO,
        timestamp: chrono::Utc::now(),
        block_number: 0,
    }
}
