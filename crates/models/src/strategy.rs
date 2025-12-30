use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// 策略状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrategyStatus {
    Running,
    Stopped,
    Paused,
    Error,
}

impl StrategyStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            StrategyStatus::Running => "running",
            StrategyStatus::Stopped => "stopped",
            StrategyStatus::Paused => "paused",
            StrategyStatus::Error => "error",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "running" => Some(StrategyStatus::Running),
            "stopped" => Some(StrategyStatus::Stopped),
            "paused" => Some(StrategyStatus::Paused),
            "error" => Some(StrategyStatus::Error),
            _ => None,
        }
    }
}

/// 套利策略配置
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ArbitrageStrategyConfig {
    pub id: i64,
    pub name: String,
    pub chain_id: u64,
    pub status: String,
    pub min_profit_threshold_usd: Decimal,
    pub max_slippage: Decimal,
    pub max_gas_price_gwei: Decimal,
    pub max_position_size_usd: Decimal,
    pub use_flash_loan: bool,
    pub flash_loan_provider: String,
    pub target_tokens: serde_json::Value,  // JSON array of token addresses
    pub target_dexes: serde_json::Value,   // JSON array of DEX types
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// 策略统计
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct StrategyStatistics {
    pub strategy_id: i64,
    pub total_trades: i64,
    pub successful_trades: i64,
    pub failed_trades: i64,
    pub total_profit_usd: Decimal,
    pub total_gas_cost_usd: Decimal,
    pub net_profit_usd: Decimal,
    pub win_rate: Decimal,
    pub avg_profit_per_trade: Decimal,
    pub max_profit_trade: Decimal,
    pub max_loss_trade: Decimal,
    pub last_trade_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// 交易记录
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TradeRecord {
    pub id: i64,
    pub strategy_id: i64,
    pub tx_hash: String,
    pub arbitrage_type: String,
    pub path: serde_json::Value,
    pub input_token: String,
    pub input_amount: Decimal,
    pub output_amount: Decimal,
    pub profit_usd: Decimal,
    pub gas_used: Decimal,
    pub gas_price_gwei: Decimal,
    pub gas_cost_usd: Decimal,
    pub net_profit_usd: Decimal,
    pub status: String,
    pub error_message: Option<String>,
    pub block_number: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// 价格监控记录
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PriceMonitorRecord {
    pub id: i64,
    pub token0: String,
    pub token1: String,
    pub dex_type: String,
    pub pool_address: String,
    pub price: Decimal,
    pub liquidity: Decimal,
    pub block_number: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
