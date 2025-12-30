use ethers::types::{Address, U256, H256};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use crate::DexType;

/// 套利路径中的单跳
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapHop {
    pub pool_address: Address,
    pub dex_type: DexType,
    pub token_in: Address,
    pub token_out: Address,
    pub fee: u32,
}

/// 套利路径
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitragePath {
    pub hops: Vec<SwapHop>,
    pub start_token: Address,
    pub chain_id: u64,
}

impl ArbitragePath {
    pub fn new(start_token: Address, chain_id: u64) -> Self {
        Self {
            hops: Vec::new(),
            start_token,
            chain_id,
        }
    }

    pub fn add_hop(&mut self, hop: SwapHop) {
        self.hops.push(hop);
    }

    /// 检查路径是否形成闭环
    pub fn is_closed_loop(&self) -> bool {
        if self.hops.is_empty() {
            return false;
        }

        let last_hop = self.hops.last().unwrap();
        last_hop.token_out == self.start_token
    }

    /// 获取路径长度
    pub fn len(&self) -> usize {
        self.hops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hops.is_empty()
    }
}

/// 套利机会
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageOpportunity {
    pub id: String,
    pub path: ArbitragePath,
    pub input_amount: U256,
    pub expected_output: U256,
    pub expected_profit: U256,
    pub expected_profit_usd: Decimal,
    pub gas_estimate: U256,
    pub gas_cost_usd: Decimal,
    pub net_profit_usd: Decimal,
    pub profit_percentage: Decimal,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub block_number: u64,
}

impl ArbitrageOpportunity {
    /// 检查套利是否有利可图
    pub fn is_profitable(&self, min_profit_threshold: Decimal) -> bool {
        self.net_profit_usd > min_profit_threshold
    }
}

/// 套利执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageResult {
    pub opportunity: ArbitrageOpportunity,
    pub tx_hash: Option<H256>,
    pub status: ArbitrageStatus,
    pub actual_profit: Option<U256>,
    pub actual_gas_used: Option<U256>,
    pub error_message: Option<String>,
    pub executed_at: chrono::DateTime<chrono::Utc>,
}

/// 套利状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArbitrageStatus {
    Pending,
    Submitted,
    Confirmed,
    Failed,
    Reverted,
}

impl ArbitrageStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ArbitrageStatus::Pending => "pending",
            ArbitrageStatus::Submitted => "submitted",
            ArbitrageStatus::Confirmed => "confirmed",
            ArbitrageStatus::Failed => "failed",
            ArbitrageStatus::Reverted => "reverted",
        }
    }
}

/// 套利类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArbitrageType {
    /// DEX 内三角套利
    TriangularIntraDex,
    /// 跨 DEX 套利
    CrossDex,
    /// Flash Accounting 套利 (Uniswap V4)
    FlashAccounting,
}

impl ArbitrageType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ArbitrageType::TriangularIntraDex => "triangular_intra_dex",
            ArbitrageType::CrossDex => "cross_dex",
            ArbitrageType::FlashAccounting => "flash_accounting",
        }
    }
}

/// 价格偏差信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceDeviation {
    pub token_pair: (Address, Address),
    pub dex_a: DexType,
    pub dex_b: DexType,
    pub price_a: Decimal,
    pub price_b: Decimal,
    pub deviation_percentage: Decimal,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl PriceDeviation {
    /// 计算价格偏差百分比
    pub fn calculate_deviation(price_a: Decimal, price_b: Decimal) -> Decimal {
        if price_a.is_zero() || price_b.is_zero() {
            return Decimal::ZERO;
        }

        let diff = (price_a - price_b).abs();
        let avg = (price_a + price_b) / Decimal::from(2);

        (diff / avg) * Decimal::from(100)
    }
}
