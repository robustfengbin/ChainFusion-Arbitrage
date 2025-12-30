use anyhow::{anyhow, Result};
use ethers::prelude::*;
use ethers::types::{Address, Bytes, U256, H256};
use std::sync::Arc;
use tracing::info;

use super::providers::FlashLoanProvider;

/// 闪电贷套利执行器
#[allow(dead_code)]
pub struct FlashLoanExecutor<M: Middleware, S: Signer> {
    provider: Arc<M>,
    signer: Option<Arc<S>>,
    default_provider: FlashLoanProvider,
    /// 套利合约地址
    arbitrage_contract: Option<Address>,
    /// 最小利润阈值 (USD, 以 1e18 为基数)
    min_profit_threshold: U256,
    /// 最大 gas 价格 (gwei)
    max_gas_price: U256,
}

impl<M: Middleware + 'static, S: Signer + 'static> FlashLoanExecutor<M, S> {
    pub fn new(provider: Arc<M>, default_provider: FlashLoanProvider) -> Self {
        Self {
            provider,
            signer: None,
            default_provider,
            arbitrage_contract: None,
            min_profit_threshold: U256::from(10u64) * U256::exp10(18), // $10
            max_gas_price: U256::from(100u64) * U256::exp10(9), // 100 gwei
        }
    }

    pub fn with_signer(mut self, signer: Arc<S>) -> Self {
        self.signer = Some(signer);
        self
    }

    pub fn with_arbitrage_contract(mut self, contract: Address) -> Self {
        self.arbitrage_contract = Some(contract);
        self
    }

    pub fn with_min_profit(mut self, min_profit: U256) -> Self {
        self.min_profit_threshold = min_profit;
        self
    }

    pub fn with_max_gas_price(mut self, max_gas_price: U256) -> Self {
        self.max_gas_price = max_gas_price;
        self
    }

    /// 执行闪电贷套利
    pub async fn execute_arbitrage(
        &self,
        request: ArbitrageRequest,
    ) -> Result<ArbitrageResult> {
        info!(
            "开始执行闪电贷套利: provider={}, token={:?}, amount={}",
            request.flash_provider.name(),
            request.borrow_token,
            request.borrow_amount
        );

        // 验证配置
        let contract = self.arbitrage_contract
            .ok_or_else(|| anyhow!("未配置套利合约地址"))?;

        // 根据提供商执行不同的闪电贷
        let result = match request.flash_provider {
            FlashLoanProvider::UniswapV3 => {
                self.execute_v3_flash(contract, &request).await?
            }
            FlashLoanProvider::UniswapV4 => {
                self.execute_v4_flash(contract, &request).await?
            }
            FlashLoanProvider::AaveV3 => {
                self.execute_aave_flash(contract, &request).await?
            }
            FlashLoanProvider::Balancer => {
                self.execute_balancer_flash(contract, &request).await?
            }
        };

        Ok(result)
    }

    /// 执行 Uniswap V3 闪电贷
    async fn execute_v3_flash(
        &self,
        _contract: Address,
        request: &ArbitrageRequest,
    ) -> Result<ArbitrageResult> {
        let pool_address = request.v3_pool_address
            .ok_or_else(|| anyhow!("V3 闪电贷需要指定池子地址"))?;

        // 编码回调数据
        let callback_data = self.encode_v3_callback(&request.swap_path)?;

        info!(
            "执行 V3 闪电贷: pool={:?}, amount={}",
            pool_address, request.borrow_amount
        );

        // 构建闪电贷交易
        let flash_data = FlashLoanCallData {
            pool: pool_address,
            amount0: if request.is_token0 { request.borrow_amount } else { U256::zero() },
            amount1: if request.is_token0 { U256::zero() } else { request.borrow_amount },
            callback_data,
        };

        // 估算 gas
        let estimated_gas = self.estimate_gas(&flash_data).await?;

        Ok(ArbitrageResult {
            success: true,
            tx_hash: None, // 实际执行时填充
            profit: U256::zero(),
            gas_used: estimated_gas,
            provider: FlashLoanProvider::UniswapV3,
        })
    }

