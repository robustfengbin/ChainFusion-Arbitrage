//! 回测数据模型

use ethers::types::{Address, H256, U256};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// 从数据库读取的 Swap 记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapRecord {
    pub block_number: u64,
    pub block_timestamp: u64,
    pub pool_address: String,
    pub amount0: String,
    pub amount1: String,
    pub sqrt_price_x96: String,
    pub tick: i32,
    pub liquidity: String,
    pub usd_volume: f64,
}

/// 价格快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceSnapshot {
    pub sqrt_price_x96: String,
    pub tick: i32,
    pub liquidity: String,
    pub block_number: u64,
}

/// 池子配置（从数据库读取）
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct PoolConfig {
    pub id: i64,
    pub chain_id: i64,
    pub address: String,
    pub dex_type: String,
    pub token0: String,
    pub token0_symbol: String,
    pub token1: String,
    pub token1_symbol: String,
    pub fee: i32,
    pub enabled: bool,
}

impl PoolConfig {
    /// 获取地址作为 Address 类型
    pub fn address_h160(&self) -> Address {
        self.address.parse().unwrap_or_default()
    }

    /// 获取费率百分比
    pub fn fee_percent(&self) -> f64 {
        self.fee as f64 / 10000.0
    }

    /// 获取 token0 的小数位数
    pub fn token0_decimals(&self) -> u8 {
        match self.token0_symbol.to_uppercase().as_str() {
            "USDC" | "USDT" => 6,
            "WBTC" => 8,
            _ => 18,
        }
    }

    /// 获取 token1 的小数位数
    pub fn token1_decimals(&self) -> u8 {
        match self.token1_symbol.to_uppercase().as_str() {
            "USDC" | "USDT" => 6,
            "WBTC" => 8,
            _ => 18,
        }
    }
}

/// 三角套利路径配置
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct PoolPathConfig {
    pub id: i64,
    pub chain_id: i64,
    pub trigger_pool: String,
    pub path_name: String,
    pub triangle_name: String,
    pub token_a: String,
    pub token_b: String,
    pub token_c: String,
    pub pool1: String,
    pub pool2: String,
    pub pool3: String,
    pub priority: i32,
    pub enabled: bool,
}

/// Swap 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapEventData {
    pub block_number: u64,
    pub block_timestamp: u64,
    pub tx_hash: H256,
    pub log_index: u32,
    pub pool_address: Address,
    /// amount0 (正数表示池子收到, 负数表示池子支出)
    pub amount0: i128,
    /// amount1 (正数表示池子收到, 负数表示池子支出)
    pub amount1: i128,
    /// sqrtPriceX96
    pub sqrt_price_x96: U256,
    /// 当前 tick
    pub tick: i32,
    /// 当前流动性
    pub liquidity: u128,
}

impl SwapEventData {
    /// 计算交易的 USD 金额（简化版本，基于 stablecoin）
    pub fn usd_volume(&self, pool: &PoolConfig) -> f64 {
        let amount0_abs = self.amount0.abs() as f64 / 10f64.powi(pool.token0_decimals() as i32);
        let amount1_abs = self.amount1.abs() as f64 / 10f64.powi(pool.token1_decimals() as i32);

        // 如果其中一个是稳定币，使用稳定币金额作为 USD 交易量
        let token0_is_stable = matches!(
            pool.token0_symbol.to_uppercase().as_str(),
            "USDC" | "USDT" | "DAI"
        );
        let token1_is_stable = matches!(
            pool.token1_symbol.to_uppercase().as_str(),
            "USDC" | "USDT" | "DAI"
        );

        if token0_is_stable {
            amount0_abs
        } else if token1_is_stable {
            amount1_abs
        } else {
            // 非稳定币池子，暂时返回 0
            0.0
        }
    }
}

/// 区块 Swap 数据汇总
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockSwapSummary {
    pub block_number: u64,
    pub block_timestamp: u64,
    pub base_fee_gwei: f64,
    /// 各池子的 Swap 事件
    pub swaps_by_pool: std::collections::HashMap<String, Vec<SwapEventData>>,
    /// 各池子的 sqrtPriceX96（区块结束时的价格）
    pub prices_by_pool: std::collections::HashMap<String, U256>,
    /// 总 USD 交易量
    pub total_usd_volume: f64,
}

