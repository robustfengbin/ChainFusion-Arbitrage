//! 套利执行调试信息模块
//!
//! 用于记录执行时的详细信息，包括：
//! - 币种信息和实时价格
//! - 套利路径详情
//! - 预期输出 vs 实际输出
//! - 滑点分析

use ethers::prelude::*;
use ethers::types::{Address, U256};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn, error};
use chrono::{DateTime, Utc};
use chrono_tz::Asia::Shanghai;

use crate::types::ArbitrageParams;
use crate::revert_decoder::{RevertDecoder, DecodedRevertError};

/// 执行快照 - 记录执行时刻的完整状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSnapshot {
    /// 快照时间
    pub timestamp: DateTime<Utc>,
    /// 区块号
    pub block_number: u64,
    /// 套利参数
    pub params: ArbitrageParamsSnapshot,
    /// 代币信息
    pub token_info: TokenInfoSnapshot,
    /// 池子状态
    pub pool_states: Vec<PoolStateSnapshot>,
    /// 预期输出
    pub expected: ExpectedOutput,
    /// 实际结果 (如果已执行)
    pub actual: Option<ActualResult>,
    /// 错误信息 (如果失败)
    pub error: Option<ErrorSnapshot>,
}

/// 套利参数快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageParamsSnapshot {
    pub flash_pool: String,
    pub flash_pool_fee_bps: u32,
    pub token_a: String,
    pub token_b: String,
    pub token_c: String,
    pub fee1_bps: u32,
    pub fee2_bps: u32,
    pub fee3_bps: u32,
    pub amount_in: String,
    pub amount_in_formatted: String,
    pub min_profit: String,
    pub estimated_flash_fee: String,
    /// Swap 路径中的池子地址
    pub swap_pools: Vec<SwapPoolInfo>,
}

/// Swap 池子信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapPoolInfo {
    /// 池子地址
    pub pool_address: String,
    /// 输入代币
    pub token_in: String,
    /// 输入代币符号
    pub token_in_symbol: String,
    /// 输入代币精度
    pub token_in_decimals: u8,
    /// 输出代币
    pub token_out: String,
    /// 输出代币符号
    pub token_out_symbol: String,
    /// 输出代币精度
    pub token_out_decimals: u8,
    /// 池子费率 (bps)
    pub fee_bps: u32,
    /// 跳数 (1, 2, 3)
    pub hop: u8,
    /// 池子当前价格 (token_out / token_in)
    pub pool_price: Option<Decimal>,
    /// 池子 sqrtPriceX96 (V3 池子)
    pub sqrt_price_x96: Option<String>,
}

/// 代币信息快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfoSnapshot {
    pub token_a: TokenDetail,
    pub token_b: TokenDetail,
    pub token_c: TokenDetail,
}

/// 代币详情
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenDetail {
    pub address: String,
    pub symbol: String,
    pub decimals: u8,
    pub price_usd: Decimal,
    pub price_source: String,
}

impl Default for TokenDetail {
    fn default() -> Self {
        Self {
            address: String::new(),
            symbol: "UNKNOWN".to_string(),
            decimals: 18,
            price_usd: Decimal::ZERO,
            price_source: "unknown".to_string(),
        }
    }
}

/// 池子状态快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolStateSnapshot {
    pub pool_address: String,
    pub pool_type: String,
    pub token0: String,
    pub token1: String,
    pub fee_bps: u32,
    /// 池子储备或流动性
    pub reserve0: Option<String>,
    pub reserve1: Option<String>,
    /// V3 池子的 sqrtPriceX96
    pub sqrt_price_x96: Option<String>,
    /// 池子当前价格 (token1/token0)
    pub price: Option<Decimal>,
    /// 是 swap 池还是闪电贷池
    pub role: PoolRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PoolRole {
    FlashLoan,
    SwapHop1,
    SwapHop2,
    SwapHop3,
}