    /// 执行 Uniswap V4 Flash Accounting
    async fn execute_v4_flash(
        &self,
        _contract: Address,
        request: &ArbitrageRequest,
    ) -> Result<ArbitrageResult> {
        info!("执行 V4 Flash Accounting 套利");

        // V4 不需要真正的"闪电贷"，使用 unlock -> swap -> settle 模式
        let _unlock_data = self.encode_v4_unlock_callback(request)?;

        Ok(ArbitrageResult {
            success: true,
            tx_hash: None,
            profit: U256::zero(),
            gas_used: U256::from(200000u64), // 估算
            provider: FlashLoanProvider::UniswapV4,
        })
    }

    /// 执行 Aave V3 闪电贷
    async fn execute_aave_flash(
        &self,
        _contract: Address,
        request: &ArbitrageRequest,
    ) -> Result<ArbitrageResult> {
        let _pool_address = request.aave_pool_address
            .ok_or_else(|| anyhow!("Aave 闪电贷需要指定 Pool 地址"))?;

        info!(
            "执行 Aave V3 闪电贷: token={:?}, amount={}",
            request.borrow_token, request.borrow_amount
        );

        Ok(ArbitrageResult {
            success: true,
            tx_hash: None,
            profit: U256::zero(),
            gas_used: U256::from(300000u64),
            provider: FlashLoanProvider::AaveV3,
        })
    }

    /// 执行 Balancer 闪电贷
    async fn execute_balancer_flash(
        &self,
        _contract: Address,
        request: &ArbitrageRequest,
    ) -> Result<ArbitrageResult> {
        info!(
            "执行 Balancer 闪电贷: token={:?}, amount={}",
            request.borrow_token, request.borrow_amount
        );

        Ok(ArbitrageResult {
            success: true,
            tx_hash: None,
            profit: U256::zero(),
            gas_used: U256::from(250000u64),
            provider: FlashLoanProvider::Balancer,
        })
    }

    /// 编码 V3 闪电贷回调数据
    fn encode_v3_callback(&self, swap_path: &[SwapStep]) -> Result<Vec<u8>> {
        use ethers::abi::{encode, Token};

        let path_tokens: Vec<Token> = swap_path
            .iter()
            .map(|step| {
                Token::Tuple(vec![
                    Token::Address(step.token_in),
                    Token::Address(step.token_out),
                    Token::Address(step.pool),
                    Token::Uint(step.fee.into()),
                    Token::Uint(step.dex_type.into()),
                ])
            })
            .collect();

        let encoded = encode(&[
            Token::Array(path_tokens),
            Token::Uint(self.min_profit_threshold),
        ]);

        Ok(encoded)
    }

    /// 编码 V4 unlock 回调数据
    fn encode_v4_unlock_callback(&self, request: &ArbitrageRequest) -> Result<Vec<u8>> {
        use ethers::abi::{encode, Token};

        // 将 swap 路径转换为 V4 格式
        let swap_tokens: Vec<Token> = request.swap_path
            .iter()
            .map(|step| {
                Token::Tuple(vec![
                    Token::Address(step.token_in),
                    Token::Address(step.token_out),
                    Token::Uint(step.fee.into()),
                    Token::Bool(step.token_in < step.token_out), // zero_for_one
                    Token::Uint(step.amount_in),
                ])
            })
            .collect();

        let encoded = encode(&[
            Token::Array(swap_tokens),
            Token::Uint(self.min_profit_threshold),
        ]);

        Ok(encoded)
    }

    /// 估算 gas
    async fn estimate_gas(&self, _data: &FlashLoanCallData) -> Result<U256> {
        // TODO: 实际调用 eth_estimateGas
        Ok(U256::from(500000u64))
    }