/// 触发事件信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerEventInfo {
    /// 触发池子地址
    pub pool_address: String,
    /// 触发池子的交易对名称 (如 "USDC/WETH")
    pub pool_name: String,
    /// 池子费率 (如 0.05%)
    pub pool_fee_percent: f64,
    /// 该池子在此区块的交易量 USD
    pub pool_volume_usd: f64,
    /// 交易方向 (如 "USDC -> WETH" 表示用户用 USDC 买 WETH)
    pub swap_direction: String,
    /// 用户卖出的代币
    pub user_sell_token: String,
    /// 用户买入的代币
    pub user_buy_token: String,
    /// 价格影响说明
    pub price_impact: String,
}

/// 套利步骤详情
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageStep {
    /// 步骤编号 (1, 2, 3)
    pub step: u8,
    /// 池子地址
    pub pool_address: String,
    /// 池子名称 (如 "USDC/WETH")
    pub pool_name: String,
    /// 池子费率 (如 0.05%)
    pub fee_percent: f64,
    /// 卖出代币
    pub sell_token: String,
    /// 卖出数量
    pub sell_amount: f64,
    /// 买入代币
    pub buy_token: String,
    /// 买入数量
    pub buy_amount: f64,
    /// 操作说明
    pub description: String,
}

/// 套利机会分析结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageOpportunity {
    pub block_number: u64,
    pub block_timestamp: u64,
    /// 上海时间字符串
    pub datetime_shanghai: String,
    pub path_name: String,
    pub triangle_name: String,
    /// 区块内真实 USD 交易量
    pub real_volume_usd: f64,
    /// 捕获比例 (10%, 25%, 50%, 100%)
    pub capture_percent: u32,
    /// 实际输入金额
    pub input_amount_usd: f64,
    /// 输出金额
    pub output_amount_usd: f64,
    /// 毛利润
    pub gross_profit_usd: f64,
    /// Gas 成本
    pub gas_cost_usd: f64,
    /// 净利润
    pub net_profit_usd: f64,
    /// 是否盈利
    pub is_profitable: bool,
    /// 触发事件信息
    pub trigger_event: Option<TriggerEventInfo>,
    /// 套利步骤详情
    pub arb_steps: Vec<ArbitrageStep>,
    /// 价格偏离率 (毛利润/输入金额 * 100%)
    pub price_deviation_percent: f64,
    /// 总手续费率
    pub total_fee_percent: f64,
    /// 理论套利空间 (价格偏离 - 手续费)
    pub arb_spread_percent: f64,
    /// 闪电贷费用 (假设从最低费率池借)
    pub flash_loan_fee_usd: f64,
    /// 闪电贷费率
    pub flash_loan_fee_percent: f64,
    /// 扣除闪电贷后的真实净利润
    pub real_net_profit_usd: f64,
}

/// 回测统计结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestStatistics {
    /// 回测时间范围
    pub start_block: u64,
    pub end_block: u64,
    pub start_timestamp: u64,
    pub end_timestamp: u64,
    /// 分析的区块数
    pub total_blocks: u64,
    /// 有交易的区块数
    pub blocks_with_swaps: u64,
    /// 总交易量
    pub total_volume_usd: f64,
    /// 各路径的统计
    pub path_stats: Vec<PathStatistics>,
    /// 盈利机会列表
    pub profitable_opportunities: Vec<ArbitrageOpportunity>,
}

/// 路径统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathStatistics {
    pub path_name: String,
    pub triangle_name: String,
    /// 分析次数
    pub analysis_count: u64,
    /// 盈利次数
    pub profitable_count: u64,
    /// 最大净利润
    pub max_profit_usd: f64,
    /// 平均净利润
    pub avg_profit_usd: f64,
    /// 总净利润
    pub total_profit_usd: f64,
}

/// 保存到数据库的回测记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestRecord {
    pub id: Option<i64>,
    pub chain_id: i64,
    pub start_block: i64,
    pub end_block: i64,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: chrono::DateTime<chrono::Utc>,
    pub total_blocks: i64,
    pub blocks_with_swaps: i64,
    pub total_volume_usd: Decimal,
    pub total_opportunities: i64,
    pub profitable_opportunities: i64,
    pub max_profit_usd: Decimal,
    pub total_profit_usd: Decimal,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}
