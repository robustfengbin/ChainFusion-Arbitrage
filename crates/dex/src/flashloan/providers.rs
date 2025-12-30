use anyhow::Result;
use ethers::prelude::*;
use ethers::types::{Address, U256};
use std::sync::Arc;
use tracing::info;

use crate::uniswap::v4_addresses;

/// 闪电贷提供商类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashLoanProvider {
    /// Uniswap V3 Flash
    UniswapV3,
    /// Uniswap V4 Flash Accounting (零成本)
    UniswapV4,
    /// Aave V3 Flash Loan
    AaveV3,
    /// Balancer Flash Loan
    Balancer,
}

impl FlashLoanProvider {
    /// 获取提供商名称
    pub fn name(&self) -> &'static str {
        match self {
            FlashLoanProvider::UniswapV3 => "Uniswap V3",
            FlashLoanProvider::UniswapV4 => "Uniswap V4",
            FlashLoanProvider::AaveV3 => "Aave V3",
            FlashLoanProvider::Balancer => "Balancer",
        }
    }

    /// 获取闪电贷默认费率 (以 1e6 为基数)
    ///
    /// 费率单位: 100 = 0.01%, 500 = 0.05%, 3000 = 0.3%, 10000 = 1%
    ///
    /// **重要**: 对于 Uniswap V3，闪电贷费率等于借贷池的 fee tier，
    /// 应使用 `ArbitrageRequest::with_flash_pool_fee()` 设置实际池子费率，
    /// 或使用 `fee_rate_with_pool_fee()` 方法传入具体费率。
    /// 这里返回的 3000 (0.3%) 仅作为默认估算值。
    pub fn fee_rate(&self) -> u32 {
        match self {
            // V3: 费率 = 池子 fee tier，需从路径动态获取
            // 可选费率: 100 (0.01%), 500 (0.05%), 3000 (0.3%), 10000 (1%)
            FlashLoanProvider::UniswapV3 => 3000,   // 默认估算值，实际应从池子获取
            FlashLoanProvider::UniswapV4 => 0,      // V4 flash accounting 无费用
            FlashLoanProvider::AaveV3 => 500,       // 固定 0.05%
            FlashLoanProvider::Balancer => 0,       // Balancer flash 无费用
        }
    }

    /// 获取指定池子费率的闪电贷费用 (用于精确计算)
    ///
    /// # 参数
    /// - `pool_fee`: 池子费率，从套利路径中获取
    ///   - 100 = 0.01%
    ///   - 500 = 0.05%
    ///   - 3000 = 0.3%
    ///   - 10000 = 1%
    pub fn fee_rate_with_pool_fee(&self, pool_fee: u32) -> u32 {
        match self {
            FlashLoanProvider::UniswapV3 => pool_fee, // V3 费率等于池子费率
            _ => self.fee_rate(),                     // 其他提供商使用固定费率
        }
    }

    /// 是否支持多资产闪电贷
    pub fn supports_multi_asset(&self) -> bool {
        match self {
            FlashLoanProvider::UniswapV3 => true,  // 可以同时借两种代币
            FlashLoanProvider::UniswapV4 => true,  // Flash accounting 支持任意数量
            FlashLoanProvider::AaveV3 => true,     // 支持批量闪电贷
            FlashLoanProvider::Balancer => true,   // 支持批量
        }
    }
}

// Uniswap V3 闪电贷 ABI
abigen!(
    UniswapV3FlashPool,
    r#"[
        function flash(address recipient, uint256 amount0, uint256 amount1, bytes calldata data) external
    ]"#
);

// Aave V3 Pool ABI (闪电贷相关)
abigen!(
    AaveV3Pool,
    r#"[
        function flashLoan(address receiverAddress, address[] calldata assets, uint256[] calldata amounts, uint256[] calldata interestRateModes, address onBehalfOf, bytes calldata params, uint16 referralCode) external
        function flashLoanSimple(address receiverAddress, address asset, uint256 amount, bytes calldata params, uint16 referralCode) external
        function FLASHLOAN_PREMIUM_TOTAL() external view returns (uint128)
    ]"#
);

// Balancer Vault ABI (闪电贷相关)
abigen!(
    BalancerVault,
    r#"[
        function flashLoan(address recipient, address[] calldata tokens, uint256[] calldata amounts, bytes calldata userData) external
    ]"#
);

/// 闪电贷请求
#[derive(Debug, Clone)]
pub struct FlashLoanRequest {
    pub provider: FlashLoanProvider,
    pub tokens: Vec<Address>,
    pub amounts: Vec<U256>,
    pub callback_data: Vec<u8>,
}