    /// 构建完整的套利交易
    pub fn build_arbitrage_transaction(
        &self,
        request: &ArbitrageRequest,
    ) -> Result<ArbitrageTransaction> {
        let contract = self.arbitrage_contract
            .ok_or_else(|| anyhow!("未配置套利合约地址"))?;

        // 根据提供商构建不同的调用数据
        let calldata = match request.flash_provider {
            FlashLoanProvider::UniswapV3 => {
                self.build_v3_calldata(request)?
            }
            FlashLoanProvider::UniswapV4 => {
                self.build_v4_calldata(request)?
            }
            FlashLoanProvider::AaveV3 => {
                self.build_aave_calldata(request)?
            }
            FlashLoanProvider::Balancer => {
                self.build_balancer_calldata(request)?
            }
        };

        Ok(ArbitrageTransaction {
            to: contract,
            value: U256::zero(),
            data: calldata,
            gas_limit: U256::from(1000000u64),
        })
    }

    fn build_v3_calldata(&self, request: &ArbitrageRequest) -> Result<Bytes> {
        use ethers::abi::{encode, Token};

        let pool = request.v3_pool_address.unwrap_or(Address::zero());
        let (amount0, amount1) = if request.is_token0 {
            (request.borrow_amount, U256::zero())
        } else {
            (U256::zero(), request.borrow_amount)
        };

        let callback_data = self.encode_v3_callback(&request.swap_path)?;

        // 函数签名: executeV3Flash(address pool, uint256 amount0, uint256 amount1, bytes callback)
        let function_selector = ethers::utils::keccak256("executeV3Flash(address,uint256,uint256,bytes)")[..4].to_vec();

        let params = encode(&[
            Token::Address(pool),
            Token::Uint(amount0),
            Token::Uint(amount1),
            Token::Bytes(callback_data),
        ]);

        let mut calldata = function_selector;
        calldata.extend(params);

        Ok(Bytes::from(calldata))
    }

    fn build_v4_calldata(&self, request: &ArbitrageRequest) -> Result<Bytes> {
        use ethers::abi::{encode, Token};

        let unlock_data = self.encode_v4_unlock_callback(request)?;

        // 函数签名: executeV4Arbitrage(bytes unlockData)
        let function_selector = ethers::utils::keccak256("executeV4Arbitrage(bytes)")[..4].to_vec();

        let params = encode(&[Token::Bytes(unlock_data)]);

        let mut calldata = function_selector;
        calldata.extend(params);

        Ok(Bytes::from(calldata))
    }

    fn build_aave_calldata(&self, request: &ArbitrageRequest) -> Result<Bytes> {
        use ethers::abi::{encode, Token};

        // 函数签名: executeAaveFlash(address token, uint256 amount, bytes params)
        let function_selector = ethers::utils::keccak256("executeAaveFlash(address,uint256,bytes)")[..4].to_vec();

        let callback_data = self.encode_v3_callback(&request.swap_path)?;

        let params = encode(&[
            Token::Address(request.borrow_token),
            Token::Uint(request.borrow_amount),
            Token::Bytes(callback_data),
        ]);

        let mut calldata = function_selector;
        calldata.extend(params);

        Ok(Bytes::from(calldata))
    }

    fn build_balancer_calldata(&self, request: &ArbitrageRequest) -> Result<Bytes> {
        use ethers::abi::{encode, Token};

        // 函数签名: executeBalancerFlash(address[] tokens, uint256[] amounts, bytes userData)
        let function_selector = ethers::utils::keccak256("executeBalancerFlash(address[],uint256[],bytes)")[..4].to_vec();

        let callback_data = self.encode_v3_callback(&request.swap_path)?;

        let params = encode(&[
            Token::Array(vec![Token::Address(request.borrow_token)]),
            Token::Array(vec![Token::Uint(request.borrow_amount)]),
            Token::Bytes(callback_data),
        ]);

        let mut calldata = function_selector;
        calldata.extend(params);

        Ok(Bytes::from(calldata))
    }

    /// 模拟套利交易
    pub async fn simulate_arbitrage(
        &self,
        request: &ArbitrageRequest,
    ) -> Result<SimulationResult> {
        let _tx = self.build_arbitrage_transaction(request)?;

        // TODO: 使用 eth_call 模拟交易
        // 这里返回一个模拟结果

        Ok(SimulationResult {
            success: true,
            expected_profit: request.expected_profit,
            gas_estimate: U256::from(500000u64),
            error_message: None,
        })
    }
}