/// 预期输出
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedOutput {
    /// 预期最终输出 (wei)
    pub final_output: String,
    /// 预期利润 (wei)
    pub profit: String,
    /// 预期利润 (USD)
    pub profit_usd: Decimal,
    /// 闪电贷需要归还的金额
    pub amount_owed: String,
    /// 各步骤预期输出
    pub step_outputs: Vec<StepOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepOutput {
    pub step: String,
    pub input_token: String,
    pub output_token: String,
    pub input_amount: String,
    pub expected_output: String,
    pub fee_bps: u32,
}

/// 实际结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActualResult {
    pub tx_hash: String,
    pub success: bool,
    pub gas_used: String,
    pub gas_price: String,
    pub actual_profit: Option<String>,
    pub block_number: u64,
}

/// 错误快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorSnapshot {
    pub error_type: String,
    pub message: String,
    pub raw_data: String,
    pub possible_causes: Vec<String>,
    pub suggestions: Vec<String>,
    pub is_retryable: bool,
    /// 价格变化分析
    pub price_change_analysis: Option<PriceChangeAnalysis>,
}

/// 价格变化分析
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceChangeAnalysis {
    /// 发现机会时的价格
    pub discovery_prices: Vec<PricePoint>,
    /// 执行时的价格
    pub execution_prices: Vec<PricePoint>,
    /// 价格变化百分比
    pub price_changes: Vec<PriceChange>,
    /// 是否因价格变化导致失败
    pub is_price_change_cause: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricePoint {
    pub token_pair: String,
    pub price: Decimal,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceChange {
    pub token_pair: String,
    pub change_percent: Decimal,
    pub direction: String,  // "up" or "down"
}

/// 执行调试器
pub struct ExecutionDebugger<M: Middleware> {
    provider: Arc<M>,
    #[allow(dead_code)]
    chain_id: u64,
}

impl<M: Middleware + 'static> ExecutionDebugger<M> {
    pub fn new(provider: Arc<M>, chain_id: u64) -> Self {
        Self { provider, chain_id }
    }

    /// 截断地址显示
    fn truncate_address(addr: &str) -> String {
        if addr.len() > 12 {
            format!("{}...{}", &addr[..8], &addr[addr.len()-4..])
        } else {
            addr.to_string()
        }
    }

    /// 创建执行快照
    pub async fn create_snapshot(
        &self,
        params: &ArbitrageParams,
        token_info: Option<TokenInfoSnapshot>,
    ) -> ExecutionSnapshot {
        let block_number = self.provider.get_block_number().await
            .map(|n| n.as_u64())
            .unwrap_or(0);

        // 计算闪电贷需要归还的金额
        let flash_fee = params.amount_in * U256::from(params.flash_pool_fee) / U256::from(1_000_000);
        let amount_owed = params.amount_in + flash_fee;

        // 获取代币信息
        let token_info_ref = token_info.as_ref();
        let symbol_a = token_info_ref.map(|t| t.token_a.symbol.clone()).unwrap_or_else(|| "?".to_string());
        let symbol_b = token_info_ref.map(|t| t.token_b.symbol.clone()).unwrap_or_else(|| "?".to_string());
        let symbol_c = token_info_ref.map(|t| t.token_c.symbol.clone()).unwrap_or_else(|| "?".to_string());
        let decimals_a = token_info_ref.map(|t| t.token_a.decimals).unwrap_or(18);
        let decimals_b = token_info_ref.map(|t| t.token_b.decimals).unwrap_or(18);
        let decimals_c = token_info_ref.map(|t| t.token_c.decimals).unwrap_or(18);

        // 构建 swap 池子信息并查询实时价格
        let mut swap_pools = Vec::new();
        let fees = [params.fee1, params.fee2, params.fee3];
        let tokens = [
            (params.token_a, &symbol_a, decimals_a, params.token_b, &symbol_b, decimals_b),
            (params.token_b, &symbol_b, decimals_b, params.token_c, &symbol_c, decimals_c),
            (params.token_c, &symbol_c, decimals_c, params.token_a, &symbol_a, decimals_a),
        ];

        for (i, ((token_in, sym_in, dec_in, token_out, sym_out, dec_out), fee)) in tokens.iter().zip(fees.iter()).enumerate() {
            let (pool_addr_str, pool_price, sqrt_price_str) = if i < params.swap_pools.len() {
                let pool_addr = params.swap_pools[i];
                let addr_str = format!("{:?}", pool_addr);
                // 查询池子实时价格
                let (price, sqrt_str) = self.get_pool_price_with_sqrt(pool_addr, *dec_in, *dec_out).await;
                (addr_str, price, sqrt_str)
            } else {
                ("未知".to_string(), None, None)
            };

            swap_pools.push(SwapPoolInfo {
                pool_address: pool_addr_str,
                token_in: format!("{:?}", token_in),
                token_in_symbol: (*sym_in).clone(),
                token_in_decimals: *dec_in,
                token_out: format!("{:?}", token_out),
                token_out_symbol: (*sym_out).clone(),
                token_out_decimals: *dec_out,
                fee_bps: *fee,
                hop: (i + 1) as u8,
                pool_price,
                sqrt_price_x96: sqrt_price_str,
            });
        }

        // 根据 token_a 的精度格式化输入金额
        let amount_in_formatted = format_wei(params.amount_in, decimals_a);

        ExecutionSnapshot {
            timestamp: Utc::now(),
            block_number,
            params: ArbitrageParamsSnapshot {
                flash_pool: format!("{:?}", params.flash_pool),
                flash_pool_fee_bps: params.flash_pool_fee,
                token_a: format!("{:?}", params.token_a),
                token_b: format!("{:?}", params.token_b),
                token_c: format!("{:?}", params.token_c),
                fee1_bps: params.fee1,
                fee2_bps: params.fee2,
                fee3_bps: params.fee3,
                amount_in: params.amount_in.to_string(),
                amount_in_formatted,
                min_profit: params.min_profit.to_string(),
                estimated_flash_fee: params.estimated_flash_fee.to_string(),
                swap_pools,
            },
            token_info: token_info.unwrap_or_else(|| TokenInfoSnapshot {
                token_a: TokenDetail::default(),
                token_b: TokenDetail::default(),
                token_c: TokenDetail::default(),
            }),
            pool_states: vec![],  // 需要从链上获取
            expected: ExpectedOutput {
                final_output: "0".to_string(),
                profit: params.min_profit.to_string(),
                profit_usd: params.estimated_profit_usd,
                amount_owed: amount_owed.to_string(),
                step_outputs: vec![],
            },
            actual: None,
            error: None,
        }
    }

    /// 查询池子价格 (返回价格和 sqrtPriceX96)
    async fn get_pool_price_with_sqrt(&self, pool_address: Address, decimals_in: u8, decimals_out: u8) -> (Option<Decimal>, Option<String>) {
        abigen!(
            IUniswapV3Pool,
            r#"[
                function slot0() external view returns (uint160 sqrtPriceX96, int24 tick, uint16 observationIndex, uint16 observationCardinality, uint16 observationCardinalityNext, uint8 feeProtocol, bool unlocked)
                function token0() external view returns (address)
                function token1() external view returns (address)
            ]"#
        );

        let pool = IUniswapV3Pool::new(pool_address, self.provider.clone());

        match pool.slot_0().call().await {
            Ok((sqrt_price_x96, _, _, _, _, _, _)) => {
                let sqrt_str = sqrt_price_x96.to_string();

                // 计算价格: price = (sqrtPriceX96 / 2^96)^2
                // 然后根据代币精度调整
                if let Some(price) = Self::calculate_price_from_sqrt(sqrt_price_x96, decimals_in, decimals_out) {
                    (Some(price), Some(sqrt_str))
                } else {
                    (None, Some(sqrt_str))
                }
            }
            Err(_) => {
                // 可能是 V2 池子，尝试 getReserves
                (None, None)
            }
        }
    }

    /// 从 sqrtPriceX96 计算价格
    fn calculate_price_from_sqrt(sqrt_price_x96: U256, decimals_in: u8, decimals_out: u8) -> Option<Decimal> {
        // price = (sqrtPriceX96 / 2^96)^2 * 10^(decimals_in - decimals_out)
        // 为了保持精度，我们用大数计算

        // sqrtPriceX96^2 / 2^192
        let sqrt_squared = sqrt_price_x96.saturating_mul(sqrt_price_x96);

        // 使用 Decimal 进行高精度计算
        let numerator = Decimal::from_u128(sqrt_squared.low_u128())?;
        let two_pow_192 = Decimal::from_u128(2u128.pow(64))? * Decimal::from_u128(2u128.pow(64))? * Decimal::from_u128(2u128.pow(64))?;

        let mut price = numerator / two_pow_192;

        // 调整精度差异
        let decimal_diff = decimals_in as i32 - decimals_out as i32;
        if decimal_diff > 0 {
            for _ in 0..decimal_diff {
                price = price * Decimal::from(10);
            }
        } else if decimal_diff < 0 {
            for _ in 0..(-decimal_diff) {
                price = price / Decimal::from(10);
            }
        }

        Some(price)
    }

    /// 记录错误并分析
    pub fn record_error(
        &self,
        snapshot: &mut ExecutionSnapshot,
        error: &str,
        discovery_prices: Option<Vec<PricePoint>>,
        execution_prices: Option<Vec<PricePoint>>,
    ) {
        let decoded = RevertDecoder::decode_from_error_string(error);

        // 分析价格变化
        let price_change_analysis = if let (Some(disc), Some(exec)) = (discovery_prices, execution_prices) {
            Some(Self::analyze_price_changes(&disc, &exec))
        } else {
            None
        };

        let analysis = decoded.analysis.as_ref();

        snapshot.error = Some(ErrorSnapshot {
            error_type: format!("{:?}", decoded.error_type),
            message: decoded.message.clone(),
            raw_data: decoded.raw_data.clone(),
            possible_causes: analysis
                .map(|a| a.possible_causes.clone())
                .unwrap_or_default(),
            suggestions: analysis
                .map(|a| a.suggestions.clone())
                .unwrap_or_default(),
            is_retryable: analysis.map(|a| a.is_retryable).unwrap_or(false),
            price_change_analysis,
        });

        // 打印详细的错误报告
        self.print_error_report(snapshot, &decoded);
    }

    /// 分析价格变化
    fn analyze_price_changes(
        discovery: &[PricePoint],
        execution: &[PricePoint],
    ) -> PriceChangeAnalysis {
        let mut changes = vec![];
        let mut is_significant_change = false;

        for disc_price in discovery {
            if let Some(exec_price) = execution.iter().find(|p| p.token_pair == disc_price.token_pair) {
                if disc_price.price > Decimal::ZERO {
                    let change_percent = ((exec_price.price - disc_price.price) / disc_price.price) * Decimal::from(100);
                    let direction = if change_percent >= Decimal::ZERO { "up" } else { "down" };

                    // 超过 0.1% 的变化视为显著
                    if change_percent.abs() > Decimal::from_f64(0.1).unwrap_or(Decimal::ZERO) {
                        is_significant_change = true;
                    }

                    changes.push(PriceChange {
                        token_pair: disc_price.token_pair.clone(),
                        change_percent,
                        direction: direction.to_string(),
                    });
                }
            }
        }

        PriceChangeAnalysis {
            discovery_prices: discovery.to_vec(),
            execution_prices: execution.to_vec(),
            price_changes: changes,
            is_price_change_cause: is_significant_change,
        }
    }

    /// 打印错误报告
    fn print_error_report(&self, snapshot: &ExecutionSnapshot, decoded: &DecodedRevertError) {
        // 转换为上海时间
        let shanghai_time = snapshot.timestamp.with_timezone(&Shanghai);
        let time_str = shanghai_time.format("%Y-%m-%d %H:%M:%S CST").to_string();

        error!("╔══════════════════════════════════════════════════════════════════════════════╗");
        error!("║                           🔴 套利执行失败详细报告                              ║");
        error!("╠══════════════════════════════════════════════════════════════════════════════╣");
        error!("║ 时间: {} (上海)", time_str);
        error!("║ 区块: #{}", snapshot.block_number);
        error!("╠══════════════════════════════════════════════════════════════════════════════╣");
        error!("║ 📋 错误信息:");
        error!("║    类型: {:?}", decoded.error_type);
        error!("║    消息: {}", decoded.message);
        error!("╠══════════════════════════════════════════════════════════════════════════════╣");
        error!("║ 🔄 套利路径详情:");
        error!("║");
        for pool_info in &snapshot.params.swap_pools {
            error!("║    ┌─ Hop {} ─────────────────────────────────────────────────────────────────┐", pool_info.hop);
            error!("║    │ {} ({}) -> {} ({})",
                pool_info.token_in_symbol,
                Self::truncate_address(&pool_info.token_in),
                pool_info.token_out_symbol,
                Self::truncate_address(&pool_info.token_out)
            );
            error!("║    │ 池子: {}", pool_info.pool_address);
            error!("║    │ 费率: {} ({:.4}%)", pool_info.fee_bps, pool_info.fee_bps as f64 / 10000.0);
            // 显示池子实时价格
            if let Some(ref price) = pool_info.pool_price {
                error!("║    │ 📊 实时价格: 1 {} = {} {}",
                    pool_info.token_in_symbol,
                    price,
                    pool_info.token_out_symbol
                );
            }
            if let Some(ref sqrt) = pool_info.sqrt_price_x96 {
                error!("║    │ sqrtPriceX96: {}", sqrt);
            }
            error!("║    └──────────────────────────────────────────────────────────────────────────┘");
        }
        error!("║");
        error!("║    路径概览: {} -> {} -> {} -> {}",
            snapshot.token_info.token_a.symbol,
            snapshot.token_info.token_b.symbol,
            snapshot.token_info.token_c.symbol,
            snapshot.token_info.token_a.symbol
        );
        error!("╠══════════════════════════════════════════════════════════════════════════════╣");
        error!("║ 💰 金额信息:");
        error!("║    输入金额: {} {} (wei: {})",
            snapshot.params.amount_in_formatted,
            snapshot.token_info.token_a.symbol,
            snapshot.params.amount_in
        );
        error!("║    最小利润: {} wei", snapshot.params.min_profit);
        error!("║    闪电贷费: {} wei ({:.4}%)",
            snapshot.params.estimated_flash_fee,
            snapshot.params.flash_pool_fee_bps as f64 / 10000.0
        );
        error!("║    预期利润: ${}", snapshot.expected.profit_usd);
        error!("╠══════════════════════════════════════════════════════════════════════════════╣");
        error!("║ 🏊 闪电贷池:");
        error!("║    地址: {}", snapshot.params.flash_pool);
        error!("║    费率: {} ({:.4}%)",
            snapshot.params.flash_pool_fee_bps,
            snapshot.params.flash_pool_fee_bps as f64 / 10000.0
        );
        error!("╠══════════════════════════════════════════════════════════════════════════════╣");

        // 代币信息
        error!("║ 🪙 代币信息 (USD 价格):");
        error!("║    Token A: {} (精度:{}) @ ${:.6}",
            snapshot.token_info.token_a.symbol,
            snapshot.token_info.token_a.decimals,
            snapshot.token_info.token_a.price_usd
        );
        error!("║    Token B: {} (精度:{}) @ ${:.6}",
            snapshot.token_info.token_b.symbol,
            snapshot.token_info.token_b.decimals,
            snapshot.token_info.token_b.price_usd
        );
        error!("║    Token C: {} (精度:{}) @ ${:.6}",
            snapshot.token_info.token_c.symbol,
            snapshot.token_info.token_c.decimals,
            snapshot.token_info.token_c.price_usd
        );

        // 价格变化分析
        if let Some(ref err) = snapshot.error {
            if let Some(ref price_analysis) = err.price_change_analysis {
                error!("╠══════════════════════════════════════════════════════════════════════════════╣");
                error!("║ 📊 价格变化分析:");
                for change in &price_analysis.price_changes {
                    let arrow = if change.direction == "up" { "↑" } else { "↓" };
                    error!("║    {}: {} {:.4}%", change.token_pair, arrow, change.change_percent);
                }
                if price_analysis.is_price_change_cause {
                    error!("║    ⚠️  价格变化可能是失败原因!");
                }
            }
        }

        error!("╠══════════════════════════════════════════════════════════════════════════════╣");

        if let Some(ref analysis) = decoded.analysis {
            error!("║ 🔍 可能原因:");
            for cause in &analysis.possible_causes {
                error!("║    • {}", cause);
            }
            error!("╠══════════════════════════════════════════════════════════════════════════════╣");
            error!("║ 💡 建议措施:");
            for suggestion in &analysis.suggestions {
                error!("║    • {}", suggestion);
            }
        }

        error!("╠══════════════════════════════════════════════════════════════════════════════╣");
        error!("║ 🔢 原始错误数据:");
        // 分行显示长数据
        let raw = &decoded.raw_data;
        if raw.len() > 70 {
            for chunk in raw.as_bytes().chunks(70) {
                error!("║    {}", String::from_utf8_lossy(chunk));
            }
        } else {
            error!("║    {}", raw);
        }
        error!("╚══════════════════════════════════════════════════════════════════════════════╝");
    }

    /// 查询池子实时价格
    pub async fn get_pool_price(&self, pool_address: Address) -> Option<Decimal> {
        // V3 池子查询 slot0
        abigen!(
            IUniswapV3Pool,
            r#"[
                function slot0() external view returns (uint160 sqrtPriceX96, int24 tick, uint16 observationIndex, uint16 observationCardinality, uint16 observationCardinalityNext, uint8 feeProtocol, bool unlocked)
                function token0() external view returns (address)
                function token1() external view returns (address)
            ]"#
        );

        let pool = IUniswapV3Pool::new(pool_address, self.provider.clone());

        match pool.slot_0().call().await {
            Ok((sqrt_price_x96, _, _, _, _, _, _)) => {
                // 将 sqrtPriceX96 转换为价格
                // price = (sqrtPriceX96 / 2^96)^2
                let sqrt_price = Decimal::from_u128(sqrt_price_x96.as_u128())?;
                let two_pow_96 = Decimal::from_u128(2u128.pow(96))?;
                let price_sqrt = sqrt_price / two_pow_96;
                Some(price_sqrt * price_sqrt)
            }
            Err(e) => {
                warn!("获取池子价格失败: {:?}", e);
                None
            }
        }
    }
}