impl FlashLoanRequest {
    /// 创建单资产闪电贷请求
    pub fn single(provider: FlashLoanProvider, token: Address, amount: U256) -> Self {
        Self {
            provider,
            tokens: vec![token],
            amounts: vec![amount],
            callback_data: vec![],
        }
    }

    /// 创建双资产闪电贷请求 (用于 V3 池子)
    pub fn dual(
        provider: FlashLoanProvider,
        token0: Address,
        amount0: U256,
        token1: Address,
        amount1: U256,
    ) -> Self {
        Self {
            provider,
            tokens: vec![token0, token1],
            amounts: vec![amount0, amount1],
            callback_data: vec![],
        }
    }

    /// 设置回调数据
    pub fn with_callback_data(mut self, data: Vec<u8>) -> Self {
        self.callback_data = data;
        self
    }

    /// 计算需要归还的总金额（本金 + 费用）
    /// 使用默认费率估算
    pub fn repay_amounts(&self) -> Vec<U256> {
        let fee_rate = self.provider.fee_rate() as u128;
        self.amounts
            .iter()
            .map(|amount| {
                let fee = (*amount * U256::from(fee_rate)) / U256::from(1_000_000u128);
                *amount + fee
            })
            .collect()
    }

    /// 计算需要归还的总金额（本金 + 费用）
    /// 使用指定的池子费率进行精确计算
    /// pool_fee: 池子费率 (100=0.01%, 500=0.05%, 3000=0.3%, 10000=1%)
    pub fn repay_amounts_with_pool_fee(&self, pool_fee: u32) -> Vec<U256> {
        let fee_rate = self.provider.fee_rate_with_pool_fee(pool_fee) as u128;
        self.amounts
            .iter()
            .map(|amount| {
                let fee = (*amount * U256::from(fee_rate)) / U256::from(1_000_000u128);
                *amount + fee
            })
            .collect()
    }

    /// 计算闪电贷费用
    pub fn calculate_fees(&self) -> Vec<U256> {
        let fee_rate = self.provider.fee_rate() as u128;
        self.amounts
            .iter()
            .map(|amount| {
                (*amount * U256::from(fee_rate)) / U256::from(1_000_000u128)
            })
            .collect()
    }

    /// 使用指定池子费率计算闪电贷费用
    pub fn calculate_fees_with_pool_fee(&self, pool_fee: u32) -> Vec<U256> {
        let fee_rate = self.provider.fee_rate_with_pool_fee(pool_fee) as u128;
        self.amounts
            .iter()
            .map(|amount| {
                (*amount * U256::from(fee_rate)) / U256::from(1_000_000u128)
            })
            .collect()
    }
}

/// Uniswap V3 闪电贷提供者
pub struct UniswapV3FlashProvider<M: Middleware> {
    provider: Arc<M>,
}

impl<M: Middleware + 'static> UniswapV3FlashProvider<M> {
    pub fn new(provider: Arc<M>) -> Self {
        Self { provider }
    }

    /// 执行 V3 闪电贷
    pub async fn flash(
        &self,
        pool_address: Address,
        recipient: Address,
        amount0: U256,
        amount1: U256,
        data: Vec<u8>,
    ) -> Result<()> {
        let pool = UniswapV3FlashPool::new(pool_address, self.provider.clone());

        info!(
            "执行 Uniswap V3 闪电贷: pool={:?}, amount0={}, amount1={}",
            pool_address, amount0, amount1
        );

        // 构建交易但不发送（需要签名者）
        let _tx = pool.flash(recipient, amount0, amount1, data.into());

        Ok(())
    }

    /// 编码闪电贷回调数据
    pub fn encode_callback_data(
        swap_path: &[(Address, Address, u32)], // (token_in, token_out, fee)
        min_profit: U256,
    ) -> Vec<u8> {
        use ethers::abi::{encode, Token};

        let path_tokens: Vec<Token> = swap_path
            .iter()
            .map(|(token_in, token_out, fee)| {
                Token::Tuple(vec![
                    Token::Address(*token_in),
                    Token::Address(*token_out),
                    Token::Uint((*fee).into()),
                ])
            })
            .collect();

        encode(&[Token::Array(path_tokens), Token::Uint(min_profit)])
    }
}

/// Uniswap V4 Flash Accounting 提供者
/// V4 使用 "delta" 系统，允许先交易后结算
#[allow(dead_code)]
pub struct UniswapV4FlashProvider<M: Middleware> {
    provider: Arc<M>,
    pool_manager: Address,
}

