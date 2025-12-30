//! 事件驱动的套利扫描器
//!
//! 通过监听 Swap 事件来触发套利检测，而不是高频轮询
//! 使用链上 Quoter 合约获取真实报价，接入价格服务获取实时价格
//! 支持检测到利润后自动调用执行器执行套利

use anyhow::Result;
use ethers::prelude::*;
use ethers::signers::LocalWallet;
use ethers::types::{Address, U256};
use models::{ArbitrageOpportunity, ArbitragePath, DexType, SwapHop};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromStr;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{broadcast, RwLock, Semaphore};
use tracing::{info, debug, warn, error};

use services::{SwapEvent, NewBlockEvent, SharedPriceService, get_email_notifier, ArbitrageExecutionInfo, WalletBalance};
use ::utils::utc_to_shanghai_str;

// 使用新的执行器和闪电贷池选择器
use executor::{
    ArbitrageExecutor as RealExecutor, ExecutorConfig, GasStrategy, SendMode,
    ArbitrageParamsBuilder, FlashbotsConfig, RevertDecoder,
};

// ERC20 ABI for balance queries
abigen!(
    IERC20Balance,
    r#"[function balanceOf(address account) external view returns (uint256)]"#
);

// Uniswap V3 QuoterV2 ABI (返回 gas 估算)
abigen!(
    UniswapV3QuoterV2,
    r#"[
        {
            "inputs": [
                {
                    "components": [
                        {"name": "tokenIn", "type": "address"},
                        {"name": "tokenOut", "type": "address"},
                        {"name": "amountIn", "type": "uint256"},
                        {"name": "fee", "type": "uint24"},
                        {"name": "sqrtPriceLimitX96", "type": "uint160"}
                    ],
                    "name": "params",
                    "type": "tuple"
                }
            ],
            "name": "quoteExactInputSingle",
            "outputs": [
                {"name": "amountOut", "type": "uint256"},
                {"name": "sqrtPriceX96After", "type": "uint160"},
                {"name": "initializedTicksCrossed", "type": "uint32"},
                {"name": "gasEstimate", "type": "uint256"}
            ],
            "stateMutability": "nonpayable",
            "type": "function"
        }
    ]"#
);

// Uniswap V3 Pool ABI (用于查询 slot0)
abigen!(
    IUniswapV3Pool,
    r#"[
        function slot0() external view returns (uint160 sqrtPriceX96, int24 tick, uint16 observationIndex, uint16 observationCardinality, uint16 observationCardinalityNext, uint8 feeProtocol, bool unlocked)
        function liquidity() external view returns (uint128)
    ]"#
);

// Multicall3 ABI
abigen!(
    Multicall3,
    r#"[
        struct Call3 { address target; bool allowFailure; bytes callData; }
        struct Result { bool success; bytes returnData; }
        function aggregate3(Call3[] calldata calls) external payable returns (Result[] memory returnData)
    ]"#
);

/// Multicall3 合约地址 (在大多数链上都是这个地址)
const MULTICALL3_ADDRESS: &str = "0xcA11bde05977b3631167028862bE2a173976CA11";

/// 缓存过期阈值 (允许的最大过期区块数)
/// 注意：现在每个新区块都会刷新所有池子，此常量仅作为备用参考
#[allow(dead_code)]
const MAX_STALE_BLOCKS: u64 = 2;

/// 单次报价结果
#[derive(Debug, Clone)]
pub struct QuoteResult {
    pub amount_out: U256,
    pub gas_estimate: U256,
}

/// 套利模拟结果
#[derive(Debug, Clone)]
pub struct ArbitrageSimResult {
    pub net_profit_usd: Decimal,
    pub amount_out: U256,
    pub total_gas_used: U256,
    pub gas_cost_usd: Decimal,
}

/// 池子本地状态缓存
#[derive(Debug, Clone)]
pub struct PoolState {
    pub address: Address,
    pub token0: Address,
    pub token1: Address,
    pub dex_type: DexType,
    pub fee: u32,
    /// reserve0 或 liquidity (V2 用)
    pub reserve0: U256,
    /// reserve1 (V2 用)
    pub reserve1: U256,
    /// V3 价格状态: sqrtPriceX96
    pub sqrt_price_x96: Option<U256>,
    /// V3 流动性
    pub liquidity: Option<u128>,
    /// V3 tick
    pub tick: Option<i32>,
    /// 最后更新的区块
    pub last_block: u64,
    /// 最后更新时间
    pub last_updated: std::time::Instant,
}

impl PoolState {
    /// 从 Swap 事件更新价格状态
    pub fn update_from_swap(&mut self, event: &SwapEvent) {
        self.last_block = event.block_number;
        self.last_updated = std::time::Instant::now();

        // 更新 V3 价格状态 (如果事件中包含)
        if let Some(sqrt_price) = event.sqrt_price_x96 {
            self.sqrt_price_x96 = Some(sqrt_price);
        }
        if let Some(liq) = event.liquidity {
            self.liquidity = Some(liq);
        }
        if let Some(t) = event.tick {
            self.tick = Some(t);
        }
    }

    /// 检查缓存是否足够新鲜
    /// max_stale_blocks: 允许的最大过期区块数
    pub fn is_fresh(&self, current_block: u64, max_stale_blocks: u64) -> bool {
        current_block.saturating_sub(self.last_block) <= max_stale_blocks
    }

    /// 检查是否有有效的 V3 价格数据
    pub fn has_v3_price_data(&self) -> bool {
        self.sqrt_price_x96.is_some() && self.liquidity.is_some()
    }
}

/// 动态利润门槛配置 - 根据 Gas 价格自动调整最小利润要求
#[derive(Debug, Clone)]
pub struct DynamicProfitConfig {
    /// Gas < 1 Gwei 时的最小利润 (USD) - 超低 gas 场景
    pub ultra_low_gas_min_profit: Decimal,
    /// Gas < 5 Gwei 时的最小利润 (USD) - 低 gas 场景
    pub low_gas_min_profit: Decimal,
    /// Gas < 20 Gwei 时的最小利润 (USD) - 正常 gas 场景
    pub normal_gas_min_profit: Decimal,
    /// Gas < 50 Gwei 时的最小利润 (USD) - 高 gas 场景
    pub high_gas_min_profit: Decimal,
    /// Gas >= 50 Gwei 时的最小利润 (USD) - 超高 gas 场景
    pub very_high_gas_min_profit: Decimal,
}

impl Default for DynamicProfitConfig {
    fn default() -> Self {
        Self {
            // 根据 Gas 价格设置最小净利润要求，增加安全边际
            ultra_low_gas_min_profit: dec!(1),    // Gas < 1 Gwei: $1
            low_gas_min_profit: dec!(3),          // Gas 1-5 Gwei: $3
            normal_gas_min_profit: dec!(5),       // Gas 5-20 Gwei: $5
            high_gas_min_profit: dec!(15),        // Gas 20-50 Gwei: $15
            very_high_gas_min_profit: dec!(30),   // Gas >= 50 Gwei: $30
        }
    }
}

/// 执行数量策略 - 决定使用多少资金进行套利
#[derive(Debug, Clone)]
pub enum ExecutionAmountStrategy {
    /// 使用检测到的最优输入金额的百分比 (例如 0.8 = 80%)
    Percentage(f64),
    /// 使用完整金额 (100%)
    FullAmount,
    /// 最大 USD 金额限制 - 如果最优输入超过此值，则使用此值
    MaxUsd(Decimal),
    /// 组合策略: 先应用百分比，再限制最大 USD 金额
    PercentageWithMaxUsd { percentage: f64, max_usd: Decimal },
}

impl Default for ExecutionAmountStrategy {
    fn default() -> Self {
        // 默认使用 80% 的最优输入金额，降低风险
        ExecutionAmountStrategy::Percentage(0.8)
    }
}

impl ExecutionAmountStrategy {
    /// 根据策略计算实际执行金额
    pub fn calculate_amount(
        &self,
        optimal_input: U256,
        token_decimals: u8,
        token_price_usd: Decimal,
    ) -> U256 {
        match self {
            ExecutionAmountStrategy::FullAmount => optimal_input,
            ExecutionAmountStrategy::Percentage(pct) => {
                // 应用百分比
                let pct_u256 = U256::from((*pct * 1000.0) as u64);
                optimal_input * pct_u256 / U256::from(1000u64)
            }
            ExecutionAmountStrategy::MaxUsd(max_usd) => {
                // 计算 optimal_input 的 USD 价值
                let divisor = Decimal::from(10u64.pow(token_decimals as u32));
                let input_dec = decimal_from_str(&optimal_input.to_string()).unwrap_or(Decimal::ZERO);
                let input_usd = (input_dec / divisor) * token_price_usd;

                if input_usd <= *max_usd {
                    optimal_input
                } else {
                    // 限制为 max_usd 对应的代币数量
                    let max_tokens = (*max_usd / token_price_usd) * divisor;
                    let max_str = max_tokens.floor().to_string();
                    U256::from_dec_str(&max_str).unwrap_or(optimal_input)
                }
            }
            ExecutionAmountStrategy::PercentageWithMaxUsd { percentage, max_usd } => {
                // 先应用百分比
                let pct_u256 = U256::from((*percentage * 1000.0) as u64);
                let after_pct = optimal_input * pct_u256 / U256::from(1000u64);

                // 再检查是否超过 max_usd
                let divisor = Decimal::from(10u64.pow(token_decimals as u32));
                let input_dec = decimal_from_str(&after_pct.to_string()).unwrap_or(Decimal::ZERO);
                let input_usd = (input_dec / divisor) * token_price_usd;

                if input_usd <= *max_usd {
                    after_pct
                } else {
                    let max_tokens = (*max_usd / token_price_usd) * divisor;
                    let max_str = max_tokens.floor().to_string();
                    U256::from_dec_str(&max_str).unwrap_or(after_pct)
                }
            }
        }
    }
}

/// 执行器配置 (用于事件驱动扫描器)
#[derive(Debug, Clone)]
pub struct ScannerExecutorConfig {
    /// 是否启用自动执行
    pub auto_execute: bool,
    /// 套利合约地址
    pub arbitrage_contract: Option<Address>,
    /// 最大 Gas 价格 (Gwei) - 支持小数，如 0.08
    pub max_gas_price_gwei: f64,
    /// 是否使用 Flashbots
    pub use_flashbots: bool,
    /// Flashbots RPC URL
    pub flashbots_rpc_url: Option<String>,
    /// 是否同时使用公开 mempool（Both 模式）
    pub use_public_mempool: bool,
    /// 是否为干运行模式 (不实际执行交易)
    pub dry_run: bool,
    /// 优先费 (Gwei) - 支持小数，如 0.005
    pub priority_fee_gwei: f64,
    /// 执行数量策略
    pub amount_strategy: ExecutionAmountStrategy,
    /// 执行前是否模拟
    pub simulate_before_execute: bool,
}

impl Default for ScannerExecutorConfig {
    fn default() -> Self {
        Self {
            auto_execute: false,
            arbitrage_contract: None,
            max_gas_price_gwei: 100.0,
            use_flashbots: false,
            flashbots_rpc_url: Some("https://relay.flashbots.net".to_string()),
            use_public_mempool: false,
            dry_run: true,
            priority_fee_gwei: 2.0,
            amount_strategy: ExecutionAmountStrategy::default(),
            simulate_before_execute: true,
        }
    }
}

/// 事件驱动套利扫描器配置
#[derive(Debug, Clone)]
pub struct EventDrivenScannerConfig {
    /// 链 ID
    pub chain_id: u64,
    /// 最小利润阈值 (USD) - 作为后备值
    pub min_profit_usd: Decimal,
    /// 最大滑点
    pub max_slippage: Decimal,
    /// 目标代币地址
    pub target_tokens: Vec<Address>,
    /// 兜底扫描间隔 (毫秒)
    pub fallback_scan_interval_ms: u64,
    /// 价格变化阈值 (触发检测的最小价格变化百分比)
    pub price_change_threshold: Decimal,
    /// 动态利润门槛配置
    pub dynamic_profit_config: DynamicProfitConfig,
    /// 是否启用动态利润门槛
    pub enable_dynamic_profit: bool,
    /// 最小交易金额过滤阈值 (USD) - 小于该值的交易不进行套利评估
    pub min_swap_value_usd: Decimal,
    /// 跳过本地计算阈值 (USD) - 超过该值直接用链上计算，避免大资金跨 Tick 时本地估算不准
    pub skip_local_calc_threshold_usd: Decimal,
    /// 执行器配置
    pub executor_config: ScannerExecutorConfig,
    /// 最大并发处理事件数量 (防止资源耗尽)
    pub max_concurrent_handlers: usize,
}

impl Default for EventDrivenScannerConfig {
    fn default() -> Self {
        Self {
            chain_id: 1, // Ethereum Mainnet
            min_profit_usd: dec!(0), // 只要净利润 > 0 就认为是机会
            max_slippage: dec!(0.005),
            target_tokens: Vec::new(),
            fallback_scan_interval_ms: 5000, // 5秒兜底扫描
            price_change_threshold: dec!(0.001), // 0.1% 价格变化触发检测
            dynamic_profit_config: DynamicProfitConfig::default(),
            enable_dynamic_profit: true, // 默认启用动态门槛
            min_swap_value_usd: dec!(1), // 默认 $1，小于该值的交易不进行套利评估
            skip_local_calc_threshold_usd: dec!(5000), // 默认 $5000，超过此值跳过本地计算直接链上计算
            executor_config: ScannerExecutorConfig::default(),
            max_concurrent_handlers: 5, // 默认最多同时处理 5 个 swap 事件
        }
    }
}

/// Gas 价格缓存
struct GasPriceCache {
    price_wei: U256,
    last_updated: std::time::Instant,
}

/// RPC 调用类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RpcCallType {
    /// Multicall 批量刷新池子状态
    MulticallRefreshPools,
    /// QuoterV2 链上报价
    QuoterV2Quote,
    /// 获取 Gas Price
    GetGasPrice,
    /// ERC20 余额查询
    Erc20BalanceOf,
}

impl RpcCallType {
    fn name(&self) -> &'static str {
        match self {
            RpcCallType::MulticallRefreshPools => "Multicall刷新池子",
            RpcCallType::QuoterV2Quote => "QuoterV2报价",
            RpcCallType::GetGasPrice => "Gas Price查询",
            RpcCallType::Erc20BalanceOf => "ERC20余额查询",
        }
    }
}

/// 单个 RPC 类型的统计
#[derive(Debug, Clone, Default)]
struct RpcTypeStats {
    /// 总调用次数 (启动以来)
    total_calls: u64,
    /// 当前分钟调用次数
    current_minute_calls: u64,
    /// 上一分钟调用次数 (用于显示)
    last_minute_calls: u64,
    /// 总耗时 (毫秒)
    total_duration_ms: u64,
    /// 失败次数
    failed_calls: u64,
}

/// RPC 调用统计
pub struct RpcStats {
    /// 各类型统计
    stats: std::sync::RwLock<HashMap<RpcCallType, RpcTypeStats>>,
    /// 启动时间
    start_time: std::time::Instant,
    /// 当前分钟开始时间
    current_minute_start: std::sync::RwLock<std::time::Instant>,
}

impl RpcStats {
    pub fn new() -> Self {
        Self {
            stats: std::sync::RwLock::new(HashMap::new()),
            start_time: std::time::Instant::now(),
            current_minute_start: std::sync::RwLock::new(std::time::Instant::now()),
        }
    }

    /// 记录一次 RPC 调用
    pub fn record_call(&self, call_type: RpcCallType, duration_ms: u64, success: bool) {
        let mut stats = self.stats.write().unwrap();
        let entry = stats.entry(call_type).or_insert_with(RpcTypeStats::default);
        entry.total_calls += 1;
        entry.current_minute_calls += 1;
        entry.total_duration_ms += duration_ms;
        if !success {
            entry.failed_calls += 1;
        }
    }

    /// 切换到新的一分钟 (在每个新区块时检查)
    pub fn maybe_rotate_minute(&self) {
        let mut minute_start = self.current_minute_start.write().unwrap();
        if minute_start.elapsed().as_secs() >= 60 {
            // 切换分钟
            let mut stats = self.stats.write().unwrap();
            for (_, type_stats) in stats.iter_mut() {
                type_stats.last_minute_calls = type_stats.current_minute_calls;
                type_stats.current_minute_calls = 0;
            }
            *minute_start = std::time::Instant::now();
        }
    }