/// 套利请求
#[derive(Debug, Clone)]
pub struct ArbitrageRequest {
    pub flash_provider: FlashLoanProvider,
    pub borrow_token: Address,
    pub borrow_amount: U256,
    pub is_token0: bool,
    pub swap_path: Vec<SwapStep>,
    pub expected_profit: U256,
    /// Uniswap V3 池子地址 (用于 V3 闪电贷)
    pub v3_pool_address: Option<Address>,
    /// Aave V3 Pool 地址
    pub aave_pool_address: Option<Address>,
    /// 闪电贷池的费率 (用于 V3: 100=0.01%, 500=0.05%, 3000=0.3%, 10000=1%)
    pub flash_pool_fee: Option<u32>,
}

impl ArbitrageRequest {
    pub fn new(
        flash_provider: FlashLoanProvider,
        borrow_token: Address,
        borrow_amount: U256,
    ) -> Self {
        Self {
            flash_provider,
            borrow_token,
            borrow_amount,
            is_token0: true,
            swap_path: vec![],
            expected_profit: U256::zero(),
            v3_pool_address: None,
            aave_pool_address: None,
            flash_pool_fee: None,
        }
    }

    pub fn with_swap_path(mut self, path: Vec<SwapStep>) -> Self {
        self.swap_path = path;
        self
    }

    pub fn with_v3_pool(mut self, pool: Address) -> Self {
        self.v3_pool_address = Some(pool);
        self
    }

    /// 设置 V3 闪电贷池及其费率
    pub fn with_v3_pool_and_fee(mut self, pool: Address, fee: u32) -> Self {
        self.v3_pool_address = Some(pool);
        self.flash_pool_fee = Some(fee);
        self
    }

    /// 设置闪电贷池费率 (用于精确计算闪电贷成本)
    pub fn with_flash_pool_fee(mut self, fee: u32) -> Self {
        self.flash_pool_fee = Some(fee);
        self
    }

    pub fn with_aave_pool(mut self, pool: Address) -> Self {
        self.aave_pool_address = Some(pool);
        self
    }

    pub fn with_expected_profit(mut self, profit: U256) -> Self {
        self.expected_profit = profit;
        self
    }

    /// 获取闪电贷费率
    /// 优先使用明确设置的费率，否则使用提供商默认费率
    pub fn get_flash_loan_fee(&self) -> u32 {
        self.flash_pool_fee.unwrap_or_else(|| self.flash_provider.fee_rate())
    }

    /// 计算闪电贷费用
    pub fn calculate_flash_loan_fee(&self) -> U256 {
        let fee_rate = self.get_flash_loan_fee() as u128;
        (self.borrow_amount * U256::from(fee_rate)) / U256::from(1_000_000u128)
    }

    /// 计算需要归还的总金额 (本金 + 费用)
    pub fn calculate_repay_amount(&self) -> U256 {
        self.borrow_amount + self.calculate_flash_loan_fee()
    }
}

/// Swap 步骤
#[derive(Debug, Clone)]
pub struct SwapStep {
    pub token_in: Address,
    pub token_out: Address,
    pub pool: Address,
    pub fee: u32,
    pub dex_type: u8, // 0 = V2, 1 = V3, 2 = V4, 3 = Curve
    pub amount_in: U256,
}

impl SwapStep {
    pub fn new_v2(token_in: Address, token_out: Address, pool: Address, amount_in: U256) -> Self {
        Self {
            token_in,
            token_out,
            pool,
            fee: 3000,
            dex_type: 0,
            amount_in,
        }
    }

    pub fn new_v3(token_in: Address, token_out: Address, pool: Address, fee: u32, amount_in: U256) -> Self {
        Self {
            token_in,
            token_out,
            pool,
            fee,
            dex_type: 1,
            amount_in,
        }
    }