/// 格式化 wei 为可读格式
fn format_wei(wei: U256, decimals: u8) -> String {
    let divisor = U256::exp10(decimals as usize);
    let whole = wei / divisor;
    let fraction = wei % divisor;

    if fraction.is_zero() {
        format!("{}", whole)
    } else {
        let frac_str = format!("{:0>width$}", fraction, width = decimals as usize);
        let trimmed = frac_str.trim_end_matches('0');
        if trimmed.is_empty() {
            format!("{}", whole)
        } else {
            format!("{}.{}", whole, trimmed)
        }
    }
}

/// 截断地址为短格式
fn truncate_addr(addr: &str) -> String {
    if addr.len() > 12 {
        format!("{}...{}", &addr[..8], &addr[addr.len()-4..])
    } else {
        addr.to_string()
    }
}

/// 获取代币精度 (常见代币)
fn get_token_decimals(token: Address) -> u8 {
    // USDC
    if token == "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".parse().unwrap_or_default() {
        return 6;
    }
    // USDT
    if token == "0xdAC17F958D2ee523a2206206994597C13D831ec7".parse().unwrap_or_default() {
        return 6;
    }
    // WBTC
    if token == "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599".parse().unwrap_or_default() {
        return 8;
    }
    // 默认 18 (WETH, DAI, 等大多数 ERC20)
    18
}