    /// 获取统计摘要
    pub fn get_summary(&self) -> String {
        let stats = self.stats.read().unwrap();
        let uptime_secs = self.start_time.elapsed().as_secs();
        let uptime_mins = uptime_secs / 60;
        let uptime_hours = uptime_mins / 60;

        let mut lines = Vec::new();
        lines.push(format!(
            "📊 RPC 调用统计 (运行时间: {}h {}m {}s)",
            uptime_hours, uptime_mins % 60, uptime_secs % 60
        ));
        lines.push("─".repeat(60));
        lines.push(format!(
            "{:<20} {:>10} {:>10} {:>10} {:>10}",
            "类型", "总调用", "上分钟", "当前分钟", "平均耗时"
        ));
        lines.push("─".repeat(60));

        let call_types = [
            RpcCallType::MulticallRefreshPools,
            RpcCallType::QuoterV2Quote,
            RpcCallType::GetGasPrice,
            RpcCallType::Erc20BalanceOf,
        ];

        let mut total_calls = 0u64;
        let mut total_last_min = 0u64;
        let mut total_current_min = 0u64;

        for call_type in &call_types {
            let type_stats = stats.get(call_type).cloned().unwrap_or_default();
            let avg_ms = if type_stats.total_calls > 0 {
                type_stats.total_duration_ms / type_stats.total_calls
            } else {
                0
            };

            total_calls += type_stats.total_calls;
            total_last_min += type_stats.last_minute_calls;
            total_current_min += type_stats.current_minute_calls;

            lines.push(format!(
                "{:<20} {:>10} {:>10} {:>10} {:>8}ms",
                call_type.name(),
                type_stats.total_calls,
                type_stats.last_minute_calls,
                type_stats.current_minute_calls,
                avg_ms
            ));
        }

        lines.push("─".repeat(60));
        lines.push(format!(
            "{:<20} {:>10} {:>10} {:>10}",
            "合计", total_calls, total_last_min, total_current_min
        ));

        // 计算每分钟平均
        if uptime_mins > 0 {
            lines.push(format!(
                "📈 平均: {:.1} 次/分钟",
                total_calls as f64 / uptime_mins as f64
            ));
        }

        lines.join("\n")
    }
}

/// 代币配置信息 (从数据库加载)
#[derive(Debug, Clone)]
pub struct TokenConfig {
    pub address: Address,
    pub symbol: String,
    pub decimals: u8,
    pub is_stable: bool,
    pub price_symbol: String,
    pub optimal_input_amount: U256,
}

/// 三角套利组合配置 (从数据库加载) - 保留用于向后兼容
#[derive(Debug, Clone)]
pub struct TriangleConfig {
    pub name: String,
    pub token_a: Address,
    pub token_b: Address,
    pub token_c: Address,
    pub priority: i32,
    pub category: String,
}

/// 池子触发的套利路径配置 (从数据库加载)
#[derive(Debug, Clone)]
pub struct PoolPathConfig {
    pub path_name: String,
    pub triangle_name: String,
    pub token_a: Address,
    pub token_b: Address,
    pub token_c: Address,
    pub priority: i32,
}

/// 链合约地址配置 (用于扫描器)
#[derive(Debug, Clone)]
pub struct ChainContractsConfig {
    /// Quoter 合约地址
    pub quoter_address: Address,
    /// Multicall3 合约地址
    pub multicall_address: Address,
    /// 链名称 (用于日志)
    pub chain_name: String,
}

impl ChainContractsConfig {
    /// 以太坊主网配置
    pub fn ethereum() -> Self {
        Self {
            quoter_address: "0x61fFE014bA17989E743c5F6cB21bF9697530B21e".parse().unwrap(),
            multicall_address: "0xcA11bde05977b3631167028862bE2a173976CA11".parse().unwrap(),
            chain_name: "Ethereum".to_string(),
        }
    }

    /// BSC 主网配置
    pub fn bsc() -> Self {
        Self {
            quoter_address: "0xB048Bbc1Ee6b733FFfCFb9e9CeF7375518e25997".parse().unwrap(),
            multicall_address: "0xcA11bde05977b3631167028862bE2a173976CA11".parse().unwrap(),
            chain_name: "BSC".to_string(),
        }
    }

    /// Polygon 主网配置
    pub fn polygon() -> Self {
        Self {
            quoter_address: "0x61fFE014bA17989E743c5F6cB21bF9697530B21e".parse().unwrap(),
            multicall_address: "0xcA11bde05977b3631167028862bE2a173976CA11".parse().unwrap(),
            chain_name: "Polygon".to_string(),
        }
    }

    /// Arbitrum 主网配置
    pub fn arbitrum() -> Self {
        Self {
            quoter_address: "0x61fFE014bA17989E743c5F6cB21bF9697530B21e".parse().unwrap(),
            multicall_address: "0xcA11bde05977b3631167028862bE2a173976CA11".parse().unwrap(),
            chain_name: "Arbitrum".to_string(),
        }
    }

    /// Base 主网配置
    pub fn base() -> Self {
        Self {
            quoter_address: "0x3d4e44Eb1374240CE5F1B871ab261CD16335B76a".parse().unwrap(),
            multicall_address: "0xcA11bde05977b3631167028862bE2a173976CA11".parse().unwrap(),
            chain_name: "Base".to_string(),
        }
    }

    /// 根据 chain_id 获取配置
    pub fn for_chain(chain_id: u64) -> Option<Self> {
        match chain_id {
            1 => Some(Self::ethereum()),
            56 => Some(Self::bsc()),
            137 => Some(Self::polygon()),
            42161 => Some(Self::arbitrum()),
            8453 => Some(Self::base()),
            _ => None,
        }
    }
}

/// 已执行记录 (用于去重)
#[derive(Debug, Clone)]
struct ExecutedRecord {
    /// 执行时间
    executed_at: std::time::Instant,
    /// 区块号 (保留用于调试)
    #[allow(dead_code)]
    block_number: u64,
}

/// 事件驱动套利扫描器
pub struct EventDrivenScanner<M: Middleware> {
    config: EventDrivenScannerConfig,
    /// 链上 provider
    provider: Arc<M>,
    /// 价格服务
    price_service: SharedPriceService,
    /// Quoter 合约地址
    quoter_address: Address,
    /// Multicall3 合约地址
    #[allow(dead_code)]
    multicall_address: Address,
    /// 链名称 (用于日志)
    chain_name: String,
    /// 池子状态缓存: address -> PoolState
    pool_states: RwLock<HashMap<Address, PoolState>>,
    /// 代币配置缓存: address -> TokenConfig
    token_configs: RwLock<HashMap<Address, TokenConfig>>,
    /// 三角套利组合配置缓存 (保留用于向后兼容)
    triangle_configs: RwLock<Vec<TriangleConfig>>,
    /// 池子-路径映射缓存: trigger_pool_address -> Vec<PoolPathConfig>
    pool_path_mappings: RwLock<HashMap<Address, Vec<PoolPathConfig>>>,
    /// 发现的套利机会
    opportunities: RwLock<Vec<ArbitrageOpportunity>>,
    /// 是否正在运行
    running: RwLock<bool>,
    /// Gas 价格缓存 (30秒更新一次)
    gas_price_cache: RwLock<Option<GasPriceCache>>,
    /// 当前区块号 (用于检查缓存新鲜度)
    current_block: AtomicU64,
    /// 钱包 (用于执行交易)
    wallet: RwLock<Option<LocalWallet>>,
    /// 私钥字符串 (用于创建执行器)
    private_key: RwLock<Option<String>>,
    /// 执行统计
    execution_stats: RwLock<ExecutionStats>,
    /// 并发控制信号量
    handler_semaphore: Arc<Semaphore>,
    /// 已执行的机会记录 (路径签名 -> 执行记录)，用于去重
    executed_opportunities: RwLock<HashMap<String, ExecutedRecord>>,
    /// 正在执行的池子集合，用于防止同一池子并发执行
    executing_pools: RwLock<std::collections::HashSet<Address>>,
    /// 已处理的 swap 事件 tx_hash (用于防止 WS 重复推送同一事件)
    processed_tx_hashes: RwLock<HashMap<H256, std::time::Instant>>,
    /// RPC 调用统计
    rpc_stats: Arc<RpcStats>,
}

/// 执行统计
#[derive(Debug, Clone, Default)]
pub struct ExecutionStats {
    /// 总执行次数
    pub total_executions: u64,
    /// 成功次数
    pub successful_executions: u64,
    /// 失败次数
    pub failed_executions: u64,
    /// 干运行次数
    pub dry_run_executions: u64,
    /// 总利润 (USD)
    pub total_profit_usd: Decimal,
    /// 当前正在处理的事件数
    pub active_handlers: u64,
    /// 被丢弃的事件数 (并发数已满时)
    pub dropped_events: u64,
    /// 重复事件被跳过的次数
    pub duplicates_skipped: u64,
    /// 因池子正在执行而跳过的次数
    pub pool_busy_skipped: u64,
}

/// Uniswap V3 QuoterV2 地址 (Ethereum Mainnet) - 返回 gas 估算
#[allow(dead_code)]
const UNISWAP_V3_QUOTER_V2: &str = "0x61fFE014bA17989E743c5F6cB21bF9697530B21e";
/// Multicall3 合约地址 (通用)
const DEFAULT_MULTICALL3: &str = "0xcA11bde05977b3631167028862bE2a173976CA11";