    pub fn new_v4(token_in: Address, token_out: Address, fee: u32, amount_in: U256) -> Self {
        Self {
            token_in,
            token_out,
            pool: Address::zero(), // V4 使用 PoolManager
            fee,
            dex_type: 2,
            amount_in,
        }
    }
}

/// 套利结果
#[derive(Debug, Clone)]
pub struct ArbitrageResult {
    pub success: bool,
    pub tx_hash: Option<H256>,
    pub profit: U256,
    pub gas_used: U256,
    pub provider: FlashLoanProvider,
}

/// 模拟结果
#[derive(Debug, Clone)]
pub struct SimulationResult {
    pub success: bool,
    pub expected_profit: U256,
    pub gas_estimate: U256,
    pub error_message: Option<String>,
}

/// 闪电贷调用数据
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct FlashLoanCallData {
    pool: Address,
    amount0: U256,
    amount1: U256,
    callback_data: Vec<u8>,
}

/// 套利交易
#[derive(Debug, Clone)]
pub struct ArbitrageTransaction {
    pub to: Address,
    pub value: U256,
    pub data: Bytes,
    pub gas_limit: U256,
}

/// 三角套利构建器
pub struct TriangularArbitrageBuilder {
    flash_provider: FlashLoanProvider,
    start_token: Address,
    path: Vec<(Address, Address, u32, u8)>, // (pool, intermediate_token, fee, dex_type)
}

impl TriangularArbitrageBuilder {
    pub fn new(flash_provider: FlashLoanProvider, start_token: Address) -> Self {
        Self {
            flash_provider,
            start_token,
            path: vec![],
        }
    }

    /// 添加 swap 步骤
    pub fn add_hop(mut self, pool: Address, next_token: Address, fee: u32, dex_type: u8) -> Self {
        self.path.push((pool, next_token, fee, dex_type));
        self
    }

    /// 构建套利请求
    pub fn build(self, borrow_amount: U256) -> ArbitrageRequest {
        let mut swap_path = Vec::new();
        let mut current_token = self.start_token;

        for (pool, next_token, fee, dex_type) in &self.path {
            swap_path.push(SwapStep {
                token_in: current_token,
                token_out: *next_token,
                pool: *pool,
                fee: *fee,
                dex_type: *dex_type,
                amount_in: U256::zero(), // 由合约计算
            });
            current_token = *next_token;
        }

        // 最后一步回到起始代币
        if !swap_path.is_empty() && current_token != self.start_token {
            // 需要额外处理...
        }

        ArbitrageRequest::new(self.flash_provider, self.start_token, borrow_amount)
            .with_swap_path(swap_path)
    }
}

/// 跨 DEX 套利构建器
pub struct CrossDexArbitrageBuilder {
    flash_provider: FlashLoanProvider,
    token: Address,
    buy_dex: DexInfo,
    sell_dex: DexInfo,
}

#[derive(Debug, Clone)]
pub struct DexInfo {
    pub pool: Address,
    pub fee: u32,
    pub dex_type: u8,
}

impl CrossDexArbitrageBuilder {
    pub fn new(
        flash_provider: FlashLoanProvider,
        token: Address,
        buy_dex: DexInfo,
        sell_dex: DexInfo,
    ) -> Self {
        Self {
            flash_provider,
            token,
            buy_dex,
            sell_dex,
        }
    }

    pub fn build(self, quote_token: Address, borrow_amount: U256) -> ArbitrageRequest {
        let swap_path = vec![
            // 在 buy_dex 买入
            SwapStep {
                token_in: quote_token,
                token_out: self.token,
                pool: self.buy_dex.pool,
                fee: self.buy_dex.fee,
                dex_type: self.buy_dex.dex_type,
                amount_in: borrow_amount,
            },
            // 在 sell_dex 卖出
            SwapStep {
                token_in: self.token,
                token_out: quote_token,
                pool: self.sell_dex.pool,
                fee: self.sell_dex.fee,
                dex_type: self.sell_dex.dex_type,
                amount_in: U256::zero(),
            },
        ];

        ArbitrageRequest::new(self.flash_provider, quote_token, borrow_amount)
            .with_swap_path(swap_path)
    }
}