impl<M: Middleware + 'static> UniswapV4FlashProvider<M> {
    pub fn new(provider: Arc<M>) -> Self {
        Self {
            provider,
            pool_manager: *v4_addresses::POOL_MANAGER,
        }
    }

    /// 编码 unlock callback 数据
    /// V4 的闪电贷通过 unlock -> 执行操作 -> settle 模式实现
    pub fn encode_unlock_data(
        swaps: &[SwapOperation],
        settle_currencies: &[Address],
        take_currencies: &[(Address, U256)],
    ) -> Vec<u8> {
        use ethers::abi::{encode, Token};

        // 编码 swap 操作
        let swap_tokens: Vec<Token> = swaps
            .iter()
            .map(|op| {
                Token::Tuple(vec![
                    Token::Address(op.currency0),
                    Token::Address(op.currency1),
                    Token::Uint(op.fee.into()),
                    Token::Int(op.tick_spacing.into()),
                    Token::Address(op.hooks),
                    Token::Bool(op.zero_for_one),
                    Token::Int(op.amount_specified.into_raw()),
                ])
            })
            .collect();

        // 编码结算货币
        let settle_tokens: Vec<Token> = settle_currencies
            .iter()
            .map(|addr| Token::Address(*addr))
            .collect();

        // 编码提取操作
        let take_tokens: Vec<Token> = take_currencies
            .iter()
            .map(|(addr, amount)| {
                Token::Tuple(vec![Token::Address(*addr), Token::Uint(*amount)])
            })
            .collect();

        encode(&[
            Token::Array(swap_tokens),
            Token::Array(settle_tokens),
            Token::Array(take_tokens),
        ])
    }
}

/// V4 Swap 操作
#[derive(Debug, Clone)]
pub struct SwapOperation {
    pub currency0: Address,
    pub currency1: Address,
    pub fee: u32,
    pub tick_spacing: i32,
    pub hooks: Address,
    pub zero_for_one: bool,
    pub amount_specified: I256,
}

impl SwapOperation {
    pub fn new(
        currency0: Address,
        currency1: Address,
        fee: u32,
        tick_spacing: i32,
        zero_for_one: bool,
        amount_in: U256,
    ) -> Self {
        let (c0, c1) = if currency0 < currency1 {
            (currency0, currency1)
        } else {
            (currency1, currency0)
        };

        // 负数表示 exact input
        let amount_specified = I256::from_raw(amount_in).checked_neg().unwrap_or(I256::zero());

        Self {
            currency0: c0,
            currency1: c1,
            fee,
            tick_spacing,
            hooks: Address::zero(),
            zero_for_one,
            amount_specified,
        }
    }
}

/// Aave V3 闪电贷提供者
pub struct AaveV3FlashProvider<M: Middleware> {
    provider: Arc<M>,
    pool_address: Address,
}

impl<M: Middleware + 'static> AaveV3FlashProvider<M> {
    pub fn new(provider: Arc<M>, pool_address: Address) -> Self {
        Self {
            provider,
            pool_address,
        }
    }

    /// 获取 Aave 闪电贷费率
    pub async fn get_premium(&self) -> Result<u128> {
        let pool = AaveV3Pool::new(self.pool_address, self.provider.clone());
        let premium = pool.flashloan_premium_total().call().await?;
        Ok(premium)
    }

    /// 编码闪电贷回调数据
    pub fn encode_callback_data(
        operations: &[FlashLoanOperation],
    ) -> Vec<u8> {
        use ethers::abi::{encode, Token};

        let op_tokens: Vec<Token> = operations
            .iter()
            .map(|op| {
                Token::Tuple(vec![
                    Token::Uint(op.action_type.into()),
                    Token::Address(op.target),
                    Token::Bytes(op.data.clone()),
                ])
            })
            .collect();

        encode(&[Token::Array(op_tokens)])
    }
}

/// 闪电贷操作
#[derive(Debug, Clone)]
pub struct FlashLoanOperation {
    pub action_type: u8,  // 0 = swap, 1 = 添加流动性, 2 = 移除流动性
    pub target: Address,
    pub data: Vec<u8>,
}

/// Balancer 闪电贷提供者
#[allow(dead_code)]
pub struct BalancerFlashProvider<M: Middleware> {
    provider: Arc<M>,
    vault_address: Address,
}

impl<M: Middleware + 'static> BalancerFlashProvider<M> {
    /// Balancer Vault 地址 (Ethereum Mainnet)
    pub const VAULT_ADDRESS: &'static str = "0xBA12222222228d8Ba445958a75a0704d566BF2C8";

    pub fn new(provider: Arc<M>) -> Self {
        Self {
            provider,
            vault_address: Address::from_str(Self::VAULT_ADDRESS).unwrap(),
        }
    }

    pub fn with_vault(provider: Arc<M>, vault_address: Address) -> Self {
        Self {
            provider,
            vault_address,
        }
    }
}

use std::str::FromStr;