impl<M: Middleware + 'static> EventDrivenScanner<M> {
    /// 创建扫描器 (使用默认以太坊配置，保持向后兼容)
    pub fn new(config: EventDrivenScannerConfig, provider: Arc<M>, price_service: SharedPriceService) -> Self {
        Self::with_chain_config(
            config,
            provider,
            price_service,
            ChainContractsConfig::ethereum(),
        )
    }

    /// 使用链配置创建扫描器 (推荐方式)
    pub fn with_chain_config(
        config: EventDrivenScannerConfig,
        provider: Arc<M>,
        price_service: SharedPriceService,
        chain_contracts: ChainContractsConfig,
    ) -> Self {
        let max_concurrent = config.max_concurrent_handlers;
        info!("[{}] 创建事件驱动扫描器, chain_id={}, quoter={:?}, auto_execute={}, max_concurrent={}",
              chain_contracts.chain_name, config.chain_id, chain_contracts.quoter_address,
              config.executor_config.auto_execute, max_concurrent);
        Self {
            handler_semaphore: Arc::new(Semaphore::new(max_concurrent)),
            config,
            provider,
            price_service,
            quoter_address: chain_contracts.quoter_address,
            multicall_address: chain_contracts.multicall_address,
            chain_name: chain_contracts.chain_name,
            pool_states: RwLock::new(HashMap::new()),
            token_configs: RwLock::new(HashMap::new()),
            triangle_configs: RwLock::new(Vec::new()),
            pool_path_mappings: RwLock::new(HashMap::new()),
            opportunities: RwLock::new(Vec::new()),
            running: RwLock::new(false),
            gas_price_cache: RwLock::new(None),
            current_block: AtomicU64::new(0),
            wallet: RwLock::new(None),
            private_key: RwLock::new(None),
            execution_stats: RwLock::new(ExecutionStats::default()),
            executed_opportunities: RwLock::new(HashMap::new()),
            executing_pools: RwLock::new(std::collections::HashSet::new()),
            processed_tx_hashes: RwLock::new(HashMap::new()),
            rpc_stats: Arc::new(RpcStats::new()),
        }
    }

    /// 使用自定义 Quoter 地址创建 (保持向后兼容)
    pub fn with_quoter(config: EventDrivenScannerConfig, provider: Arc<M>, price_service: SharedPriceService, quoter_address: Address) -> Self {
        let max_concurrent = config.max_concurrent_handlers;
        Self {
            handler_semaphore: Arc::new(Semaphore::new(max_concurrent)),
            config,
            provider,
            price_service,
            quoter_address,
            multicall_address: DEFAULT_MULTICALL3.parse().unwrap(),
            chain_name: "Unknown".to_string(),
            pool_states: RwLock::new(HashMap::new()),
            token_configs: RwLock::new(HashMap::new()),
            triangle_configs: RwLock::new(Vec::new()),
            pool_path_mappings: RwLock::new(HashMap::new()),
            opportunities: RwLock::new(Vec::new()),
            running: RwLock::new(false),
            gas_price_cache: RwLock::new(None),
            current_block: AtomicU64::new(0),
            wallet: RwLock::new(None),
            private_key: RwLock::new(None),
            execution_stats: RwLock::new(ExecutionStats::default()),
            executed_opportunities: RwLock::new(HashMap::new()),
            executing_pools: RwLock::new(std::collections::HashSet::new()),
            processed_tx_hashes: RwLock::new(HashMap::new()),
            rpc_stats: Arc::new(RpcStats::new()),
        }
    }

    /// 设置钱包 (用于执行交易)
    pub async fn set_wallet(&self, wallet: LocalWallet, private_key: String) {
        let mut w = self.wallet.write().await;
        *w = Some(wallet);
        let mut pk = self.private_key.write().await;
        *pk = Some(private_key);
        info!("[{}] 钱包已设置", self.chain_name);
    }

    /// 获取执行统计
    pub async fn get_execution_stats(&self) -> ExecutionStats {
        self.execution_stats.read().await.clone()
    }

    /// 获取 RPC 调用统计
    pub fn get_rpc_stats(&self) -> Arc<RpcStats> {
        self.rpc_stats.clone()
    }

    /// 打印 RPC 统计摘要
    pub fn print_rpc_stats(&self) {
        info!("\n{}", self.rpc_stats.get_summary());
    }

    /// 获取链名称
    pub fn chain_name(&self) -> &str {
        &self.chain_name
    }

    /// 获取链 ID
    pub fn chain_id(&self) -> u64 {
        self.config.chain_id
    }

    /// 添加代币配置
    pub async fn add_token_config(&self, config: TokenConfig) {
        let mut configs = self.token_configs.write().await;
        info!("添加代币配置: {} ({:?}), decimals={}, optimal_input={}",
              config.symbol, config.address, config.decimals, config.optimal_input_amount);
        configs.insert(config.address, config);
    }

    /// 批量添加代币配置
    pub async fn add_token_configs(&self, configs: Vec<TokenConfig>) {
        let mut token_configs = self.token_configs.write().await;
        for config in configs {
            info!("添加代币配置: {} ({:?})", config.symbol, config.address);
            token_configs.insert(config.address, config);
        }
    }

    /// 批量添加三角套利组合配置 (保留用于向后兼容)
    pub async fn add_triangle_configs(&self, configs: Vec<TriangleConfig>) {
        let mut triangle_configs = self.triangle_configs.write().await;
        let count = configs.len();
        for config in configs {
            info!("添加三角配置: {} | {} -> {} -> {} | 优先级={} | 类型={}",
                  config.name,
                  format!("{:?}", config.token_a)[0..10].to_string(),
                  format!("{:?}", config.token_b)[0..10].to_string(),
                  format!("{:?}", config.token_c)[0..10].to_string(),
                  config.priority, config.category);
            triangle_configs.push(config);
        }
        info!("已加载 {} 个三角套利组合配置", count);
    }

    /// 添加池子-路径映射配置
    /// trigger_pool: 触发池子地址
    /// paths: 该池子触发时应检查的所有路径
    pub async fn add_pool_path_mapping(&self, trigger_pool: Address, paths: Vec<PoolPathConfig>) {
        let mut mappings = self.pool_path_mappings.write().await;
        let path_count = paths.len();

        info!("添加池子-路径映射: {:?} -> {} 条路径", trigger_pool, path_count);
        for path in &paths {
            debug!("   路径: {} | {} -> {} -> {} -> {} | 优先级={}",
                  path.path_name,
                  format!("{:?}", path.token_a)[0..10].to_string(),
                  format!("{:?}", path.token_b)[0..10].to_string(),
                  format!("{:?}", path.token_c)[0..10].to_string(),
                  format!("{:?}", path.token_a)[0..10].to_string(),
                  path.priority);
        }

        mappings.insert(trigger_pool, paths);
    }

    /// 批量添加池子-路径映射配置
    pub async fn add_pool_path_mappings(&self, mappings_list: Vec<(Address, Vec<PoolPathConfig>)>) {
        let mut mappings = self.pool_path_mappings.write().await;
        let pool_count = mappings_list.len();
        let mut total_paths = 0;

        info!("开始加载池子-路径映射...");
        for (trigger_pool, paths) in mappings_list {
            let path_count = paths.len();
            total_paths += path_count;
            debug!("   加载触发池子 {:?} -> {} 条路径", trigger_pool, path_count);
            mappings.insert(trigger_pool, paths);
        }

        // 输出所有已加载的触发池子地址
        let loaded_pools: Vec<String> = mappings.keys()
            .map(|addr| format!("{:?}", addr))
            .collect();
        info!("✅ 已加载 {} 个池子的路径映射，共 {} 条路径", pool_count, total_paths);
        info!("📋 触发池子列表: {:?}", loaded_pools);
    }

    /// 获取池子-路径映射数量
    pub async fn pool_path_mapping_count(&self) -> (usize, usize) {
        let mappings = self.pool_path_mappings.read().await;
        let pool_count = mappings.len();
        let path_count: usize = mappings.values().map(|v| v.len()).sum();
        (pool_count, path_count)
    }

    /// 获取指定池子触发时应检查的路径
    async fn get_paths_for_pool(&self, pool_address: Address) -> Vec<PoolPathConfig> {
        let mappings = self.pool_path_mappings.read().await;
        let result = mappings.get(&pool_address).cloned().unwrap_or_default();

        if result.is_empty() && !mappings.is_empty() {
            // 调试日志：显示已配置的 trigger_pool 列表
            let configured_pools: Vec<String> = mappings.keys()
                .map(|addr| format!("{:?}", addr))
                .collect();
            debug!(
                "⚠️ 池子 {:?} 不在路径映射中 | 已配置的池子数={} | 示例: {:?}",
                pool_address,
                mappings.len(),
                configured_pools.iter().take(5).collect::<Vec<_>>()
            );
        }

        result
    }

    /// 检查三角组合是否在配置中（任意顺序和方向都算匹配）
    /// 因为 A->B->C->A 和 A->C->B->A 是同一个三角形的两个方向
    /// 注意: 如果使用了池子-路径映射，此方法不再需要
    async fn is_valid_triangle(&self, token_a: Address, token_b: Address, token_c: Address) -> bool {
        let configs = self.triangle_configs.read().await;

        // 如果没有配置三角组合，允许所有（向后兼容）
        if configs.is_empty() {
            return true;
        }

        // 创建一个排序后的代币集合来比较（忽略顺序）
        let mut tokens = [token_a, token_b, token_c];
        tokens.sort();

        for config in configs.iter() {
            let mut config_tokens = [config.token_a, config.token_b, config.token_c];
            config_tokens.sort();

            if tokens == config_tokens {
                return true;
            }
        }

        false
    }

    /// 获取三角配置数量
    pub async fn triangle_config_count(&self) -> usize {
        self.triangle_configs.read().await.len()
    }

    /// 获取代币配置
    async fn get_token_config(&self, address: Address) -> Option<TokenConfig> {
        let configs = self.token_configs.read().await;
        configs.get(&address).cloned()
    }

    /// 调用链上 QuoterV2 获取真实报价和 gas 估算
    async fn quote_exact_input(
        &self,
        token_in: Address,
        token_out: Address,
        fee: u32,
        amount_in: U256,
    ) -> Result<QuoteResult> {
        let quoter = UniswapV3QuoterV2::new(self.quoter_address, self.provider.clone());

        // QuoterV2 使用 tuple 参数
        let params = (
            token_in,
            token_out,
            amount_in,
            fee.try_into().unwrap_or(3000u32),
            U256::zero(), // sqrtPriceLimitX96 = 0 表示无限制
        );

        // 执行 RPC 调用并计时
        let rpc_start = std::time::Instant::now();
        let call_result = quoter
            .quote_exact_input_single(params)
            .call()
            .await;
        let rpc_elapsed = rpc_start.elapsed();

        match call_result {
            Ok(result) => {
                // 记录成功的 RPC 调用
                self.rpc_stats.record_call(
                    RpcCallType::QuoterV2Quote,
                    rpc_elapsed.as_millis() as u64,
                    true
                );
                // result: (amountOut, sqrtPriceX96After, initializedTicksCrossed, gasEstimate)
                Ok(QuoteResult {
                    amount_out: result.0,
                    gas_estimate: result.3,
                })
            }
            Err(e) => {
                // 记录失败的 RPC 调用
                self.rpc_stats.record_call(
                    RpcCallType::QuoterV2Quote,
                    rpc_elapsed.as_millis() as u64,
                    false
                );
                Err(e.into())
            }
        }
    }

    /// 添加池子到监控列表
    pub async fn add_pool(&self, pool: PoolState) {
        let mut states = self.pool_states.write().await;
        info!("添加池子到事件监控: {:?}, token0={:?}, token1={:?}",
              pool.address, pool.token0, pool.token1);
        states.insert(pool.address, pool);
    }

    /// 获取池子数量
    pub async fn pool_count(&self) -> usize {
        self.pool_states.read().await.len()
    }

    /// 获取当前区块号
    pub fn get_current_block(&self) -> u64 {
        self.current_block.load(Ordering::Relaxed)
    }

    /// 批量刷新过期池子的价格状态 (使用 Multicall 一次 RPC 查询多个池子)
    async fn refresh_stale_pools(&self, stale_pool_addrs: &[Address]) -> Result<()> {
        if stale_pool_addrs.is_empty() {
            return Ok(());
        }

        let multicall_addr: Address = MULTICALL3_ADDRESS.parse()?;
        let multicall = Multicall3::new(multicall_addr, self.provider.clone());

        // 构建 slot0() 和 liquidity() 调用
        // 每个池子需要 2 个调用
        let mut calls: Vec<multicall_3::Call3> = Vec::new();

        for &pool_addr in stale_pool_addrs {
            let pool = IUniswapV3Pool::new(pool_addr, self.provider.clone());

            // slot0() 调用
            let slot0_call = pool.slot_0().calldata().unwrap_or_default();
            calls.push(multicall_3::Call3 {
                target: pool_addr,
                allow_failure: true,
                call_data: slot0_call,
            });

            // liquidity() 调用
            let liquidity_call = pool.liquidity().calldata().unwrap_or_default();
            calls.push(multicall_3::Call3 {
                target: pool_addr,
                allow_failure: true,
                call_data: liquidity_call,
            });
        }

        info!(
            "🔄 批量刷新 {} 个过期池子 (Multicall {} 次调用)",
            stale_pool_addrs.len(),
            calls.len()
        );

        // 执行 Multicall 并计时
        let rpc_start = std::time::Instant::now();
        let results = match multicall.aggregate_3(calls.clone()).call().await {
            Ok(r) => {
                let rpc_elapsed = rpc_start.elapsed();
                // 记录 RPC 调用统计
                self.rpc_stats.record_call(
                    RpcCallType::MulticallRefreshPools,
                    rpc_elapsed.as_millis() as u64,
                    true
                );
                r
            }
            Err(e) => {
                let rpc_elapsed = rpc_start.elapsed();
                // 记录失败的 RPC 调用
                self.rpc_stats.record_call(
                    RpcCallType::MulticallRefreshPools,
                    rpc_elapsed.as_millis() as u64,
                    false
                );
                warn!("Multicall 失败: {}, 跳过刷新 | RPC耗时: {:.2}ms", e, rpc_elapsed.as_secs_f64() * 1000.0);
                return Ok(());
            }
        };
        let rpc_elapsed = rpc_start.elapsed();
        info!(
            "   📡 Multicall RPC 完成 | 耗时: {:.2}ms",
            rpc_elapsed.as_secs_f64() * 1000.0
        );

        // 解析结果并更新缓存
        let current_block = self.current_block.load(Ordering::Relaxed);
        let mut states = self.pool_states.write().await;

        // 收集需要打印的日志信息（避免在循环中调用异步函数）
        let mut log_entries: Vec<(Address, Address, Address, u32, U256, i32, u128)> = Vec::new();

        for (i, pool_addr) in stale_pool_addrs.iter().enumerate() {
            let slot0_idx = i * 2;
            let liquidity_idx = i * 2 + 1;

            if slot0_idx >= results.len() || liquidity_idx >= results.len() {
                continue;
            }

            let slot0_result = &results[slot0_idx];
            let liquidity_result = &results[liquidity_idx];

            // Result 是 tuple: (success: bool, returnData: Bytes)
            if !slot0_result.0 || !liquidity_result.0 {
                debug!("池子 {:?} 的 slot0/liquidity 调用失败", pool_addr);
                continue;
            }

            // 解析 slot0: (uint160 sqrtPriceX96, int24 tick, ...)
            let slot0_data = &slot0_result.1;
            let mut sqrt_price_x96 = U256::zero();
            let mut tick = 0i32;

            if slot0_data.len() >= 64 {
                sqrt_price_x96 = U256::from_big_endian(&slot0_data[0..32]);
                // tick 在第二个 32 字节槽位，是 int24
                let tick_bytes: [u8; 4] = slot0_data[60..64].try_into().unwrap_or([0; 4]);
                tick = i32::from_be_bytes(tick_bytes);

                if let Some(pool) = states.get_mut(pool_addr) {
                    pool.sqrt_price_x96 = Some(sqrt_price_x96);
                    pool.tick = Some(tick);
                    pool.last_block = current_block;
                    pool.last_updated = std::time::Instant::now();
                }
            }

            // 解析 liquidity: uint128
            let liquidity_data = &liquidity_result.1;
            let mut liquidity = 0u128;

            if liquidity_data.len() >= 32 {
                let mut liq_bytes = [0u8; 16];
                liq_bytes.copy_from_slice(&liquidity_data[16..32]);
                liquidity = u128::from_be_bytes(liq_bytes);

                if let Some(pool) = states.get_mut(pool_addr) {
                    pool.liquidity = Some(liquidity);
                }
            }

            // 收集日志信息
            if let Some(pool) = states.get(pool_addr) {
                log_entries.push((
                    *pool_addr,
                    pool.token0,
                    pool.token1,
                    pool.fee,
                    sqrt_price_x96,
                    tick,
                    liquidity,
                ));
            }
        }

        // 释放写锁后打印日志
        drop(states);

        // 打印可读的日志信息
        for (pool_addr, token0, token1, fee, sqrt_price_x96, tick, liquidity) in log_entries {
            let token0_info = self.get_token_info(token0).await;
            let token1_info = self.get_token_info(token1).await;

            // 计算人类可读的价格
            let price = sqrt_price_x96_to_price(sqrt_price_x96, token0_info.decimals, token1_info.decimals);
            let fee_percent = fee as f64 / 10000.0;

            // 短地址格式
            let addr_short = format!("{:?}", pool_addr);
            let addr_short = &addr_short[0..10];

            info!(
                "   ✅ {}/{}({:.2}%) [{}..]: 价格={:.6} {}/{}, tick={}, 流动性={}",
                token0_info.symbol,
                token1_info.symbol,
                fee_percent,
                addr_short,
                price,
                token1_info.symbol,
                token0_info.symbol,
                tick,
                format_liquidity(liquidity)
            );
        }

        Ok(())
    }

    /// 每个新区块批量刷新所有池子状态 (一次 Multicall)
    /// 这样本地计算时总是使用当前区块的最新数据
    async fn refresh_all_pools(&self) -> Result<()> {
        let all_pool_addrs: Vec<Address> = {
            let states = self.pool_states.read().await;
            states.keys().cloned().collect()
        };

        if all_pool_addrs.is_empty() {
            return Ok(());
        }

        debug!(
            "[{}] 🔄 新区块刷新所有 {} 个池子状态",
            self.chain_name,
            all_pool_addrs.len()
        );

        // 复用现有的批量刷新逻辑
        self.refresh_stale_pools(&all_pool_addrs).await
    }

    /// 本地计算 V3 报价 (简化版，不考虑跨 tick)
    ///
    /// 用于快速筛选套利机会，替代链上 QuoterV2 调用
    /// 注意：这是简化计算，只在当前 tick 范围内有效
    /// - 对于小额 swap（不跨 tick），精度足够
    /// - 对于大额 swap 可能有误差，但用于筛选足够
    /// - 最终执行前可选做链上验证
    fn calculate_amount_out_local(
        &self,
        sqrt_price_x96: U256,
        liquidity: u128,
        amount_in: U256,
        zero_for_one: bool,
        fee: u32,
    ) -> Option<U256> {
        if liquidity == 0 || sqrt_price_x96.is_zero() || amount_in.is_zero() {
            return None;
        }

        // 扣除手续费
        let fee_factor = U256::from(1_000_000u64 - fee as u64);
        let amount_in_after_fee = amount_in * fee_factor / U256::from(1_000_000u64);

        // Q96 = 2^96
        let q96 = U256::from(1u128) << 96;
        let _liquidity_u256 = U256::from(liquidity);

        // Uniswap V3 价格公式:
        // price = (sqrtPriceX96 / 2^96)^2 = sqrtPriceX96^2 / 2^192
        //
        // 对于 zero_for_one (token0 -> token1):
        //   amount_out ≈ amount_in * price = amount_in * sqrtPriceX96^2 / 2^192
        //
        // 对于 one_for_zero (token1 -> token0):
        //   amount_out ≈ amount_in / price = amount_in * 2^192 / sqrtPriceX96^2

        let amount_out = if zero_for_one {
            // token0 -> token1
            // 简化计算: amount_out ≈ amount_in * sqrtPriceX96 / Q96 * sqrtPriceX96 / Q96
            let intermediate = amount_in_after_fee
                .checked_mul(sqrt_price_x96)?
                .checked_div(q96)?;
            intermediate.checked_mul(sqrt_price_x96)?.checked_div(q96)?
        } else {
            // token1 -> token0
            // 简化计算: amount_out ≈ amount_in * Q96 / sqrtPriceX96 * Q96 / sqrtPriceX96
            let intermediate = amount_in_after_fee
                .checked_mul(q96)?
                .checked_div(sqrt_price_x96)?;
            intermediate.checked_mul(q96)?.checked_div(sqrt_price_x96)?
        };

        // 应用滑点保护：本地计算可能不精确，打个 95% 折扣
        Some(amount_out * U256::from(95u64) / U256::from(100u64))
    }

    /// 本地快速估算三角套利利润
    ///
    /// 用于快速筛选，替代链上 QuoterV2 调用
    /// 注意：本地估算不考虑跨 tick，但对于套利场景：
    /// - 套利金额通常较小，不会跨越多个 tick
    /// - 即使有误差，能过滤掉大部分无利润的路径
    #[allow(dead_code)]
    fn estimate_profit_local(
        &self,
        input_amount: U256,
        pool1: &PoolState,
        pool2: &PoolState,
        pool3: &PoolState,
        token_a: Address,
        token_b: Address,
        token_c: Address,
    ) -> Option<U256> {
        // 检查所有池子是否有 V3 价格数据
        if !pool1.has_v3_price_data() || !pool2.has_v3_price_data() || !pool3.has_v3_price_data() {
            return None;
        }

        let sqrt_price1 = pool1.sqrt_price_x96?;
        let liquidity1 = pool1.liquidity?;
        let sqrt_price2 = pool2.sqrt_price_x96?;
        let liquidity2 = pool2.liquidity?;
        let sqrt_price3 = pool3.sqrt_price_x96?;
        let liquidity3 = pool3.liquidity?;

        // Step 1: A -> B
        let zero_for_one1 = pool1.token0 == token_a;
        let out1 = self.calculate_amount_out_local(sqrt_price1, liquidity1, input_amount, zero_for_one1, pool1.fee)?;

        // Step 2: B -> C
        let zero_for_one2 = pool2.token0 == token_b;
        let out2 = self.calculate_amount_out_local(sqrt_price2, liquidity2, out1, zero_for_one2, pool2.fee)?;

        // Step 3: C -> A
        let zero_for_one3 = pool3.token0 == token_c;
        let out3 = self.calculate_amount_out_local(sqrt_price3, liquidity3, out2, zero_for_one3, pool3.fee)?;

        // 检查是否盈利
        if out3 > input_amount {
            Some(out3 - input_amount)
        } else {
            None
        }
    }

    /// 处理 Swap 事件 - 核心方法
    pub async fn handle_swap_event(&self, event: SwapEvent) -> Option<ArbitrageOpportunity> {
        // 开始计时
        let start_time = std::time::Instant::now();

        // 更新当前区块号
        self.current_block.store(event.block_number, Ordering::Relaxed);

        // 1. 检查是否是我们监控的池子
        let (pool_updated, pool_info, token0, token1) = {
            let mut states = self.pool_states.write().await;
            if let Some(pool) = states.get_mut(&event.pool_address) {
                pool.update_from_swap(&event);
                let info = format!("{:?}", pool.dex_type);
                (true, Some(info), pool.token0, pool.token1)
            } else {
                // 不是我们监控的池子
                (false, None, Address::zero(), Address::zero())
            }
        };

        if !pool_updated {
            // 跳过不监控的池子（这是正常的）
            return None;
        }

        // 获取代币信息 (从价格服务)
        let token0_info = self.get_token_info(token0).await;
        let token1_info = self.get_token_info(token1).await;

        // 确定 swap 方向和金额
        let (token_in, token_out, amount_in, amount_out) = if event.amount0_in > U256::zero() {
            // token0 -> token1
            (token0_info.clone(), token1_info.clone(), event.amount0_in, event.amount1_out)
        } else {
            // token1 -> token0
            (token1_info.clone(), token0_info.clone(), event.amount1_in, event.amount0_out)
        };

        // 格式化数量
        let amount_in_fmt = format_token_amount(amount_in, token_in.decimals);
        let amount_out_fmt = format_token_amount(amount_out, token_out.decimals);

        // 计算美金价值
        let usd_in = self.calculate_usd_value(amount_in, &token_in);
        let usd_out = self.calculate_usd_value(amount_out, &token_out);
        let swap_usd = if usd_in > Decimal::ZERO { usd_in } else { usd_out };

        // 输出详细日志 (包含代币价格)
        info!("┌─────────────────────────────────────────────────────────────────────────────┐");
        info!("│ 🔍 触发套利检测 - Swap 事件详情");
        info!("├─────────────────────────────────────────────────────────────────────────────┤");
        info!("│ 📊 交易对: {} -> {}", token_in.symbol, token_out.symbol);
        info!("│ 💰 输入: {} {} @ ${:.4}/个 = ${:.2}",
            amount_in_fmt, token_in.symbol, token_in.price_usd, usd_in);
        info!("│ 💰 输出: {} {} @ ${:.4}/个 = ${:.2}",
            amount_out_fmt, token_out.symbol, token_out.price_usd, usd_out);
        info!("│ 🏊 池子: {:?} ({})", event.pool_address, pool_info.as_deref().unwrap_or("?"));
        info!("│ 📦 区块: #{}", event.block_number);
        info!("└─────────────────────────────────────────────────────────────────────────────┘");

        // 过滤小额交易：资金 < 配置阈值 不进行套利评估
        let min_swap_value = self.config.min_swap_value_usd;
        if swap_usd < min_swap_value {
            let elapsed = start_time.elapsed();
            info!("⏭️ 跳过小额交易: ${:.2} < ${} | 耗时: {:.2}ms", swap_usd, min_swap_value, elapsed.as_secs_f64() * 1000.0);
            return None;
        }

        // 2. 检测涉及该池子的套利机会（传递真实交易量用于本地估算）
        let detect_start = std::time::Instant::now();
        let result = self.detect_arbitrage_for_pool(event.pool_address, swap_usd).await;
        let detect_elapsed = detect_start.elapsed();

        // 计算总耗时
        let total_elapsed = start_time.elapsed();

        match &result {
            Some(opp) => {
                info!(
                    target: "arbitrage_opportunity",
                    "💰 发现套利机会! 净利润=${:.2} | 检测耗时: {:.2}ms | 总耗时: {:.2}ms",
                    opp.net_profit_usd,
                    detect_elapsed.as_secs_f64() * 1000.0,
                    total_elapsed.as_secs_f64() * 1000.0
                );

                // 写入专用套利机会日志
                self.log_opportunity(opp, &event, &token_in, &token_out, swap_usd).await;

                // 如果启用了自动执行，立即执行套利
                if self.config.executor_config.auto_execute {
                    let exec_start = std::time::Instant::now();
                    match self.execute_arbitrage(opp.clone()).await {
                        Ok(exec_result) => {
                            let exec_elapsed = exec_start.elapsed();
                            info!(
                                target: "arbitrage_execution",
                                "🚀 套利执行完成: status={:?}, tx_hash={:?} | 执行耗时: {:.2}ms",
                                exec_result.status,
                                exec_result.tx_hash,
                                exec_elapsed.as_secs_f64() * 1000.0
                            );
                        }
                        Err(e) => {
                            let exec_elapsed = exec_start.elapsed();
                            error!(
                                target: "arbitrage_execution",
                                "❌ 套利执行失败: {} | 执行耗时: {:.2}ms",
                                e,
                                exec_elapsed.as_secs_f64() * 1000.0
                            );
                        }
                    }
                }
            }
            None => {
                info!(
                    "📊 未发现套利机会 | 检测耗时: {:.2}ms | 总耗时: {:.2}ms",
                    detect_elapsed.as_secs_f64() * 1000.0,
                    total_elapsed.as_secs_f64() * 1000.0
                );
            }
        }

        result
    }

    /// 生成套利路径的唯一签名 (用于去重)
    fn generate_path_signature(&self, opportunity: &ArbitrageOpportunity) -> String {
        // 签名格式: chain_id:start_token:pool1:pool2:pool3:block_number
        let mut sig = format!("{}:{:?}", self.config.chain_id, opportunity.path.start_token);
        for hop in &opportunity.path.hops {
            sig.push_str(&format!(":{:?}", hop.pool_address));
        }
        // 加入区块号，同一区块内的相同路径视为重复
        sig.push_str(&format!(":{}", opportunity.block_number));
        sig
    }

    /// 获取套利路径涉及的所有池子地址
    fn get_path_pools(&self, opportunity: &ArbitrageOpportunity) -> Vec<Address> {
        opportunity.path.hops.iter().map(|hop| hop.pool_address).collect()
    }

    /// 清理过期的执行记录 (30秒过期)
    async fn cleanup_executed_records(&self) {
        const EXPIRY_SECS: u64 = 30;
        let now = std::time::Instant::now();
        let mut records = self.executed_opportunities.write().await;
        records.retain(|_, record| {
            now.duration_since(record.executed_at).as_secs() < EXPIRY_SECS
        });
    }

    /// 执行套利交易 (带去重检查)
    async fn execute_arbitrage(&self, mut opportunity: ArbitrageOpportunity) -> Result<models::ArbitrageResult> {
        let exec_config = &self.config.executor_config;

        // 生成路径签名
        let path_signature = self.generate_path_signature(&opportunity);
        let path_pools = self.get_path_pools(&opportunity);

        // ========== 去重检查 ==========

        // 1. 检查是否在时间窗口内已执行过相同路径
        {
            let records = self.executed_opportunities.read().await;
            if let Some(record) = records.get(&path_signature) {
                let elapsed = record.executed_at.elapsed().as_secs();
                if elapsed < 30 {
                    // 30秒内已执行过，跳过
                    let mut stats = self.execution_stats.write().await;
                    stats.duplicates_skipped += 1;
                    warn!(
                        "[{}] ⏭️ 跳过重复套利: 路径签名={}, 上次执行={:.1}秒前, 累计跳过={}",
                        self.chain_name, path_signature, elapsed, stats.duplicates_skipped
                    );
                    return Ok(models::ArbitrageResult {
                        opportunity: opportunity.clone(),
                        tx_hash: None,
                        status: models::ArbitrageStatus::Failed,
                        actual_profit: None,
                        actual_gas_used: None,
                        error_message: Some(format!("重复套利，{}秒前已执行", elapsed)),
                        executed_at: chrono::Utc::now(),
                    });
                }
            }
        }

        // 2. 检查相关池子是否正在执行
        {
            let executing = self.executing_pools.read().await;
            for pool in &path_pools {
                if executing.contains(pool) {
                    let mut stats = self.execution_stats.write().await;
                    stats.pool_busy_skipped += 1;
                    warn!(
                        "[{}] ⏭️ 跳过套利: 池子 {:?} 正在执行其他套利, 累计跳过={}",
                        self.chain_name, pool, stats.pool_busy_skipped
                    );
                    return Ok(models::ArbitrageResult {
                        opportunity: opportunity.clone(),
                        tx_hash: None,
                        status: models::ArbitrageStatus::Failed,
                        actual_profit: None,
                        actual_gas_used: None,
                        error_message: Some(format!("池子 {:?} 正在执行其他套利", pool)),
                        executed_at: chrono::Utc::now(),
                    });
                }
            }
        }

        // 3. 标记池子为正在执行
        {
            let mut executing = self.executing_pools.write().await;
            for pool in &path_pools {
                executing.insert(*pool);
            }
        }

        // 注意：后续代码需要确保在所有退出路径上清理 executing_pools

        // ========== 更新执行统计 ==========
        {
            let mut stats = self.execution_stats.write().await;
            stats.total_executions += 1;
        }

        // 检查是否为干运行模式
        if exec_config.dry_run {
            info!("[{}] 🔸 干运行模式: 跳过实际执行", self.chain_name);

            // 记录已执行（即使是干运行也要记录，防止重复）
            {
                let mut records = self.executed_opportunities.write().await;
                records.insert(path_signature.clone(), ExecutedRecord {
                    executed_at: std::time::Instant::now(),
                    block_number: opportunity.block_number,
                });
            }

            // 清理池子锁
            {
                let mut executing = self.executing_pools.write().await;
                for pool in &path_pools {
                    executing.remove(pool);
                }
            }

            let mut stats = self.execution_stats.write().await;
            stats.dry_run_executions += 1;

            return Ok(models::ArbitrageResult {
                opportunity: opportunity.clone(),
                tx_hash: None,
                status: models::ArbitrageStatus::Pending,
                actual_profit: None,
                actual_gas_used: None,
                error_message: Some("干运行模式".to_string()),
                executed_at: chrono::Utc::now(),
            });
        }

        // 获取钱包和私钥
        let (wallet, private_key_str) = {
            let w = self.wallet.read().await;
            let pk = self.private_key.read().await;
            match (&*w, &*pk) {
                (Some(wallet), Some(pk)) => (wallet.clone(), pk.clone()),
                _ => {
                    // 清理池子锁
                    let mut executing = self.executing_pools.write().await;
                    for pool in &path_pools {
                        executing.remove(pool);
                    }
                    error!("[{}] ❌ 无法执行: 钱包或私钥未配置", self.chain_name);
                    return Err(anyhow::anyhow!("钱包或私钥未配置"));
                }
            }
        };

        // 检查合约地址
        if exec_config.arbitrage_contract.is_none() {
            // 清理池子锁
            let mut executing = self.executing_pools.write().await;
            for pool in &path_pools {
                executing.remove(pool);
            }
            error!("[{}] ❌ 无法执行: 套利合约地址未配置", self.chain_name);
            return Err(anyhow::anyhow!("套利合约地址未配置"));
        }

        // 应用执行数量策略
        let start_token_config = {
            let configs = self.token_configs.read().await;
            configs.get(&opportunity.path.start_token).cloned()
        };

        if let Some(token_config) = start_token_config {
            let token_price = self.price_service.get_price_by_symbol(&token_config.price_symbol).await
                .unwrap_or(Decimal::ZERO);
            if token_price > Decimal::ZERO {
                let adjusted_amount = exec_config.amount_strategy.calculate_amount(
                    opportunity.input_amount,
                    token_config.decimals,
                    token_price,
                );

                if adjusted_amount != opportunity.input_amount {
                    info!(
                        "[{}] 📊 应用执行数量策略: {} -> {} (策略: {:?})",
                        self.chain_name,
                        opportunity.input_amount,
                        adjusted_amount,
                        exec_config.amount_strategy
                    );
                    opportunity.input_amount = adjusted_amount;
                }
            }
        }

        // ========== 使用闪电贷池选择器构建参数 ==========
        // 验证路径长度 (目前只支持三角套利)
        if opportunity.path.hops.len() != 3 {
            let mut executing = self.executing_pools.write().await;
            for pool in &path_pools {
                executing.remove(pool);
            }
            error!("[{}] ❌ 不支持的套利路径长度: {} (目前只支持3跳)", self.chain_name, opportunity.path.hops.len());
            return Ok(models::ArbitrageResult {
                opportunity: opportunity.clone(),
                tx_hash: None,
                status: models::ArbitrageStatus::Failed,
                actual_profit: None,
                actual_gas_used: None,
                error_message: Some(format!("不支持的套利路径长度: {}", opportunity.path.hops.len())),
                executed_at: chrono::Utc::now(),
            });
        }

        let hops = &opportunity.path.hops;
        let swap_pools: Vec<Address> = hops.iter().map(|h| h.pool_address).collect();

        // 计算 min_profit (将 USD 转换为 tokenA 的 wei 单位)
        let start_token = hops[0].token_in;
        let token_info = self.get_token_info(start_token).await;
        let min_profit_usd = self.get_dynamic_min_profit().await;
        let min_profit_wei = if token_info.price_usd > Decimal::ZERO {
            let token_amount = min_profit_usd / token_info.price_usd;
            let wei_amount = token_amount * Decimal::from(10u64.pow(token_info.decimals as u32));
            U256::from_dec_str(&wei_amount.floor().to_string()).unwrap_or(U256::zero())
        } else {
            U256::zero() // 价格未知时不设限制
        };
        info!(
            "[{}] 💰 最小利润阈值: ${} USD = {} {} (wei)",
            self.chain_name, min_profit_usd, min_profit_wei, token_info.symbol
        );

        // 使用闪电贷池选择器自动选择最优池
        let params_builder = ArbitrageParamsBuilder::new(self.provider.clone(), self.config.chain_id)
            .with_min_profit(min_profit_wei);

        let arb_params = match params_builder
            .build_manual(
                hops[0].token_in,   // token_a
                hops[0].token_out,  // token_b
                hops[1].token_out,  // token_c
                hops[0].fee,        // fee1
                hops[1].fee,        // fee2
                hops[2].fee,        // fee3
                opportunity.input_amount,
                swap_pools,
                opportunity.expected_profit_usd,
                opportunity.gas_cost_usd,
            )
            .await
        {
            Ok(p) => p,
            Err(e) => {
                let mut executing = self.executing_pools.write().await;
                for pool in &path_pools {
                    executing.remove(pool);
                }
                error!("[{}] ❌ 选择闪电贷池失败: {}", self.chain_name, e);
                return Ok(models::ArbitrageResult {
                    opportunity: opportunity.clone(),
                    tx_hash: None,
                    status: models::ArbitrageStatus::Failed,
                    actual_profit: None,
                    actual_gas_used: None,
                    error_message: Some(format!("选择闪电贷池失败: {}", e)),
                    executed_at: chrono::Utc::now(),
                });
            }
        };

        info!(
            "[{}] 🎯 闪电贷池自动选择: {:?}, 费率: {} ({:.4}%)",
            self.chain_name,
            arb_params.flash_pool,
            arb_params.flash_pool_fee,
            arb_params.flash_pool_fee as f64 / 10000.0
        );

        // 构建执行器配置
        // 根据配置决定发送模式:
        // - Both: 同时使用 Flashbots 和公开 mempool
        // - Flashbots: 仅使用 Flashbots
        // - Normal: 仅使用公开 mempool
        let send_mode = if exec_config.use_flashbots && exec_config.use_public_mempool {
            SendMode::Both
        } else if exec_config.use_flashbots {
            SendMode::Flashbots
        } else {
            SendMode::Normal
        };

        let executor_config = ExecutorConfig {
            contract_address: exec_config.arbitrage_contract.unwrap(),
            chain_id: self.config.chain_id,
            gas_strategy: GasStrategy {
                gas_price_multiplier: 1.2,
                max_gas_price_gwei: exec_config.max_gas_price_gwei,
                gas_limit_multiplier: 1.3,
                use_eip1559: true,
                priority_fee_gwei: exec_config.priority_fee_gwei,
                fixed_gas_limit: None, // 动态估算
            },
            confirmation_timeout_secs: 120,
            confirmations: 1,
            simulate_before_execute: exec_config.simulate_before_execute,
            private_key: Some(private_key_str.clone()),
            send_mode,
            flashbots_config: FlashbotsConfig {
                enabled: exec_config.use_flashbots,
                relay_url: exec_config.flashbots_rpc_url.clone().unwrap_or_default(),
                chain_id: self.config.chain_id,
                ..Default::default()
            },
        };

        // 创建带签名的 provider (SignerMiddleware)
        let signer = SignerMiddleware::new(self.provider.clone(), wallet);
        let signer = Arc::new(signer);

        // 创建执行器 (带 price_service 以正确显示代币价格)
        let executor = match RealExecutor::new(executor_config, signer) {
            Ok(e) => e.with_price_service(self.price_service.clone()),
            Err(e) => {
                let mut executing = self.executing_pools.write().await;
                for pool in &path_pools {
                    executing.remove(pool);
                }
                error!("[{}] ❌ 创建执行器失败: {}", self.chain_name, e);
                return Ok(models::ArbitrageResult {
                    opportunity: opportunity.clone(),
                    tx_hash: None,
                    status: models::ArbitrageStatus::Failed,
                    actual_profit: None,
                    actual_gas_used: None,
                    error_message: Some(format!("创建执行器失败: {}", e)),
                    executed_at: chrono::Utc::now(),
                });
            }
        };

        // ========== 执行前输出详细参数日志 ==========
        // 获取代币符号
        let token_configs = self.token_configs.read().await;
        let get_symbol = |addr: &Address| -> String {
            token_configs.get(addr)
                .map(|c| c.symbol.clone())
                .unwrap_or_else(|| format!("{:?}", addr)[0..10].to_string())
        };
        let get_decimals = |addr: &Address| -> u8 {
            token_configs.get(addr).map(|c| c.decimals).unwrap_or(18)
        };

        // 起始代币信息
        let start_token = &opportunity.path.start_token;
        let start_symbol = get_symbol(start_token);
        let start_decimals = get_decimals(start_token);
        let input_human = format_token_amount(opportunity.input_amount, start_decimals);
        let output_human = format_token_amount(opportunity.expected_output, start_decimals);
        let profit_human = format_token_amount(opportunity.expected_profit, start_decimals);

        // 构建详细路径描述
        let mut path_details = String::new();
        let mut total_fee_bps: u32 = 0; // 总费率 (基点)
        for (i, hop) in opportunity.path.hops.iter().enumerate() {
            let token_in_symbol = get_symbol(&hop.token_in);
            let token_out_symbol = get_symbol(&hop.token_out);
            let fee_percent = hop.fee as f64 / 10000.0; // 转换为百分比
            total_fee_bps += hop.fee;
            path_details.push_str(&format!(
                "\n║   Hop{}: {} -> {} | 池子: {:?} | 费率: {}% ({}) | DEX: {:?}",
                i + 1,
                token_in_symbol,
                token_out_symbol,
                hop.pool_address,
                fee_percent,
                hop.fee,
                hop.dex_type
            ));
        }
        let total_fee_percent = total_fee_bps as f64 / 10000.0;

        // 估算交易费用 (基于输入金额和费率)
        let estimated_swap_fee_usd = opportunity.expected_profit_usd * Decimal::from_f64_retain(total_fee_percent / 100.0).unwrap_or(Decimal::ZERO);

        // 获取当前 Gas 价格
        let current_gas_price = self.gas_price_cache.read().await
            .as_ref()
            .map(|c| c.price_wei)
            .unwrap_or(U256::zero());
        let gas_gwei = current_gas_price / U256::exp10(9);

        drop(token_configs); // 释放读锁

        info!(
            target: "arbitrage_execution",
            "\n\
╔════════════════════════════════════════════════════════════════════════════════╗\n\
║                         🚀 准备执行套利交易                                     ║\n\
╠════════════════════════════════════════════════════════════════════════════════╣\n\
║ 基本信息:\n\
║   机会ID: {}\n\
║   执行时间: {}\n\
║   当前区块: {}\n\
╠════════════════════════════════════════════════════════════════════════════════╣\n\
║ 闪电贷信息:\n\
║   借贷代币: {} ({:?})\n\
║   借贷金额: {} {} ({} wei)\n\
║   闪电贷池: {:?} (自动选择, 费率: {:.4}%)\n\
╠════════════════════════════════════════════════════════════════════════════════╣\n\
║ 套利路径 ({} 跳):{}\n\
╠════════════════════════════════════════════════════════════════════════════════╣\n\
║ 费率信息:\n\
║   各跳费率总计: {}% ({} bps)\n\
║   预估Swap手续费: ~${:.4}\n\
╠════════════════════════════════════════════════════════════════════════════════╣\n\
║ 资金明细:\n\
║   输入金额: {} {}\n\
║   预期输出: {} {}\n\
║   毛利润: {} {} (${:.4})\n\
║   Gas费用: ${:.4} (Gas估算: {}, Gas价格: {} Gwei)\n\
║   净利润: ${:.4}\n\
║   利润率: {:.4}%\n\
╠════════════════════════════════════════════════════════════════════════════════╣\n\
║ 执行配置:\n\
║   合约地址: {:?}\n\
║   最大Gas价格: {} Gwei\n\
║   当前Gas价格: {} Gwei\n\
║   使用Flashbots: {}\n\
║   使用公开Mempool: {}\n\
║   发送模式: {:?}\n\
║   执行前模拟: {}\n\
╚════════════════════════════════════════════════════════════════════════════════╝",
            opportunity.id,
            ::utils::utc_to_shanghai_str(opportunity.timestamp),
            opportunity.block_number,
            // 闪电贷信息
            start_symbol,
            start_token,
            input_human,
            start_symbol,
            opportunity.input_amount,
            arb_params.flash_pool,
            arb_params.flash_pool_fee as f64 / 10000.0,
            // 路径信息
            opportunity.path.hops.len(),
            path_details,
            // 费率信息
            total_fee_percent,
            total_fee_bps,
            estimated_swap_fee_usd,
            // 资金明细
            input_human,
            start_symbol,
            output_human,
            start_symbol,
            profit_human,
            start_symbol,
            opportunity.expected_profit_usd,
            opportunity.gas_cost_usd,
            opportunity.gas_estimate,
            gas_gwei,
            opportunity.net_profit_usd,
            opportunity.profit_percentage,
            // 执行配置
            exec_config.arbitrage_contract,
            exec_config.max_gas_price_gwei,
            gas_gwei,
            exec_config.use_flashbots,
            exec_config.use_public_mempool,
            send_mode,
            exec_config.simulate_before_execute,
        );

        // 保存合约地址用于后续异步获取余额
        let contract_address = exec_config.arbitrage_contract.unwrap();

        // 收集套利路径中涉及的所有代币 (用于后续异步获取余额)
        let mut token_addresses: Vec<Address> = vec![opportunity.path.start_token];
        for hop in &opportunity.path.hops {
            if !token_addresses.contains(&hop.token_in) {
                token_addresses.push(hop.token_in);
            }
            if !token_addresses.contains(&hop.token_out) {
                token_addresses.push(hop.token_out);
            }
        }

        // ========== 并行获取执行前余额 (不阻塞套利执行) ==========
        let provider_for_before = self.provider.clone();
        let price_service_for_before = self.price_service.clone();
        let token_configs_for_before = self.token_configs.read().await.clone();
        let token_addresses_clone = token_addresses.clone();
        let chain_name_clone = self.chain_name.clone();
        let rpc_stats_for_before = Some(self.rpc_stats.clone());

        // 启动异步任务获取执行前余额
        let balances_before_handle = tokio::spawn(async move {
            let balances = Self::get_balances_async(
                provider_for_before,
                price_service_for_before,
                &token_configs_for_before,
                contract_address,
                &token_addresses_clone,
                rpc_stats_for_before,
            ).await;
            info!(
                target: "arbitrage_execution",
                "[{}] 📊 套利前钱包余额: {:?}",
                chain_name_clone,
                balances.iter().map(|b| format!("{}: {}", b.symbol, b.balance)).collect::<Vec<_>>()
            );
            balances
        });

        // 直接执行套利，不等待余额获取完成
        let exec_result = executor.execute(arb_params.clone()).await;

        // ========== 执行完成后清理 ==========

        // 记录已执行 (无论成功失败都记录，防止短时间内重复尝试)
        {
            let mut records = self.executed_opportunities.write().await;
            records.insert(path_signature.clone(), ExecutedRecord {
                executed_at: std::time::Instant::now(),
                block_number: opportunity.block_number,
            });
        }

        // 清理池子锁
        {
            let mut executing = self.executing_pools.write().await;
            for pool in &path_pools {
                executing.remove(pool);
            }
        }

        // 定期清理过期记录 (简单策略：每次执行后检查)
        self.cleanup_executed_records().await;

        // 将执行结果转换为 ArbitrageResult
        let result: Result<models::ArbitrageResult> = match exec_result {
            Ok(res) => {
                let status = if res.success {
                    models::ArbitrageStatus::Confirmed
                } else {
                    models::ArbitrageStatus::Reverted
                };
                Ok(models::ArbitrageResult {
                    opportunity: opportunity.clone(),
                    tx_hash: Some(res.tx_hash),
                    status,
                    actual_profit: Some(res.profit),
                    actual_gas_used: Some(res.gas_used),
                    error_message: None,
                    executed_at: chrono::Utc::now(),
                })
            }
            Err(e) => {
                // 使用 RevertDecoder 解析详细错误信息
                let error_str = format!("{:?}", e);
                let decoded = RevertDecoder::decode_from_error_string(&error_str);

                // 打印详细错误日志
                error!(target: "arbitrage_execution", "[{}] ❌ 套利执行失败:\n{}", self.chain_name, decoded);

                Ok(models::ArbitrageResult {
                    opportunity: opportunity.clone(),
                    tx_hash: None,
                    status: models::ArbitrageStatus::Failed,
                    actual_profit: None,
                    actual_gas_used: None,
                    error_message: Some(decoded.message.clone()),
                    executed_at: chrono::Utc::now(),
                })
            }
        };

        // 处理执行结果
        match &result {
            Ok(exec_result) => {
                // 更新执行统计
                {
                    let mut stats = self.execution_stats.write().await;
                    match exec_result.status {
                        models::ArbitrageStatus::Confirmed => {
                            stats.successful_executions += 1;
                            stats.total_profit_usd += opportunity.net_profit_usd;
                        }
                        models::ArbitrageStatus::Failed | models::ArbitrageStatus::Reverted => {
                            stats.failed_executions += 1;
                        }
                        _ => {}
                    }
                }

                // 异步获取执行后余额并发送邮件通知 (不阻塞主流程)
                let provider = self.provider.clone();
                let price_service = self.price_service.clone();
                let token_configs = self.token_configs.read().await.clone();
                let chain_name = self.chain_name.clone();
                let opportunity_clone = opportunity.clone();
                let exec_result_clone = exec_result.clone();
                let rpc_stats_for_after = Some(self.rpc_stats.clone());

                tokio::spawn(async move {
                    // 等待执行前余额获取完成
                    let balances_before = balances_before_handle.await.unwrap_or_default();

                    // 获取执行后余额
                    let balances_after = Self::get_balances_async(
                        provider,
                        price_service,
                        &token_configs,
                        contract_address,
                        &token_addresses,
                        rpc_stats_for_after,
                    ).await;

                    info!(
                        target: "arbitrage_execution",
                        "[{}] 📊 套利后钱包余额: {:?}",
                        chain_name,
                        balances_after.iter().map(|b| format!("{}: {}", b.symbol, b.balance)).collect::<Vec<_>>()
                    );

                    // 计算盈亏
                    let total_before: Decimal = balances_before.iter().map(|b| b.usd_value).sum();
                    let total_after: Decimal = balances_after.iter().map(|b| b.usd_value).sum();
                    let pnl = total_after - total_before;
                    info!(
                        target: "arbitrage_execution",
                        "[{}] 💰 套利盈亏: 执行前=${:.4}, 执行后=${:.4}, 盈亏=${:.4}",
                        chain_name, total_before, total_after, pnl
                    );

                    // 发送邮件通知 (包含前后余额对比)
                    Self::send_email_with_comparison(
                        &chain_name,
                        &opportunity_clone,
                        &exec_result_clone,
                        balances_before,
                        balances_after,
                    ).await;
                });

            }
            Err(_) => {
                // 失败统计已在上面的 result 转换中处理
            }
        }

        // 返回结果
        result
    }

    /// 记录套利机会到专用日志文件
    async fn log_opportunity(
        &self,
        opp: &ArbitrageOpportunity,
        event: &SwapEvent,
        token_in: &TokenInfo,
        token_out: &TokenInfo,
        swap_usd: Decimal,
    ) {
        // 构建路径描述
        let mut path_desc = String::new();
        for (i, hop) in opp.path.hops.iter().enumerate() {
            let token_in_info = self.get_token_info(hop.token_in).await;
            let token_out_info = self.get_token_info(hop.token_out).await;
            if i > 0 {
                path_desc.push_str(" -> ");
            }
            path_desc.push_str(&format!("{}({})/{}", token_in_info.symbol, hop.fee, token_out_info.symbol));
        }

        // 获取起始代币信息
        let start_token_info = self.get_token_info(opp.path.start_token).await;

        // 使用 target 指定写入 opportunity.log
        tracing::info!(
            target: "arbitrage_opportunity",
            "\n\
            ╔══════════════════════════════════════════════════════════════════╗\n\
            ║                    💰 发现套利机会                                ║\n\
            ╠══════════════════════════════════════════════════════════════════╣\n\
            ║ 时间: {}\n\
            ║ 区块: {}\n\
            ║ 机会ID: {}\n\
            ╠══════════════════════════════════════════════════════════════════╣\n\
            ║ 触发事件:\n\
            ║   池子: {:?}\n\
            ║   交换: {} -> {} | ${:.2}\n\
            ╠══════════════════════════════════════════════════════════════════╣\n\
            ║ 套利路径: {}\n\
            ║ 路径详情: {}\n\
            ╠══════════════════════════════════════════════════════════════════╣\n\
            ║ 输入金额: {} {}\n\
            ║ 预期输出: {} {}\n\
            ║ 毛利润: {} {} (${:.4})\n\
            ║ Gas费用: ${:.4} (估算Gas: {})\n\
            ║ ✅ 净利润: ${:.4}\n\
            ║ 利润率: {:.4}%\n\
            ╚══════════════════════════════════════════════════════════════════╝",
            utc_to_shanghai_str(opp.timestamp),
            event.block_number,
            opp.id,
            event.pool_address,
            token_in.symbol,
            token_out.symbol,
            swap_usd,
            start_token_info.symbol,
            path_desc,
            format_token_amount(opp.input_amount, start_token_info.decimals),
            start_token_info.symbol,
            format_token_amount(opp.expected_output, start_token_info.decimals),
            start_token_info.symbol,
            format_token_amount(opp.expected_profit, start_token_info.decimals),
            start_token_info.symbol,
            opp.expected_profit_usd,
            opp.gas_cost_usd,
            opp.gas_estimate,
            opp.net_profit_usd,
            opp.profit_percentage,
        );
    }

    /// 处理新区块事件
    pub async fn handle_new_block(&self, event: NewBlockEvent) {
        // 更新当前区块号
        self.current_block.store(event.block_number, Ordering::Relaxed);

        // 更新 gas price 缓存 (从区块头获取，避免额外 RPC)
        if let Some(base_fee) = event.base_fee {
            let mut cache = self.gas_price_cache.write().await;
            *cache = Some(GasPriceCache {
                price_wei: base_fee,
                last_updated: std::time::Instant::now(),
            });
        }

        info!("新区块 #{}: base_fee={:?} gwei",
            event.block_number,
            event.base_fee.map(|f| f / U256::from(1_000_000_000))
        );

        // ========== 关键：每个新区块刷新所有池子状态 ==========
        // 这样本地计算时总是使用最新数据，无延迟
        if let Err(e) = self.refresh_all_pools().await {
            warn!("[{}] 刷新池子状态失败: {}", self.chain_name, e);
        }

        // 检查并切换 RPC 统计的分钟计数
        self.rpc_stats.maybe_rotate_minute();

        // 每 5 个区块 (约 1 分钟) 打印一次 RPC 统计
        if event.block_number % 5 == 0 {
            info!("\n{}", self.rpc_stats.get_summary());
        }

        // 清理过期的 tx_hash 记录 (超过 60 秒的)
        {
            let mut processed = self.processed_tx_hashes.write().await;
            let now = std::time::Instant::now();
            let before_count = processed.len();
            processed.retain(|_, timestamp| now.duration_since(*timestamp).as_secs() < 60);
            let cleaned = before_count - processed.len();
            if cleaned > 0 {
                debug!("[{}] 🧹 清理了 {} 条过期 tx_hash 记录, 当前缓存数={}",
                       self.chain_name, cleaned, processed.len());
            }
        }

        // 清理过期的执行记录 (超过 60 秒的)
        {
            let mut executed = self.executed_opportunities.write().await;
            let now = std::time::Instant::now();
            let before_count = executed.len();
            executed.retain(|_, record| now.duration_since(record.executed_at).as_secs() < 60);
            let cleaned = before_count - executed.len();
            if cleaned > 0 {
                debug!("[{}] 🧹 清理了 {} 条过期执行记录, 当前缓存数={}",
                       self.chain_name, cleaned, executed.len());
            }
        }
    }

    /// 检测涉及特定池子的套利机会 (使用静态路径映射)
    /// swap_usd: 触发交易的真实 USD 金额，用于本地估算
    async fn detect_arbitrage_for_pool(&self, pool_address: Address, swap_usd: Decimal) -> Option<ArbitrageOpportunity> {
        // 获取该池子触发时应检查的路径
        let paths = self.get_paths_for_pool(pool_address).await;

        // 如果没有配置路径映射，回退到旧的动态枚举方式
        if paths.is_empty() {
            debug!("池子 {:?} 没有配置路径映射，使用动态枚举", pool_address);
            return self.detect_arbitrage_for_pool_legacy(pool_address, swap_usd).await;
        }

        // 获取所有池子状态
        let all_pools: Vec<PoolState> = {
            let states = self.pool_states.read().await;
            states.values().cloned().collect()
        };

        // 获取代币符号用于日志
        let trigger_pool = all_pools.iter().find(|p| p.address == pool_address)?;
        let token0_info = self.get_token_info(trigger_pool.token0).await;
        let token1_info = self.get_token_info(trigger_pool.token1).await;

        info!(
            "🔎 开始检测套利机会 | 触发池={:?} | {}/{} | 预定义路径数={}",
            pool_address, token0_info.symbol, token1_info.symbol, paths.len()
        );

        let mut best_opportunity: Option<ArbitrageOpportunity> = None;
        let mut paths_checked = 0u32;
        let mut valid_paths = 0u32;

        // 按优先级遍历预定义的路径
        for path_config in &paths {
            paths_checked += 1;

            let token_a_info = self.get_token_info(path_config.token_a).await;
            let token_b_info = self.get_token_info(path_config.token_b).await;
            let token_c_info = self.get_token_info(path_config.token_c).await;

            info!(
                "   🔄 检查路径: {} | {} -> {} -> {} -> {}",
                path_config.path_name,
                token_a_info.symbol, token_b_info.symbol, token_c_info.symbol, token_a_info.symbol
            );

            // 检查该路径的套利机会（传递真实交易量）
            if let Some(opp) = self.check_static_path(
                path_config,
                &all_pools,
                swap_usd,
            ).await {
                valid_paths += 1;
                info!(
                    "   💰 路径 {} 发现机会: 净利润=${:.4}",
                    path_config.path_name, opp.net_profit_usd
                );
                if best_opportunity.as_ref().map_or(true, |b| opp.net_profit_usd > b.net_profit_usd) {
                    best_opportunity = Some(opp);
                }
            }
        }

        info!(
            "📊 套利检测完成 | 检查路径数={} | 有效路径数={} | 最佳机会={:?}",
            paths_checked, valid_paths,
            best_opportunity.as_ref().map(|o| format!("${:.2}", o.net_profit_usd))
        );

        best_opportunity
    }

    /// 检查静态定义的套利路径 (直接链上验证，按实际输出选择最优池子)
    async fn check_static_path(
        &self,
        path_config: &PoolPathConfig,
        all_pools: &[PoolState],
        swap_usd: Decimal,
    ) -> Option<ArbitrageOpportunity> {
        let token_a = path_config.token_a;
        let token_b = path_config.token_b;
        let token_c = path_config.token_c;

        // 获取代币信息
        let token_a_info = self.get_token_info(token_a).await;
        let token_b_info = self.get_token_info(token_b).await;
        let token_c_info = self.get_token_info(token_c).await;

        // 将 swap USD 转换为代币数量作为输入
        let input_amount = self.usd_to_token_amount(swap_usd, &token_a_info);
        if input_amount.is_zero() {
            return None;
        }

        // 检查是否需要跳过本地计算（大资金跨 Tick 时本地估算不准）
        let skip_local_calc = swap_usd >= self.config.skip_local_calc_threshold_usd;

        let (pool1, pool2, pool3) = if skip_local_calc {
            // ========== 大资金模式：直接用 RPC 选择池子 ==========
            info!(
                "      💰 大资金模式 (${:.0} >= ${}): 跳过本地计算，直接链上选择池子",
                swap_usd, self.config.skip_local_calc_threshold_usd
            );

            // 使用链上 RPC 报价选择最优池子
            let p1 = self.find_best_pool_by_output_rpc(all_pools, token_a, token_b, input_amount).await?;
            let quote1 = self.quote_exact_input(token_a, token_b, p1.fee, input_amount).await.ok()?;

            let p2 = self.find_best_pool_by_output_rpc(all_pools, token_b, token_c, quote1.amount_out).await?;
            let quote2 = self.quote_exact_input(token_b, token_c, p2.fee, quote1.amount_out).await.ok()?;

            let p3 = self.find_best_pool_by_output_rpc(all_pools, token_c, token_a, quote2.amount_out).await?;

            info!(
                "      池子选择(RPC): {} ({}bp) -> {} ({}bp) -> {} ({}bp)",
                token_a_info.symbol, p1.fee / 100,
                token_b_info.symbol, p2.fee / 100,
                token_c_info.symbol, p3.fee / 100
            );

            (p1, p2, p3)
        } else {
            // ========== 普通模式：使用本地计算选择池子和估算输出 (零 RPC) ==========

            // 查找 A->B 的最优池子 (本地计算)
            let p1 = self.find_best_pool_by_output_local(all_pools, token_a, token_b, input_amount)?;

            // 本地计算第一跳的输出
            let hop1_output = {
                let sqrt_price = p1.sqrt_price_x96?;
                let liquidity = p1.liquidity?;
                let zero_for_one = p1.token0 == token_a;
                self.calculate_amount_out_local(sqrt_price, liquidity, input_amount, zero_for_one, p1.fee)?
            };

            // 查找 B->C 的最优池子 (本地计算)
            let p2 = self.find_best_pool_by_output_local(all_pools, token_b, token_c, hop1_output)?;

            // 本地计算第二跳的输出
            let hop2_output = {
                let sqrt_price = p2.sqrt_price_x96?;
                let liquidity = p2.liquidity?;
                let zero_for_one = p2.token0 == token_b;
                self.calculate_amount_out_local(sqrt_price, liquidity, hop1_output, zero_for_one, p2.fee)?
            };

            // 查找 C->A 的最优池子 (本地计算)
            let p3 = self.find_best_pool_by_output_local(all_pools, token_c, token_a, hop2_output)?;

            info!(
                "      池子选择(本地): {} ({}bp) -> {} ({}bp) -> {} ({}bp)",
                token_a_info.symbol, p1.fee / 100,
                token_b_info.symbol, p2.fee / 100,
                token_c_info.symbol, p3.fee / 100
            );

            (p1, p2, p3)
        };

        // 检查总手续费
        let total_fee_rate = pool1.fee + pool2.fee + pool3.fee;
        if total_fee_rate > 10000 {
            info!(
                "      ⏭️ 跳过高手续费路径: {}bp + {}bp + {}bp = {}bp > 100bp",
                pool1.fee / 100, pool2.fee / 100, pool3.fee / 100, total_fee_rate / 100
            );
            return None;
        }

        // 注意：池子状态已在每个新区块时刷新，无需再次刷新

        // 使用链上 QuoterV2 精确验证（确保执行前的最终确认）
        info!("      🔗 调用链上 Quoter 验证...");
        let (optimal_input, sim_result) = match self.find_optimal_input(
            token_a, token_b, token_c, &pool1, &pool2, &pool3, swap_usd
        ).await {
            Some(result) => result,
            None => {
                // 亏损详情已在 find_optimal_input 中打印
                return None;
            }
        };
        info!(
            "      🎯 最优输入: {} ({}) | 预期输出: {} ({}) | 净利润=${:.4}",
            format_token_amount(optimal_input, token_a_info.decimals),
            token_a_info.symbol,
            format_token_amount(sim_result.amount_out, token_a_info.decimals),
            token_a_info.symbol,
            sim_result.net_profit_usd
        );

        // 使用动态利润门槛
        let dynamic_min_profit = self.get_dynamic_min_profit().await;
        if sim_result.net_profit_usd < dynamic_min_profit {
            info!(
                "      ⚠️ 利润不足动态阈值: ${:.4} < ${:.2}",
                sim_result.net_profit_usd, dynamic_min_profit
            );
            return None;
        }

        let profit = sim_result.amount_out.saturating_sub(optimal_input);
        let profit_usd = sim_result.net_profit_usd + sim_result.gas_cost_usd;

        // 构建套利机会
        let path = ArbitragePath {
            start_token: token_a,
            chain_id: self.config.chain_id,
            hops: vec![
                SwapHop {
                    pool_address: pool1.address,
                    dex_type: pool1.dex_type,
                    token_in: token_a,
                    token_out: token_b,
                    fee: pool1.fee,
                },
                SwapHop {
                    pool_address: pool2.address,
                    dex_type: pool2.dex_type,
                    token_in: token_b,
                    token_out: token_c,
                    fee: pool2.fee,
                },
                SwapHop {
                    pool_address: pool3.address,
                    dex_type: pool3.dex_type,
                    token_in: token_c,
                    token_out: token_a,
                    fee: pool3.fee,
                },
            ],
        };

        let profit_percentage = if optimal_input > U256::zero() {
            let input_dec = decimal_from_str(&optimal_input.to_string()).unwrap_or(Decimal::ONE);
            let profit_dec = decimal_from_str(&profit.to_string()).unwrap_or(Decimal::ZERO);
            (profit_dec / input_dec) * dec!(100)
        } else {
            Decimal::ZERO
        };

        info!(
            "      ✅ 发现套利机会: {} | 净利润=${:.2} | 利润率={:.4}%",
            path_config.path_name, sim_result.net_profit_usd, profit_percentage
        );

        Some(ArbitrageOpportunity {
            id: uuid::Uuid::new_v4().to_string(),
            path,
            input_amount: optimal_input,
            expected_output: sim_result.amount_out,
            expected_profit: profit,
            expected_profit_usd: profit_usd,
            gas_estimate: sim_result.total_gas_used,
            gas_cost_usd: sim_result.gas_cost_usd,
            net_profit_usd: sim_result.net_profit_usd,
            profit_percentage,
            timestamp: chrono::Utc::now(),
            block_number: self.current_block.load(Ordering::Relaxed),
        })
    }

    /// 查找代币对的最优池子 (手续费最低的) - 已废弃，保留备用
    #[allow(dead_code)]
    fn find_best_pool_for_pair(&self, pools: &[PoolState], token_in: Address, token_out: Address) -> Option<PoolState> {
        pools.iter()
            .filter(|p| {
                (p.token0 == token_in && p.token1 == token_out) ||
                (p.token0 == token_out && p.token1 == token_in)
            })
            .min_by_key(|p| p.fee)
            .cloned()
    }

    /// 查找代币对的最优池子 (使用本地计算，零 RPC)
    ///
    /// 使用本地缓存的 sqrt_price_x96 和 liquidity 估算输出
    /// 替代之前的链上 QuoterV2 报价，大幅减少 RPC 调用
    fn find_best_pool_by_output_local(
        &self,
        pools: &[PoolState],
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> Option<PoolState> {
        // 找到所有匹配的池子
        let matching_pools: Vec<&PoolState> = pools.iter()
            .filter(|p| {
                (p.token0 == token_in && p.token1 == token_out) ||
                (p.token0 == token_out && p.token1 == token_in)
            })
            .collect();

        if matching_pools.is_empty() {
            return None;
        }

        // 如果只有一个池子，直接返回
        if matching_pools.len() == 1 {
            return Some(matching_pools[0].clone());
        }

        // 使用本地计算估算每个池子的输出，找输出最多的
        let mut best_pool: Option<PoolState> = None;
        let mut best_output = U256::zero();

        for pool in matching_pools {
            // 检查池子是否有 V3 价格数据
            if !pool.has_v3_price_data() {
                continue;
            }

            let sqrt_price = match pool.sqrt_price_x96 {
                Some(p) => p,
                None => continue,
            };
            let liquidity = match pool.liquidity {
                Some(l) => l,
                None => continue,
            };

            // 确定交换方向
            let zero_for_one = pool.token0 == token_in;

            // 本地计算输出
            if let Some(output) = self.calculate_amount_out_local(
                sqrt_price,
                liquidity,
                amount_in,
                zero_for_one,
                pool.fee,
            ) {
                if output > best_output {
                    best_output = output;
                    best_pool = Some(pool.clone());
                }
            }
        }

        best_pool
    }

    /// 查找代币对的最优池子 (使用链上报价，用于大资金精确选择)
    async fn find_best_pool_by_output_rpc(
        &self,
        pools: &[PoolState],
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> Option<PoolState> {
        // 找到所有匹配的池子
        let matching_pools: Vec<&PoolState> = pools.iter()
            .filter(|p| {
                (p.token0 == token_in && p.token1 == token_out) ||
                (p.token0 == token_out && p.token1 == token_in)
            })
            .collect();

        if matching_pools.is_empty() {
            return None;
        }

        // 如果只有一个池子，直接返回
        if matching_pools.len() == 1 {
            return Some(matching_pools[0].clone());
        }

        // 对每个池子报价，找输出最多的
        let mut best_pool: Option<PoolState> = None;
        let mut best_output = U256::zero();

        for pool in matching_pools {
            match self.quote_exact_input(token_in, token_out, pool.fee, amount_in).await {
                Ok(result) if result.amount_out > best_output => {
                    best_output = result.amount_out;
                    best_pool = Some(pool.clone());
                }
                Ok(_) => {
                    // 输出不是最优，跳过
                }
                Err(e) => {
                    debug!("池子 {:?} 报价失败: {}", pool.address, e);
                }
            }
        }

        best_pool
    }

    /// 旧版动态枚举方式 (向后兼容)
    /// swap_usd: 触发交易的真实 USD 金额
    async fn detect_arbitrage_for_pool_legacy(&self, pool_address: Address, swap_usd: Decimal) -> Option<ArbitrageOpportunity> {
        // 先获取数据的拷贝，然后释放锁，避免死锁
        let (pool_clone, other_pools, pool_count) = {
            let states = self.pool_states.read().await;
            let pool = states.get(&pool_address)?.clone();
            let others: Vec<PoolState> = states.values()
                .filter(|p| p.address != pool_address)
                .cloned()
                .collect();
            let count = states.len();
            (pool, others, count)
        };
        // 读锁已释放

        // 获取代币符号用于日志
        let token0_info = self.get_token_info(pool_clone.token0).await;
        let token1_info = self.get_token_info(pool_clone.token1).await;

        info!(
            "🔎 [Legacy] 开始检测套利机会 | 触发池={:?} | {}/{} | 总池子数={}",
            pool_address, token0_info.symbol, token1_info.symbol, pool_count
        );

        // 收集所有可能的套利机会，最后返回利润最高的
        let mut best_opportunity: Option<ArbitrageOpportunity> = None;
        let mut paths_checked = 0u32;
        let mut valid_paths = 0u32;

        // 检查两个方向：
        // 方向1: token0 -> token1 -> tokenX -> token0
        // 方向2: token1 -> token0 -> tokenX -> token1
        for other_pool in &other_pools {
            // 方向1: A(token0) -> B(token1) -> C -> A
            paths_checked += 1;
            if let Some(opp) = self.check_triangular_path_directed(
                pool_clone.token0, pool_clone.token1, &pool_clone, other_pool, &other_pools, swap_usd
            ).await {
                valid_paths += 1;
                if best_opportunity.as_ref().map_or(true, |b| opp.net_profit_usd > b.net_profit_usd) {
                    best_opportunity = Some(opp);
                }
            }

            // 方向2: A(token1) -> B(token0) -> C -> A
            paths_checked += 1;
            if let Some(opp) = self.check_triangular_path_directed(
                pool_clone.token1, pool_clone.token0, &pool_clone, other_pool, &other_pools, swap_usd
            ).await {
                valid_paths += 1;
                if best_opportunity.as_ref().map_or(true, |b| opp.net_profit_usd > b.net_profit_usd) {
                    best_opportunity = Some(opp);
                }
            }
        }

        info!(
            "📊 [Legacy] 套利检测完成 | 检查路径数={} | 有效路径数={} | 最佳机会={:?}",
            paths_checked, valid_paths,
            best_opportunity.as_ref().map(|o| format!("${:.2}", o.net_profit_usd))
        );

        best_opportunity
    }

    /// 检查指定方向的三角套利路径 (V2版本，使用Vec避免死锁)
    /// token_a -> token_b (通过 pool1) -> token_c (通过 pool2) -> token_a (通过 pool3)
    /// swap_usd: 触发交易的真实 USD 金额
    async fn check_triangular_path_directed(
        &self,
        token_a: Address,
        token_b: Address,
        pool1: &PoolState,
        pool2: &PoolState,
        all_pools: &[PoolState],
        swap_usd: Decimal,
    ) -> Option<ArbitrageOpportunity> {
        // pool2 必须包含 token_b，找出 token_c
        let token_c = if pool2.token0 == token_b {
            pool2.token1
        } else if pool2.token1 == token_b {
            pool2.token0
        } else {
            return None;
        };

        // token_c 不能等于 token_a（否则就是两跳，不是三角）
        if token_c == token_a {
            return None;
        }

        // 🔥 核心过滤：检查该三角组合是否在配置中
        if !self.is_valid_triangle(token_a, token_b, token_c).await {
            // 只在 debug 级别记录，避免日志过多
            debug!(
                "   ⏭️ 跳过未配置的三角组合: {:?} -> {:?} -> {:?}",
                token_a, token_b, token_c
            );
            return None;
        }

        // 获取代币符号用于日志
        let token_a_info = self.get_token_info(token_a).await;
        let token_b_info = self.get_token_info(token_b).await;
        let token_c_info = self.get_token_info(token_c).await;

        // 找所有能完成 token_c -> token_a 的池子，选最优的
        let matching_pools: Vec<&PoolState> = all_pools.iter()
            .filter(|p| {
                p.address != pool1.address &&
                p.address != pool2.address &&
                ((p.token0 == token_c && p.token1 == token_a) ||
                 (p.token0 == token_a && p.token1 == token_c))
            })
            .collect();

        if matching_pools.is_empty() {
            info!(
                "   ❌ 无法完成三角路径: {} -> {} -> {} -> {} (无匹配的第三池)",
                token_a_info.symbol, token_b_info.symbol, token_c_info.symbol, token_a_info.symbol
            );
            return None;
        }

        info!(
            "   🔄 检查三角路径: {} -> {} -> {} -> {} | pool1={:?} pool2={:?} 候选pool3={}个",
            token_a_info.symbol, token_b_info.symbol, token_c_info.symbol, token_a_info.symbol,
            pool1.address, pool2.address, matching_pools.len()
        );

        // 对每个可能的 pool3，计算最优输入和利润
        let mut best_result: Option<(U256, ArbitrageSimResult, &PoolState)> = None;
        let current_block = self.current_block.load(Ordering::Relaxed);

        // 使用真实交易量进行本地估算
        let base_input = self.usd_to_token_amount(swap_usd, &token_a_info);
        debug!(
            "      📊 [Legacy] 本地估算输入: ${:.2} -> {} {}",
            swap_usd, format_token_amount(base_input, token_a_info.decimals), token_a_info.symbol
        );

        // 注意：池子状态已在每个新区块时刷新，无需再次刷新

        // 筛选候选 pool3（用手续费过滤）
        let mut candidate_pool3s: Vec<&PoolState> = Vec::new();

        debug!(
            "   pool2状态: last_block={}, current={}, has_price={}",
            pool2.last_block, current_block, pool2.has_v3_price_data()
        );

        for pool3 in &matching_pools {
            // 优化: 先用手续费过滤明显无利润的路径
            let total_fee_rate = pool1.fee + pool2.fee + pool3.fee;
            if total_fee_rate > 10000 {
                debug!(
                    "      ⏭️ 跳过高手续费路径: {}bp + {}bp + {}bp = {}bp > 100bp",
                    pool1.fee / 100, pool2.fee / 100, pool3.fee / 100, total_fee_rate / 100
                );
                continue;
            }

            // 直接加入候选池，由链上 QuoterV2 精确验证
            candidate_pool3s.push(*pool3);
        }

        if candidate_pool3s.is_empty() {
            info!("      ❌ 没有通过筛选的候选池");
            return None;
        }

        // 对候选池使用链上 QuoterV2 精确确认
        for pool3 in candidate_pool3s {
            if let Some((optimal_input, sim_result)) =
                self.find_optimal_input(token_a, token_b, token_c, pool1, pool2, pool3, swap_usd).await
            {
                info!(
                    "      📈 pool3={:?} | 最优输入={} | 输出={} | 净利润=${:.4} | gas=${:.4}",
                    pool3.address, optimal_input, sim_result.amount_out,
                    sim_result.net_profit_usd, sim_result.gas_cost_usd
                );
                if best_result.as_ref().map_or(true, |(_, r, _)| sim_result.net_profit_usd > r.net_profit_usd) {
                    best_result = Some((optimal_input, sim_result, pool3));
                }
            }
        }

        let (input_amount, sim_result, pool3) = best_result?;

        // 使用动态利润门槛
        let dynamic_min_profit = self.get_dynamic_min_profit().await;
        if sim_result.net_profit_usd < dynamic_min_profit {
            info!(
                "   ⚠️ 利润不足动态阈值: ${:.4} < ${:.2} (Gas动态门槛)",
                sim_result.net_profit_usd, dynamic_min_profit
            );
            return None;
        }

        let profit = sim_result.amount_out.saturating_sub(input_amount);
        let profit_usd = sim_result.net_profit_usd + sim_result.gas_cost_usd;

        // 构建套利机会
        let path = ArbitragePath {
            start_token: token_a,
            chain_id: self.config.chain_id,
            hops: vec![
                SwapHop {
                    pool_address: pool1.address,
                    dex_type: pool1.dex_type,
                    token_in: token_a,
                    token_out: token_b,
                    fee: pool1.fee,
                },
                SwapHop {
                    pool_address: pool2.address,
                    dex_type: pool2.dex_type,
                    token_in: token_b,
                    token_out: token_c,
                    fee: pool2.fee,
                },
                SwapHop {
                    pool_address: pool3.address,
                    dex_type: pool3.dex_type,
                    token_in: token_c,
                    token_out: token_a,
                    fee: pool3.fee,
                },
            ],
        };

        let profit_percentage = if input_amount > U256::zero() {
            let input_dec = decimal_from_str(&input_amount.to_string()).unwrap_or(Decimal::ONE);
            let profit_dec = decimal_from_str(&profit.to_string()).unwrap_or(Decimal::ZERO);
            (profit_dec / input_dec) * dec!(100)
        } else {
            Decimal::ZERO
        };

        info!(
            "发现套利机会: {:?} -> {:?} -> {:?} -> {:?}, 净利润=${:.2}, 利润率={:.4}%, gas={}",
            token_a, token_b, token_c, token_a, sim_result.net_profit_usd, profit_percentage, sim_result.total_gas_used
        );

        Some(ArbitrageOpportunity {
            id: uuid::Uuid::new_v4().to_string(),
            path,
            input_amount,
            expected_output: sim_result.amount_out,
            expected_profit: profit,
            expected_profit_usd: profit_usd,
            gas_estimate: sim_result.total_gas_used,
            gas_cost_usd: sim_result.gas_cost_usd,
            net_profit_usd: sim_result.net_profit_usd,
            profit_percentage,
            timestamp: chrono::Utc::now(),
            block_number: self.current_block.load(Ordering::Relaxed),
        })
    }


    /// 使用 swap 事件真实金额评估套利利润
    /// 返回 (输入金额, 模拟结果)
    async fn find_optimal_input(
        &self,
        token_a: Address,
        token_b: Address,
        token_c: Address,
        pool1: &PoolState,
        pool2: &PoolState,
        pool3: &PoolState,
        swap_usd: Decimal,
    ) -> Option<(U256, ArbitrageSimResult)> {
        // 获取代币信息，将 swap USD 金额转换为代币数量
        let token_a_info = self.get_token_info(token_a).await;
        let input_amount = self.usd_to_token_amount(swap_usd, &token_a_info);

        // 防止输入金额为 0
        if input_amount.is_zero() {
            info!("         ⚠️ 输入金额为0，跳过");
            return None;
        }

        info!(
            "         📊 使用 swap 真实金额: ${:.2} -> {} {}",
            swap_usd, format_token_amount(input_amount, token_a_info.decimals), token_a_info.symbol
        );

        // 直接用 swap 金额进行链上报价评估
        let result = self.simulate_and_calculate_profit(
            input_amount, token_a, token_b, token_c, pool1, pool2, pool3
        ).await;

        match result {
            Some(sim_result) if sim_result.net_profit_usd > Decimal::ZERO => {
                info!(
                    "         ✅ 有利润: 净利润=${:.4} | gas=${:.4}",
                    sim_result.net_profit_usd, sim_result.gas_cost_usd
                );
                Some((input_amount, sim_result))
            }
            Some(sim_result) => {
                info!(
                    "         ⚪ 无利润: 净利润=${:.4}",
                    sim_result.net_profit_usd
                );
                None
            }
            None => {
                // simulate_and_calculate_profit 已打印详细亏损日志，这里不重复
                None
            }
        }
    }

    /// 使用链上 QuoterV2 获取真实报价和 gas 估算，计算净利润
    async fn simulate_and_calculate_profit(
        &self,
        input_amount: U256,
        token_a: Address,
        token_b: Address,
        token_c: Address,
        pool1: &PoolState,
        pool2: &PoolState,
        pool3: &PoolState,
    ) -> Option<ArbitrageSimResult> {
        let mut total_gas_estimate = U256::zero();

        // 获取代币符号用于日志
        let token_a_info = self.get_token_info(token_a).await;
        let token_b_info = self.get_token_info(token_b).await;
        let token_c_info = self.get_token_info(token_c).await;

        // 开始计时 - 3次链上报价
        let quote_start = std::time::Instant::now();

        // Step 1: A -> B (真实报价 + gas 估算)
        let input_fmt = format_token_amount(input_amount, token_a_info.decimals);
        let quote1 = match self.quote_exact_input(token_a, token_b, pool1.fee, input_amount).await {
            Ok(result) => result,
            Err(e) => {
                info!("         ❌ Step1 报价失败: {} {} -> {} | 错误: {}", input_fmt, token_a_info.symbol, token_b_info.symbol, e);
                return None;
            }
        };
        let quote1_elapsed = quote_start.elapsed();
        if quote1.amount_out.is_zero() {
            info!("         ❌ Step1 输出为0: {} {} -> {} | fee={}bp", input_fmt, token_a_info.symbol, token_b_info.symbol, pool1.fee / 100);
            return None;
        }
        total_gas_estimate += quote1.gas_estimate;
        let out1_fmt = format_token_amount(quote1.amount_out, token_b_info.decimals);
        debug!(
            "         Step1: {} {} -> {} {} | fee={}bp | gas={} | RPC: {:.1}ms",
            input_fmt, token_a_info.symbol, out1_fmt, token_b_info.symbol,
            pool1.fee / 100, quote1.gas_estimate, quote1_elapsed.as_secs_f64() * 1000.0
        );

        // Step 2: B -> C (真实报价 + gas 估算)
        let quote2_start = std::time::Instant::now();
        let quote2 = match self.quote_exact_input(token_b, token_c, pool2.fee, quote1.amount_out).await {
            Ok(result) => result,
            Err(e) => {
                info!("         ❌ Step2 报价失败: {} {} -> {} | 错误: {}", out1_fmt, token_b_info.symbol, token_c_info.symbol, e);
                return None;
            }
        };
        let quote2_elapsed = quote2_start.elapsed();
        if quote2.amount_out.is_zero() {
            info!("         ❌ Step2 输出为0: {} {} -> {} | fee={}bp", out1_fmt, token_b_info.symbol, token_c_info.symbol, pool2.fee / 100);
            return None;
        }
        total_gas_estimate += quote2.gas_estimate;
        let out2_fmt = format_token_amount(quote2.amount_out, token_c_info.decimals);
        debug!(
            "         Step2: {} {} -> {} {} | fee={}bp | gas={} | RPC: {:.1}ms",
            out1_fmt, token_b_info.symbol, out2_fmt, token_c_info.symbol,
            pool2.fee / 100, quote2.gas_estimate, quote2_elapsed.as_secs_f64() * 1000.0
        );

        // Step 3: C -> A (真实报价 + gas 估算)
        let quote3_start = std::time::Instant::now();
        let quote3 = match self.quote_exact_input(token_c, token_a, pool3.fee, quote2.amount_out).await {
            Ok(result) => result,
            Err(e) => {
                info!("         ❌ Step3 报价失败: {} {} -> {} | 错误: {}", out2_fmt, token_c_info.symbol, token_a_info.symbol, e);
                return None;
            }
        };
        let quote3_elapsed = quote3_start.elapsed();
        let out3_fmt = format_token_amount(quote3.amount_out, token_a_info.decimals);
        if quote3.amount_out.is_zero() {
            info!("         ❌ Step3 输出为0: {} {} -> {} | fee={}bp", out2_fmt, token_c_info.symbol, token_a_info.symbol, pool3.fee / 100);
            return None;
        }
        if quote3.amount_out <= input_amount {
            let loss = input_amount - quote3.amount_out;
            let loss_usd = self.calculate_profit_usd(loss, token_a).await;
            info!(
                "         ❌ 亏损 ${:.2} | 输入: {} {} | 输出: {} {} | 路径: {}->{}->{}->{}",
                loss_usd,
                input_fmt, token_a_info.symbol,
                out3_fmt, token_a_info.symbol,
                token_a_info.symbol, token_b_info.symbol, token_c_info.symbol, token_a_info.symbol
            );
            return None;
        }
        total_gas_estimate += quote3.gas_estimate;

        // 总报价耗时
        let total_quote_elapsed = quote_start.elapsed();
        debug!(
            "         Step3: {} {} -> {} {} | fee={}bp | gas={} | RPC: {:.1}ms | 3次报价总耗时: {:.1}ms",
            out2_fmt, token_c_info.symbol, out3_fmt, token_a_info.symbol,
            pool3.fee / 100, quote3.gas_estimate, quote3_elapsed.as_secs_f64() * 1000.0,
            total_quote_elapsed.as_secs_f64() * 1000.0
        );

        // 添加额外开销 (闪电贷回调、合约调用等) 约 50,000 gas
        total_gas_estimate += U256::from(50_000);

        // 计算真实 gas 成本
        let gas_cost_usd = self.calculate_gas_cost_usd(total_gas_estimate).await;

        let profit = quote3.amount_out.saturating_sub(input_amount);
        let profit_usd = self.calculate_profit_usd(profit, token_a).await;
        let net_profit_usd = profit_usd - gas_cost_usd;

        info!(
            "         ✅ 套利模拟完成: 输入={} {} | 输出={} {} | 毛利润={} ({} ${:.4}) | gas={} (${:.4}) | 净利润=${:.4}",
            input_amount, token_a_info.symbol,
            quote3.amount_out, token_a_info.symbol,
            profit, token_a_info.symbol, profit_usd,
            total_gas_estimate, gas_cost_usd,
            net_profit_usd
        );

        Some(ArbitrageSimResult {
            net_profit_usd,
            amount_out: quote3.amount_out,
            total_gas_used: total_gas_estimate,
            gas_cost_usd,
        })
    }

    /// 获取缓存的 gas price (30秒更新一次，减少 RPC 调用)
    async fn get_cached_gas_price(&self) -> U256 {
        const CACHE_DURATION_SECS: u64 = 30;

        // 检查缓存是否有效
        {
            let cache = self.gas_price_cache.read().await;
            if let Some(ref cached) = *cache {
                if cached.last_updated.elapsed().as_secs() < CACHE_DURATION_SECS {
                    return cached.price_wei;
                }
            }
        }

        // 缓存过期或不存在，从链上获取
        let rpc_start = std::time::Instant::now();
        let gas_price_wei = match self.provider.get_gas_price().await {
            Ok(price) => {
                let rpc_elapsed = rpc_start.elapsed();
                // 记录成功的 RPC 调用
                self.rpc_stats.record_call(
                    RpcCallType::GetGasPrice,
                    rpc_elapsed.as_millis() as u64,
                    true
                );
                debug!("更新 gas price 缓存: {} gwei | RPC耗时: {:.2}ms",
                    price / U256::from(1_000_000_000u64),
                    rpc_elapsed.as_secs_f64() * 1000.0
                );
                price
            }
            Err(e) => {
                let rpc_elapsed = rpc_start.elapsed();
                // 记录失败的 RPC 调用
                self.rpc_stats.record_call(
                    RpcCallType::GetGasPrice,
                    rpc_elapsed.as_millis() as u64,
                    false
                );
                debug!("获取 gas price 失败: {}, 使用默认值 30 Gwei", e);
                U256::from(30_000_000_000u64) // 默认 30 Gwei
            }
        };

        // 更新缓存
        {
            let mut cache = self.gas_price_cache.write().await;
            *cache = Some(GasPriceCache {
                price_wei: gas_price_wei,
                last_updated: std::time::Instant::now(),
            });
        }

        gas_price_wei
    }

    /// 根据 gas 用量计算 USD 成本 (使用缓存的 gas price)
    async fn calculate_gas_cost_usd(&self, gas_used: U256) -> Decimal {
        // 从价格服务获取 ETH 价格
        let eth_price = self.price_service.get_eth_price().await;

        // 使用缓存的 gas price (30秒更新一次)
        let gas_price_wei = self.get_cached_gas_price().await;

        // gas_cost_eth = gas_used * gas_price_wei / 10^18
        let gas_cost_wei = gas_used * gas_price_wei;
        let gas_cost_eth = decimal_from_str(&gas_cost_wei.to_string())
            .unwrap_or(Decimal::ZERO) / dec!(1_000_000_000_000_000_000);

        gas_cost_eth * eth_price
    }

    /// 根据当前 Gas 价格获取动态最小利润门槛
    /// 低 gas 时使用较低门槛，高 gas 时使用较高门槛
    pub async fn get_dynamic_min_profit(&self) -> Decimal {
        // 如果未启用动态门槛，返回静态配置值
        if !self.config.enable_dynamic_profit {
            return self.config.min_profit_usd;
        }

        let gas_price_wei = self.get_cached_gas_price().await;
        let gas_price_gwei = gas_price_wei / U256::from(1_000_000_000u64);
        let gas_gwei_u64 = gas_price_gwei.as_u64();

        let config = &self.config.dynamic_profit_config;

        let min_profit = if gas_gwei_u64 < 1 {
            // 超低 gas (< 1 Gwei): $1
            config.ultra_low_gas_min_profit
        } else if gas_gwei_u64 < 5 {
            // 低 gas (1-5 Gwei): $3
            config.low_gas_min_profit
        } else if gas_gwei_u64 < 20 {
            // 正常 gas (5-20 Gwei): $10
            config.normal_gas_min_profit
        } else if gas_gwei_u64 < 50 {
            // 高 gas (20-50 Gwei): $30
            config.high_gas_min_profit
        } else {
            // 超高 gas (>= 50 Gwei): $80
            config.very_high_gas_min_profit
        };

        debug!("动态利润门槛: Gas={} Gwei -> 最小利润=${}", gas_gwei_u64, min_profit);
        min_profit
    }

    /// 获取最优输入金额 (优先从配置缓存获取，支持多链，已停用)
    #[allow(dead_code)]
    async fn get_optimal_input_async(&self, token: Address) -> U256 {
        // 首先尝试从配置缓存获取
        if let Some(config) = self.get_token_config(token).await {
            return config.optimal_input_amount;
        }

        // 回退到默认值
        self.get_optimal_input_default(token)
    }

    /// 获取默认最优输入金额 (同步版本，用于兼容)
    #[allow(dead_code)]
    fn get_optimal_input(&self, token: Address) -> U256 {
        self.get_optimal_input_default(token)
    }

    /// 默认最优输入金额
    fn get_optimal_input_default(&self, token: Address) -> U256 {
        // WETH
        let weth: Address = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".parse().unwrap_or_default();
        // DAI
        let dai: Address = "0x6B175474E89094C44Da98b954EedeAC495271d0F".parse().unwrap_or_default();
        // USDC
        let usdc: Address = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".parse().unwrap_or_default();
        // USDT
        let usdt: Address = "0xdAC17F958D2ee523a2206206994597C13D831ec7".parse().unwrap_or_default();
        // WBTC
        let wbtc: Address = "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599".parse().unwrap_or_default();

        if token == weth {
            U256::from(1) * U256::exp10(18) // 1 ETH
        } else if token == dai {
            U256::from(3000) * U256::exp10(18) // 3000 DAI
        } else if token == usdc {
            U256::from(3000) * U256::exp10(6) // 3000 USDC
        } else if token == usdt {
            U256::from(3000) * U256::exp10(6) // 3000 USDT
        } else if token == wbtc {
            U256::from(10000000) // 0.1 BTC (8 decimals)
        } else {
            U256::from(1000) * U256::exp10(18)
        }
    }

    /// 计算利润 (USD) - 使用价格服务获取代币价格
    async fn calculate_profit_usd(&self, profit: U256, token: Address) -> Decimal {
        let token_info = self.get_token_info(token).await;
        let profit_dec = decimal_from_str(&profit.to_string()).unwrap_or(Decimal::ZERO);
        let divisor = Decimal::from(10u64.pow(token_info.decimals as u32));
        (profit_dec / divisor) * token_info.price_usd
    }

    /// 将 USD 金额转换为代币数量
    fn usd_to_token_amount(&self, usd_amount: Decimal, token_info: &TokenInfo) -> U256 {
        if token_info.price_usd <= Decimal::ZERO {
            // 价格无效，使用默认值
            return U256::from(1000) * U256::exp10(token_info.decimals as usize);
        }

        // token_amount = usd_amount / price_usd
        let token_amount = usd_amount / token_info.price_usd;

        // 转换为带小数位的原始数量
        let multiplier = Decimal::from(10u64.pow(token_info.decimals as u32));
        let raw_amount = token_amount * multiplier;

        // 转换为 U256
        let raw_str = raw_amount.floor().to_string();
        U256::from_dec_str(&raw_str).unwrap_or(U256::zero())
    }

    /// 获取代币信息 (优先从配置缓存获取，然后从价格服务获取实时价格)
    async fn get_token_info(&self, address: Address) -> TokenInfo {
        // 从价格服务获取价格
        let price_usd = self.price_service.get_price_by_address(&address).await
            .unwrap_or(Decimal::ZERO);

        // 优先从配置缓存获取代币信息
        if let Some(config) = self.get_token_config(address).await {
            let final_price = if price_usd > Decimal::ZERO {
                price_usd
            } else if config.is_stable {
                dec!(1)
            } else if config.price_symbol == "ETH" {
                self.price_service.get_eth_price().await
            } else {
                price_usd
            };

            return TokenInfo {
                symbol: config.symbol,
                decimals: config.decimals,
                price_usd: final_price,
            };
        }

        // 回退到硬编码映射 (保持向后兼容)
        let addr_str = format!("{:?}", address).to_lowercase();

        match addr_str.as_str() {
            // WETH
            "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2" => TokenInfo {
                symbol: "WETH".to_string(),
                decimals: 18,
                price_usd: if price_usd > Decimal::ZERO { price_usd } else { self.price_service.get_eth_price().await },
            },
            // DAI
            "0x6b175474e89094c44da98b954eedeac495271d0f" => TokenInfo {
                symbol: "DAI".to_string(),
                decimals: 18,
                price_usd: if price_usd > Decimal::ZERO { price_usd } else { dec!(1) },
            },
            // USDC
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48" => TokenInfo {
                symbol: "USDC".to_string(),
                decimals: 6,
                price_usd: if price_usd > Decimal::ZERO { price_usd } else { dec!(1) },
            },
            // USDT
            "0xdac17f958d2ee523a2206206994597c13d831ec7" => TokenInfo {
                symbol: "USDT".to_string(),
                decimals: 6,
                price_usd: if price_usd > Decimal::ZERO { price_usd } else { dec!(1) },
            },
            // WBTC
            "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599" => TokenInfo {
                symbol: "WBTC".to_string(),
                decimals: 8,
                price_usd,
            },
            _ => TokenInfo {
                symbol: "???".to_string(),
                decimals: 18,
                price_usd,
            },
        }
    }

    /// 计算代币的美金价值
    fn calculate_usd_value(&self, amount: U256, token_info: &TokenInfo) -> Decimal {
        let amount_dec = decimal_from_str(&amount.to_string()).unwrap_or(Decimal::ZERO);
        let divisor = Decimal::from(10u64.pow(token_info.decimals as u32));
        (amount_dec / divisor) * token_info.price_usd
    }

    /// 异步获取余额 (静态方法，用于 tokio::spawn，不阻塞主套利流程)
    async fn get_balances_async(
        provider: Arc<M>,
        price_service: SharedPriceService,
        token_configs: &HashMap<Address, TokenConfig>,
        contract_address: Address,
        token_addresses: &[Address],
        rpc_stats: Option<Arc<RpcStats>>,
    ) -> Vec<WalletBalance> {
        let mut balances = Vec::new();
        for &token_addr in token_addresses {
            // 获取代币信息
            let token_info = if let Some(config) = token_configs.get(&token_addr) {
                let price_usd = price_service.get_price_by_address(&token_addr).await
                    .unwrap_or(Decimal::ZERO);
                let final_price = if price_usd > Decimal::ZERO {
                    price_usd
                } else if config.is_stable {
                    dec!(1)
                } else {
                    price_usd
                };
                TokenInfo {
                    symbol: config.symbol.clone(),
                    decimals: config.decimals,
                    price_usd: final_price,
                }
            } else {
                TokenInfo {
                    symbol: "???".to_string(),
                    decimals: 18,
                    price_usd: Decimal::ZERO,
                }
            };

            // 获取余额 (带 RPC 统计)
            let erc20 = IERC20Balance::new(token_addr, provider.clone());
            let rpc_start = std::time::Instant::now();
            match erc20.balance_of(contract_address).call().await {
                Ok(balance) => {
                    // 记录成功的 RPC 调用
                    if let Some(ref stats) = rpc_stats {
                        stats.record_call(
                            RpcCallType::Erc20BalanceOf,
                            rpc_start.elapsed().as_millis() as u64,
                            true
                        );
                    }
                    let balance_str = format_token_amount(balance, token_info.decimals);
                    let amount_dec = decimal_from_str(&balance.to_string()).unwrap_or(Decimal::ZERO);
                    let divisor = Decimal::from(10u64.pow(token_info.decimals as u32));
                    let usd_value = (amount_dec / divisor) * token_info.price_usd;
                    balances.push(WalletBalance {
                        symbol: token_info.symbol,
                        token_address: format!("{:?}", token_addr),
                        balance: balance_str,
                        usd_value,
                    });
                }
                Err(e) => {
                    // 记录失败的 RPC 调用
                    if let Some(ref stats) = rpc_stats {
                        stats.record_call(
                            RpcCallType::Erc20BalanceOf,
                            rpc_start.elapsed().as_millis() as u64,
                            false
                        );
                    }
                    warn!("Failed to get balance for token {:?}: {:?}", token_addr, e);
                    balances.push(WalletBalance {
                        symbol: token_info.symbol,
                        token_address: format!("{:?}", token_addr),
                        balance: "N/A".to_string(),
                        usd_value: Decimal::ZERO,
                    });
                }
            }
        }
        balances
    }

    /// 异步发送邮件 (静态方法，用于 tokio::spawn，包含前后余额对比)
    async fn send_email_with_comparison(
        chain_name: &str,
        opportunity: &ArbitrageOpportunity,
        exec_result: &models::ArbitrageResult,
        balances_before: Vec<WalletBalance>,
        balances_after: Vec<WalletBalance>,
    ) {
        // 获取邮件通知器
        let notifier = match get_email_notifier() {
            Some(n) => n,
            None => return, // 邮件通知未启用
        };

        // 构建路径描述
        let mut path_desc = String::new();
        for (i, hop) in opportunity.path.hops.iter().enumerate() {
            if i > 0 {
                path_desc.push_str(" -> ");
            }
            path_desc.push_str(&format!("{:?} -> {:?}", hop.token_in, hop.token_out));
        }

        // 构建执行信息
        let execution_info = ArbitrageExecutionInfo {
            chain_name: chain_name.to_string(),
            opportunity_id: opportunity.id.clone(),
            path_description: path_desc,
            input_token: format!("{:?}", opportunity.path.start_token),
            input_amount: format!("{}", opportunity.input_amount),
            expected_profit_usd: opportunity.net_profit_usd,
            actual_profit_usd: exec_result.actual_profit.map(|_| opportunity.net_profit_usd),
            gas_cost_usd: opportunity.gas_cost_usd,
            tx_hash: exec_result.tx_hash.map(|h| format!("{:?}", h)),
            status: format!("{:?}", exec_result.status),
            block_number: opportunity.block_number,
            error_message: exec_result.error_message.clone(),
        };

        // 发送邮件 (包含前后余额对比)
        if let Err(e) = notifier.send_arbitrage_notification(
            &execution_info,
            &balances_before,
            &balances_after,
        ).await {
            error!("Failed to send arbitrage email notification: {}", e);
        }
    }

    /// 启动事件监听循环 (支持并发处理)
    pub async fn start(
        self: Arc<Self>,
        mut swap_rx: broadcast::Receiver<SwapEvent>,
        mut block_rx: broadcast::Receiver<NewBlockEvent>,
    ) -> Result<()> {
        {
            let mut running = self.running.write().await;
            *running = true;
        }

        let max_concurrent = self.config.max_concurrent_handlers;
        info!(
            "[{}] 事件驱动套利扫描器启动, 监控 {} 个池子, 最大并发={}",
            self.chain_name,
            self.pool_count().await,
            max_concurrent
        );

        loop {
            let running = *self.running.read().await;
            if !running {
                break;
            }

            tokio::select! {
                // 处理 Swap 事件 (并发)
                Ok(swap_event) = swap_rx.recv() => {
                    let tx_hash = swap_event.tx_hash;

                    // 1. 基于 tx_hash 去重 - 检查是否已处理过
                    {
                        let mut processed = self.processed_tx_hashes.write().await;
                        if processed.contains_key(&tx_hash) {
                            // 已处理过，跳过
                            let mut stats = self.execution_stats.write().await;
                            stats.duplicates_skipped += 1;
                            debug!(
                                "[{}] ⏭️ 跳过重复 swap 事件, tx_hash={:?}, pool={:?}, 累计跳过={}",
                                self.chain_name, tx_hash, swap_event.pool_address, stats.duplicates_skipped
                            );
                            continue;
                        }
                        // 标记为已处理
                        processed.insert(tx_hash, std::time::Instant::now());
                    }

                    // 2. 获取信号量许可 (阻塞等待，不丢弃事件)
                    let permit = self.handler_semaphore.clone().acquire_owned().await;
                    match permit {
                        Ok(permit) => {
                            // 更新活跃处理数
                            {
                                let mut stats = self.execution_stats.write().await;
                                stats.active_handlers += 1;
                            }

                            // 克隆必要的引用
                            let scanner = self.clone();
                            let pool_address = swap_event.pool_address;

                            // 异步处理事件
                            tokio::spawn(async move {
                                let start_time = std::time::Instant::now();
                                debug!("[{}] 🔄 开始并发处理 swap 事件, pool={:?}, tx_hash={:?}",
                                       scanner.chain_name, pool_address, tx_hash);

                                // 处理 swap 事件
                                if let Some(opportunity) = scanner.handle_swap_event(swap_event).await {
                                    let mut opps = scanner.opportunities.write().await;
                                    opps.push(opportunity);
                                }

                                let elapsed = start_time.elapsed();
                                debug!("[{}] ✅ swap 事件处理完成, 耗时={:.2}ms, pool={:?}",
                                       scanner.chain_name, elapsed.as_secs_f64() * 1000.0, pool_address);

                                // 更新活跃处理数
                                {
                                    let mut stats = scanner.execution_stats.write().await;
                                    stats.active_handlers = stats.active_handlers.saturating_sub(1);
                                }

                                // 释放信号量许可 (permit 被 drop 时自动释放)
                                drop(permit);
                            });
                        }
                        Err(e) => {
                            error!("[{}] ❌ 获取信号量失败: {}", self.chain_name, e);
                        }
                    }
                }
                // 处理新区块事件 (同步，因为需要更新全局状态)
                Ok(block_event) = block_rx.recv() => {
                    self.handle_new_block(block_event).await;
                }
                // 超时（兜底）
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(
                    self.config.fallback_scan_interval_ms
                )) => {
                    debug!("兜底扫描触发");
                }
            }
        }

        info!("[{}] 事件驱动套利扫描器停止", self.chain_name);
        Ok(())
    }

    /// 停止扫描器
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
    }

    /// 获取并清空发现的机会
    pub async fn take_opportunities(&self) -> Vec<ArbitrageOpportunity> {
        let mut opportunities = self.opportunities.write().await;
        std::mem::take(&mut *opportunities)
    }
}

