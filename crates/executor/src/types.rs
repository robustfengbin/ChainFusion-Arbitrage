//! 执行器类型定义

use ethers::types::{Address, H256, U256};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 套利执行参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageParams {
    /// 闪电贷池地址
    pub flash_pool: Address,
    /// 闪电贷池费率 (100=0.01%, 500=0.05%, 3000=0.3%, 10000=1%)
    /// 用于精确计算闪电贷成本
    #[serde(default = "default_flash_pool_fee")]
    pub flash_pool_fee: u32,
    /// 起始代币 (借入并归还)
    pub token_a: Address,
    /// 中间代币 1
    pub token_b: Address,
    /// 中间代币 2
    pub token_c: Address,
    /// A -> B 池子费率 (如 3000 = 0.3%)
    pub fee1: u32,
    /// B -> C 池子费率
    pub fee2: u32,
    /// C -> A 池子费率
    pub fee3: u32,
    /// 输入金额
    pub amount_in: U256,
    /// 最小利润要求 (wei)
    pub min_profit: U256,
    /// 预估利润 (USD)
    pub estimated_profit_usd: Decimal,
    /// 预估 gas 成本 (USD)
    pub estimated_gas_cost_usd: Decimal,
    /// 预估闪电贷费用 (wei)
    #[serde(default)]
    pub estimated_flash_fee: U256,
    /// 利润结算代币 (None 或 Address::zero() 表示不转换，保留原始代币)
    pub profit_token: Option<Address>,
    /// 利润转换池费率 (tokenA -> profitToken)
    pub profit_convert_fee: u32,
    /// swap 路径中的池子地址 (用于验证闪电贷池不重复)
    #[serde(default)]
    pub swap_pools: Vec<Address>,
}

fn default_flash_pool_fee() -> u32 {
    500 // 默认 0.05% 费率
}

/// 执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// 交易哈希
    pub tx_hash: H256,
    /// 实际利润 (wei)
    pub profit: U256,
    /// 利润 (USD)
    pub profit_usd: Decimal,
    /// 实际 gas 使用量
    pub gas_used: U256,
    /// 实际 gas 成本 (USD)
    pub gas_cost_usd: Decimal,
    /// 净利润 (USD)
    pub net_profit_usd: Decimal,
    /// 交易状态
    pub success: bool,
    /// 区块号
    pub block_number: u64,
}

/// 执行错误类型
#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("Contract call failed: {0}")]
    ContractError(String),

    #[error("Transaction reverted: {0}")]
    TransactionReverted(String),

    #[error("Insufficient profit: expected {expected}, got {actual}")]
    InsufficientProfit { expected: U256, actual: U256 },

    #[error("Gas estimation failed: {0}")]
    GasEstimationFailed(String),

    #[error("Nonce error: {0}")]
    NonceError(String),

    #[error("Provider error: {0}")]
    ProviderError(String),

    #[error("Wallet error: {0}")]
    WalletError(String),

    #[error("Timeout waiting for transaction")]
    Timeout,

    #[error("Flashbots error: {0}")]
    FlashbotsError(String),

    #[error("Flashbots bundle not included: {0}")]
    FlashbotsNotIncluded(String),

    #[error("Flashbots simulation failed: {0}")]
    FlashbotsSimulationFailed(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

/// 交易状态
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionStatus {
    /// 待发送
    Pending,
    /// 已发送，等待确认
    Submitted { tx_hash: H256 },
    /// 已确认
    Confirmed { tx_hash: H256, block_number: u64 },
    /// 失败
    Failed { reason: String },
}

/// Gas 策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasStrategy {
    /// 基础 gas price 倍数 (1.0 = 当前 gas price)
    pub gas_price_multiplier: f64,
    /// 最大 gas price (Gwei) - 支持小数，如 0.05 Gwei
    pub max_gas_price_gwei: f64,
    /// gas limit 倍数
    pub gas_limit_multiplier: f64,
    /// 使用 EIP-1559
    pub use_eip1559: bool,
    /// 优先费 (Gwei) - 支持小数，如 0.001 Gwei
    pub priority_fee_gwei: f64,
    /// 固定 gas limit (如果设置，跳过 gas 估算，强制使用此值)
    pub fixed_gas_limit: Option<u64>,
}

impl Default for GasStrategy {
    fn default() -> Self {
        Self {
            gas_price_multiplier: 1.1, // 加 10% 确保快速确认
            max_gas_price_gwei: 100.0,
            gas_limit_multiplier: 1.2,
            use_eip1559: true,
            priority_fee_gwei: 0.01,   // 当前低 Gas 环境
            fixed_gas_limit: None,     // 默认动态估算
        }
    }
}