/// 简化的日志记录函数 - 在执行前调用
pub fn log_execution_start(params: &ArbitrageParams) {
    let token_a_str = format!("{:?}", params.token_a);
    let token_b_str = format!("{:?}", params.token_b);
    let token_c_str = format!("{:?}", params.token_c);

    // 获取 token_a 的精度
    let decimals_a = get_token_decimals(params.token_a);

    // 获取上海时间
    let now = Utc::now();
    let shanghai_time = now.with_timezone(&Shanghai);
    let time_str = shanghai_time.format("%Y-%m-%d %H:%M:%S").to_string();

    info!("┌──────────────────────────────────────────────────────────────────────────────┐");
    info!("│                          🚀 开始执行套利交易                                  │");
    info!("├──────────────────────────────────────────────────────────────────────────────┤");
    info!("│ ⏰ 时间: {} (上海)", time_str);
    info!("├──────────────────────────────────────────────────────────────────────────────┤");
    info!("│ 🔄 套利路径:");

    // Hop 1: A -> B
    let pool1 = params.swap_pools.get(0).map(|p| format!("{:?}", p)).unwrap_or_else(|| "未知".to_string());
    info!("│    Hop 1: {} -> {}", truncate_addr(&token_a_str), truncate_addr(&token_b_str));
    info!("│           池子: {} | 费率: {} bps", truncate_addr(&pool1), params.fee1);

    // Hop 2: B -> C
    let pool2 = params.swap_pools.get(1).map(|p| format!("{:?}", p)).unwrap_or_else(|| "未知".to_string());
    info!("│    Hop 2: {} -> {}", truncate_addr(&token_b_str), truncate_addr(&token_c_str));
    info!("│           池子: {} | 费率: {} bps", truncate_addr(&pool2), params.fee2);

    // Hop 3: C -> A
    let pool3 = params.swap_pools.get(2).map(|p| format!("{:?}", p)).unwrap_or_else(|| "未知".to_string());
    info!("│    Hop 3: {} -> {}", truncate_addr(&token_c_str), truncate_addr(&token_a_str));
    info!("│           池子: {} | 费率: {} bps", truncate_addr(&pool3), params.fee3);

    info!("├──────────────────────────────────────────────────────────────────────────────┤");
    info!("│ 💰 金额信息:");
    info!("│    输入金额: {} (wei: {})", format_wei(params.amount_in, decimals_a), params.amount_in);
    info!("│    最小利润: {} wei", params.min_profit);
    info!("│    预估利润: ${:.4}", params.estimated_profit_usd);
    info!("│    预估Gas: ${:.4}", params.estimated_gas_cost_usd);
    info!("├──────────────────────────────────────────────────────────────────────────────┤");
    info!("│ 🏊 闪电贷:");
    info!("│    池子: {}", params.flash_pool);
    info!("│    费率: {} ({:.4}%)", params.flash_pool_fee, params.flash_pool_fee as f64 / 10000.0);
    info!("│    预估费用: {} wei", params.estimated_flash_fee);
    info!("└──────────────────────────────────────────────────────────────────────────────┘");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_wei() {
        let wei = U256::from(1_500_000_000_000_000_000u64);
        assert_eq!(format_wei(wei, 18), "1.5");

        let wei2 = U256::from(1_000_000_000_000_000_000u64);
        assert_eq!(format_wei(wei2, 18), "1");
    }
}