/// 辅助函数：从字符串解析 Decimal
fn decimal_from_str(s: &str) -> Option<Decimal> {
    Decimal::from_str(s).ok()
}

/// 代币信息
#[derive(Debug, Clone)]
struct TokenInfo {
    symbol: String,
    decimals: u8,
    price_usd: Decimal,
}

/// 格式化代币数量（带小数）
fn format_token_amount(amount: U256, decimals: u8) -> String {
    let amount_str = amount.to_string();
    let decimals = decimals as usize;

    if amount_str.len() <= decimals {
        let zeros = "0".repeat(decimals - amount_str.len());
        format!("0.{}{}", zeros, amount_str)
    } else {
        let (integer, decimal) = amount_str.split_at(amount_str.len() - decimals);
        // 只显示前4位小数
        let decimal_short = if decimal.len() > 4 { &decimal[..4] } else { decimal };
        format!("{}.{}", integer, decimal_short)
    }
}

/// 将 sqrtPriceX96 转换为人类可读的价格
/// price = (sqrtPriceX96 / 2^96)^2
/// 返回 token1/token0 的价格，考虑两个代币的精度差异
fn sqrt_price_x96_to_price(sqrt_price_x96: U256, decimals0: u8, decimals1: u8) -> f64 {
    // sqrtPriceX96 = sqrt(price) * 2^96
    // price = (sqrtPriceX96 / 2^96)^2
    let sqrt_price_f64 = sqrt_price_x96.as_u128() as f64 / (2_f64.powi(96));
    let price_raw = sqrt_price_f64 * sqrt_price_f64;

    // 调整精度: token1 的数量 / token0 的数量
    // 需要乘以 10^(decimals0 - decimals1) 来获得正确的价格
    let decimal_adjustment = 10_f64.powi(decimals0 as i32 - decimals1 as i32);
    price_raw * decimal_adjustment
}

/// 格式化流动性为可读格式
fn format_liquidity(liquidity: u128) -> String {
    if liquidity >= 1_000_000_000_000_000_000 {
        format!("{:.2}E", liquidity as f64 / 1e18)
    } else if liquidity >= 1_000_000_000_000_000 {
        format!("{:.2}P", liquidity as f64 / 1e15)
    } else if liquidity >= 1_000_000_000_000 {
        format!("{:.2}T", liquidity as f64 / 1e12)
    } else if liquidity >= 1_000_000_000 {
        format!("{:.2}B", liquidity as f64 / 1e9)
    } else if liquidity >= 1_000_000 {
        format!("{:.2}M", liquidity as f64 / 1e6)
    } else if liquidity >= 1_000 {
        format!("{:.2}K", liquidity as f64 / 1e3)
    } else {
        format!("{}", liquidity)
    }
}
