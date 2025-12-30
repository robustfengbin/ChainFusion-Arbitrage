//! 三角套利分析器 - 基于真实价格数据的精确计算

use anyhow::Result;
use chrono::{TimeZone, Utc};
use chrono_tz::Asia::Shanghai;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, debug};

use crate::config::BacktestConfig;
use crate::database::BacktestDatabase;
use crate::models::{
    ArbitrageOpportunity, ArbitrageStep, BacktestStatistics, PathStatistics, PoolConfig, PoolPathConfig,
    PriceSnapshot, SwapRecord, TriggerEventInfo,
};

/// 将 Unix 时间戳转换为上海时间字符串
fn timestamp_to_shanghai_str(timestamp: u64) -> String {
    if timestamp == 0 {
        return "N/A".to_string();
    }
    let dt = Utc.timestamp_opt(timestamp as i64, 0).single();
    match dt {
        Some(utc_time) => {
            let shanghai_time = utc_time.with_timezone(&Shanghai);
            shanghai_time.format("%Y-%m-%d %H:%M:%S").to_string()
        }
        None => "Invalid".to_string(),
    }
}

/// 获取代币的小数位数
fn get_token_decimals(symbol: &str) -> u8 {
    match symbol.to_uppercase().as_str() {
        "USDC" | "USDT" => 6,
        "WBTC" => 8,
        _ => 18, // WETH, DAI 等
    }
}

/// 从 sqrtPriceX96 计算价格
/// 返回 token1/token0 的价格（以 token0 为单位计价 token1）
fn sqrt_price_x96_to_price(sqrt_price_x96: &str, decimals0: u8, decimals1: u8) -> Option<f64> {
    let sqrt_price_big: u128 = sqrt_price_x96.parse().ok()?;

    // Q96 = 2^96
    let q96: f64 = (1u128 << 96) as f64;

    // sqrtPrice = sqrtPriceX96 / 2^96
    let sqrt_price = sqrt_price_big as f64 / q96;

    // price = sqrtPrice^2
    let raw_price = sqrt_price * sqrt_price;

    // 调整小数位数: token1/token0 需要调整 10^(decimals0 - decimals1)
    let decimal_adjustment = 10f64.powi(decimals0 as i32 - decimals1 as i32);

    Some(raw_price * decimal_adjustment)
}

/// 三角套利分析器
pub struct ArbitrageAnalyzer {
    db: Arc<BacktestDatabase>,
    config: BacktestConfig,
    pools: Vec<PoolConfig>,
    paths: Vec<PoolPathConfig>,
    /// 池子地址 -> 配置
    pool_map: HashMap<String, PoolConfig>,
    /// (token0, token1) -> 池子列表（按费率排序）
    token_pair_pools: HashMap<(String, String), Vec<PoolConfig>>,
    /// 代币地址 -> 符号
    token_address_to_symbol: HashMap<String, String>,
}

impl ArbitrageAnalyzer {
    /// 创建分析器
    pub fn new(
        config: BacktestConfig,
        db: Arc<BacktestDatabase>,
        pools: Vec<PoolConfig>,
        paths: Vec<PoolPathConfig>,
    ) -> Self {
        let pool_map: HashMap<String, PoolConfig> = pools
            .iter()
            .map(|p| (p.address.to_lowercase(), p.clone()))
            .collect();

        // 构建 token pair -> pools 映射
        let mut token_pair_pools: HashMap<(String, String), Vec<PoolConfig>> = HashMap::new();
        // 构建代币地址 -> 符号映射
        let mut token_address_to_symbol: HashMap<String, String> = HashMap::new();

        for pool in &pools {
            let token0 = pool.token0.to_lowercase();
            let token1 = pool.token1.to_lowercase();

            // 记录地址到符号的映射
            token_address_to_symbol.insert(token0.clone(), pool.token0_symbol.clone());
            token_address_to_symbol.insert(token1.clone(), pool.token1_symbol.clone());

            // 正向
            token_pair_pools
                .entry((token0.clone(), token1.clone()))
                .or_default()
                .push(pool.clone());

            // 反向
            token_pair_pools
                .entry((token1, token0))
                .or_default()
                .push(pool.clone());
        }

        // 按费率排序（低费率优先）
        for pools in token_pair_pools.values_mut() {
            pools.sort_by_key(|p| p.fee);
        }

        Self {
            db,
            config,
            pools,
            paths,
            pool_map,
            token_pair_pools,
            token_address_to_symbol,
        }
    }

    /// 使用真实价格数据分析套利机会
    pub async fn analyze(&self, start_block: u64, end_block: u64) -> Result<BacktestStatistics> {
        info!("开始基于真实价格的精确套利分析...");
        info!("分析范围: 区块 {} - {}", start_block, end_block);
        info!("路径数量: {}", self.paths.len());
        info!("池子数量: {}", self.pools.len());

        // 获取完整的 Swap 数据
        let swaps = self.db.get_full_swaps_in_range(
            self.config.chain_id as i64,
            start_block,
            end_block,
        ).await?;

        info!("共加载 {} 条 Swap 记录", swaps.len());

        // 按区块分组 Swap 数据
        let mut block_swaps: HashMap<u64, Vec<SwapRecord>> = HashMap::new();
        for swap in swaps {
            block_swaps.entry(swap.block_number).or_default().push(swap);
        }

        let blocks_with_swaps = block_swaps.len() as u64;
        info!("有交易的区块数: {}", blocks_with_swaps);

        // 统计总交易量
        let total_volume: f64 = block_swaps.values()
            .flat_map(|swaps| swaps.iter().map(|s| s.usd_volume))
            .sum();
        info!("总交易量: ${:.2}", total_volume);

        // 初始化路径统计
        let mut path_stats_map: HashMap<String, PathStatistics> = HashMap::new();
        for path in &self.paths {
            path_stats_map.insert(path.path_name.clone(), PathStatistics {
                path_name: path.path_name.clone(),
                triangle_name: path.triangle_name.clone(),
                analysis_count: 0,
                profitable_count: 0,
                max_profit_usd: f64::NEG_INFINITY,
                avg_profit_usd: 0.0,
                total_profit_usd: 0.0,
            });
        }

        let mut all_opportunities: Vec<ArbitrageOpportunity> = Vec::new();

        // 创建进度条
        let pb = ProgressBar::new(blocks_with_swaps);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} 分析区块...")
                .unwrap()
                .progress_chars("#>-"),
        );

        // 遍历每个有交易的区块
        for (block_number, swaps) in &block_swaps {
            // 构建该区块的价格快照
            let mut block_prices: HashMap<String, PriceSnapshot> = HashMap::new();
            let mut block_volumes: HashMap<String, f64> = HashMap::new();
            let mut block_timestamp = 0u64;

            for swap in swaps {
                let pool_addr = swap.pool_address.to_lowercase();
                block_prices.insert(pool_addr.clone(), PriceSnapshot {
                    sqrt_price_x96: swap.sqrt_price_x96.clone(),
                    tick: swap.tick,
                    liquidity: swap.liquidity.clone(),
                    block_number: swap.block_number,
                });
                *block_volumes.entry(pool_addr).or_default() += swap.usd_volume;
                block_timestamp = swap.block_timestamp;
            }

            // 分析每条路径
            for path in &self.paths {
                let trigger_pool_addr = path.trigger_pool.to_lowercase();
                let trigger_vol = block_volumes.get(&trigger_pool_addr).copied().unwrap_or(0.0);

                if trigger_vol < 100.0 {
                    continue;
                }

                // 获取触发池子的配置信息
                let trigger_pool_config = self.pool_map.get(&trigger_pool_addr);

                // 获取触发事件的详细信息
                let trigger_swaps: Vec<&SwapRecord> = swaps.iter()
                    .filter(|s| s.pool_address.to_lowercase() == trigger_pool_addr)
                    .collect();

                // 分析用户的交易方向
                // amount > 0 表示池子收到（用户卖出），amount < 0 表示池子支出（用户买入）
                let (swap_direction, user_sells_token0) = if let Some(swap) = trigger_swaps.first() {
                    if let Some(pool_cfg) = trigger_pool_config {
                        let amount0: i128 = swap.amount0.parse().unwrap_or(0);

                        if amount0 > 0 {
                            // 用户用 token0 换 token1（卖出 token0，买入 token1）
                            // token1 在该池变贵
                            (format!("{} -> {}", pool_cfg.token0_symbol, pool_cfg.token1_symbol), true)
                        } else {
                            // 用户用 token1 换 token0（卖出 token1，买入 token0）
                            // token0 在该池变贵
                            (format!("{} -> {}", pool_cfg.token1_symbol, pool_cfg.token0_symbol), false)
                        }
                    } else {
                        ("Unknown".to_string(), true)
                    }
                } else {
                    ("Unknown".to_string(), true)
                };

                // 解析用户买卖的代币
                let (user_sell_token, user_buy_token, price_impact) = if let Some(pool_cfg) = trigger_pool_config {
                    if user_sells_token0 {
                        (
                            pool_cfg.token0_symbol.clone(),
                            pool_cfg.token1_symbol.clone(),
                            format!("{} 价格上涨（变贵）", pool_cfg.token1_symbol),
                        )
                    } else {
                        (
                            pool_cfg.token1_symbol.clone(),
                            pool_cfg.token0_symbol.clone(),
                            format!("{} 价格上涨（变贵）", pool_cfg.token0_symbol),
                        )
                    }
                } else {
                    ("Unknown".to_string(), "Unknown".to_string(), "Unknown".to_string())
                };

                // 构建触发事件信息
                let trigger_event = trigger_pool_config.map(|pool_cfg| {
                    TriggerEventInfo {
                        pool_address: trigger_pool_addr.clone(),
                        pool_name: format!("{}/{}", pool_cfg.token0_symbol, pool_cfg.token1_symbol),
                        pool_fee_percent: pool_cfg.fee_percent(),
                        pool_volume_usd: trigger_vol,
                        swap_direction: swap_direction.clone(),
                        user_sell_token: user_sell_token.clone(),
                        user_buy_token: user_buy_token.clone(),
                        price_impact: price_impact.clone(),
                    }
                });

                // 使用真实三跳计算
                if let Some(result) = self.calculate_real_triangle_arbitrage(
                    path,
                    &block_prices,
                    trigger_vol,
                ) {
                    let (_final_output, _gross_profit, _steps) = result;

                    // 计算 Gas 成本
                    let gas_price_gwei = 10.0;
                    let gas_cost = self.config.gas_cost_usd(gas_price_gwei);

                    // 对不同捕获比例创建机会
                    for capture_pct in &self.config.capture_percentages {
                        let capture_ratio = *capture_pct as f64 / 100.0;
                        let input_amount = trigger_vol * capture_ratio;

                        if input_amount < 100.0 {
                            continue;
                        }

                        // 重新计算该捕获比例下的利润和步骤
                        let scaled_result = self.calculate_real_triangle_arbitrage(
                            path,
                            &block_prices,
                            input_amount,
                        );

                        let (scaled_output, scaled_gross_profit, arb_steps) = if let Some((out, profit, steps)) = scaled_result {
                            (out, profit, steps)
                        } else {
                            continue;
                        };

                        let net_profit = scaled_gross_profit - gas_cost;
                        let is_profitable = net_profit > 0.0;

                        // 计算价格偏离和套利空间
                        let total_fee_percent: f64 = arb_steps.iter().map(|s| s.fee_percent).sum();
                        let price_deviation_percent = if input_amount > 0.0 {
                            (scaled_gross_profit + input_amount * total_fee_percent / 100.0) / input_amount * 100.0
                        } else {
                            0.0
                        };
                        let arb_spread_percent = if input_amount > 0.0 {
                            scaled_gross_profit / input_amount * 100.0
                        } else {
                            0.0
                        };

                        // 计算闪电贷费用 (Uniswap V3 闪电贷费用 = 池子费率)
                        // 假设从第一跳的池子借入，费率就是 fee1
                        let flash_loan_fee_percent = arb_steps.first()
                            .map(|s| s.fee_percent)
                            .unwrap_or(0.05); // 默认 0.05%
                        let flash_loan_fee_usd = input_amount * flash_loan_fee_percent / 100.0;
                        let real_net_profit_usd = scaled_gross_profit - gas_cost - flash_loan_fee_usd;

                        let opp = ArbitrageOpportunity {
                            block_number: *block_number,
                            block_timestamp,
                            datetime_shanghai: timestamp_to_shanghai_str(block_timestamp),
                            path_name: path.path_name.clone(),
                            triangle_name: path.triangle_name.clone(),
                            real_volume_usd: trigger_vol,
                            capture_percent: *capture_pct,
                            input_amount_usd: input_amount,
                            output_amount_usd: scaled_output,
                            gross_profit_usd: scaled_gross_profit,
                            gas_cost_usd: gas_cost,
                            net_profit_usd: net_profit,
                            is_profitable,
                            trigger_event: trigger_event.clone(),
                            arb_steps,
                            price_deviation_percent,
                            total_fee_percent,
                            arb_spread_percent,
                            flash_loan_fee_usd,
                            flash_loan_fee_percent,
                            real_net_profit_usd,
                        };

                        // 更新路径统计
                        if let Some(stats) = path_stats_map.get_mut(&path.path_name) {
                            stats.analysis_count += 1;
                            stats.total_profit_usd += net_profit;
                            if net_profit > stats.max_profit_usd {
                                stats.max_profit_usd = net_profit;
                            }
                            if is_profitable {
                                stats.profitable_count += 1;
                            }
                        }

                        all_opportunities.push(opp);
                    }
                }
            }

            pb.inc(1);
        }

        pb.finish_with_message("分析完成");

        // 计算平均利润
        for stats in path_stats_map.values_mut() {
            if stats.analysis_count > 0 {
                stats.avg_profit_usd = stats.total_profit_usd / stats.analysis_count as f64;
            }
            if stats.max_profit_usd == f64::NEG_INFINITY {
                stats.max_profit_usd = 0.0;
            }
        }

        // 筛选盈利机会
        let profitable_opportunities: Vec<_> = all_opportunities
            .iter()
            .filter(|o| o.is_profitable)
            .cloned()
            .collect();

        info!("总分析机会数: {}", all_opportunities.len());
        info!("盈利机会数: {}", profitable_opportunities.len());

        let stats = BacktestStatistics {
            start_block,
            end_block,
            start_timestamp: 0,
            end_timestamp: 0,
            total_blocks: end_block - start_block,
            blocks_with_swaps,
            total_volume_usd: total_volume,
            path_stats: path_stats_map.into_values().collect(),
            profitable_opportunities,
        };

        Ok(stats)
    }

    /// 真实的三跳套利计算 - 追踪实际代币数量
    /// 使用配置表中的 pool1/pool2/pool3 指定的池子
    /// 返回 (最终输出USD金额, 毛利润USD, 套利步骤详情)
    fn calculate_real_triangle_arbitrage(
        &self,
        path: &PoolPathConfig,
        block_prices: &HashMap<String, PriceSnapshot>,
        input_amount_usd: f64,
    ) -> Option<(f64, f64, Vec<ArbitrageStep>)> {
        // token_a/b/c 是地址
        let token_a = path.token_a.to_lowercase();
        let token_b = path.token_b.to_lowercase();
        let token_c = path.token_c.to_lowercase();

        // 从地址获取符号
        let token_a_symbol = self.token_address_to_symbol.get(&token_a)?.clone();
        let token_b_symbol = self.token_address_to_symbol.get(&token_b)?.clone();
        let token_c_symbol = self.token_address_to_symbol.get(&token_c)?.clone();

        // 获取配置的池子地址
        let pool1_addr = path.pool1.to_lowercase();
        let pool2_addr = path.pool2.to_lowercase();
        let pool3_addr = path.pool3.to_lowercase();

        // 检查池子地址是否配置
        if pool1_addr.is_empty() || pool2_addr.is_empty() || pool3_addr.is_empty() {
            debug!("路径 {} 缺少池子配置", path.path_name);
            return None;
        }

        // 获取池子配置
        let pool1_cfg = self.pool_map.get(&pool1_addr)?;
        let pool2_cfg = self.pool_map.get(&pool2_addr)?;
        let pool3_cfg = self.pool_map.get(&pool3_addr)?;

        // 将输入 USD 转换为 token_a 数量
        let token_a_usd_price = match self.get_token_usd_price(&token_a, &token_a_symbol, block_prices) {
            Some(p) => p,
            None => {
                debug!("无法获取 {} 的 USD 价格", token_a_symbol);
                return None;
            }
        };
        let input_token_a_amount = input_amount_usd / token_a_usd_price;

        // 第一跳: token_a -> token_b (使用配置的 pool1)
        let (output1_token_b, fee1) = match self.swap_with_pool(
            &token_a, &token_b, input_token_a_amount, &pool1_addr, block_prices
        ) {
            Some(r) => r,
            None => {
                debug!("第一跳失败: {} -> {} (pool={})", token_a_symbol, token_b_symbol, pool1_addr);
                return None;
            }
        };

        // 第二跳: token_b -> token_c (使用配置的 pool2)
        let (output2_token_c, fee2) = match self.swap_with_pool(
            &token_b, &token_c, output1_token_b, &pool2_addr, block_prices
        ) {
            Some(r) => r,
            None => {
                debug!("第二跳失败: {} -> {} (pool={})", token_b_symbol, token_c_symbol, pool2_addr);
                return None;
            }
        };

        // 第三跳: token_c -> token_a (使用配置的 pool3)
        let (output3_token_a, fee3) = match self.swap_with_pool(
            &token_c, &token_a, output2_token_c, &pool3_addr, block_prices
        ) {
            Some(r) => r,
            None => {
                debug!("第三跳失败: {} -> {} (pool={})", token_c_symbol, token_a_symbol, pool3_addr);
                return None;
            }
        };

        // 计算利润 (以 token_a 计)
        let profit_token_a = output3_token_a - input_token_a_amount;
        let gross_profit_usd = profit_token_a * token_a_usd_price;
        let final_output_usd = output3_token_a * token_a_usd_price;

        // 构建套利步骤详情
        let steps = vec![
            ArbitrageStep {
                step: 1,
                pool_address: pool1_addr.clone(),
                pool_name: format!("{}/{}", pool1_cfg.token0_symbol, pool1_cfg.token1_symbol),
                fee_percent: fee1 * 100.0,
                sell_token: token_a_symbol.clone(),
                sell_amount: input_token_a_amount,
                buy_token: token_b_symbol.clone(),
                buy_amount: output1_token_b,
                description: format!(
                    "卖出 {:.4} {} 换取 {:.4} {}",
                    input_token_a_amount, token_a_symbol, output1_token_b, token_b_symbol
                ),
            },
            ArbitrageStep {
                step: 2,
                pool_address: pool2_addr.clone(),
                pool_name: format!("{}/{}", pool2_cfg.token0_symbol, pool2_cfg.token1_symbol),
                fee_percent: fee2 * 100.0,
                sell_token: token_b_symbol.clone(),
                sell_amount: output1_token_b,
                buy_token: token_c_symbol.clone(),
                buy_amount: output2_token_c,
                description: format!(
                    "卖出 {:.4} {} 换取 {:.4} {}",
                    output1_token_b, token_b_symbol, output2_token_c, token_c_symbol
                ),
            },
            ArbitrageStep {
                step: 3,
                pool_address: pool3_addr.clone(),
                pool_name: format!("{}/{}", pool3_cfg.token0_symbol, pool3_cfg.token1_symbol),
                fee_percent: fee3 * 100.0,
                sell_token: token_c_symbol.clone(),
                sell_amount: output2_token_c,
                buy_token: token_a_symbol.clone(),
                buy_amount: output3_token_a,
                description: format!(
                    "卖出 {:.4} {} 换取 {:.4} {}",
                    output2_token_c, token_c_symbol, output3_token_a, token_a_symbol
                ),
            },
        ];

        debug!("路径 {}: 输入={:.6} {}, 输出={:.6} {}, 利润={:.6} {} (${:.2})",
               path.path_name, input_token_a_amount, token_a_symbol,
               output3_token_a, token_a_symbol, profit_token_a, token_a_symbol, gross_profit_usd);

        Some((final_output_usd, gross_profit_usd, steps))
    }

    /// 使用指定池子执行交换，返回 (输出数量, 费率)
    fn swap_with_pool(
        &self,
        token_in: &str,
        _token_out: &str,
        amount_in: f64,
        pool_addr: &str,
        block_prices: &HashMap<String, PriceSnapshot>,
    ) -> Option<(f64, f64)> {
        // 获取池子配置
        let pool = self.pool_map.get(pool_addr)?;

        // 获取该区块的价格
        let price_snapshot = block_prices.get(pool_addr)?;

        let dec0 = get_token_decimals(&pool.token0_symbol);
        let dec1 = get_token_decimals(&pool.token1_symbol);
        let price_token1_per_token0 = sqrt_price_x96_to_price(&price_snapshot.sqrt_price_x96, dec0, dec1)?;

        let token0 = pool.token0.to_lowercase();
        let zero_for_one = token0 == token_in;

        let fee_rate = pool.fee as f64 / 1_000_000.0;
        let amount_after_fee = amount_in * (1.0 - fee_rate);

        let output = if zero_for_one {
            // token0 -> token1: output = input * price
            amount_after_fee * price_token1_per_token0
        } else {
            // token1 -> token0: output = input / price
            if price_token1_per_token0 > 0.0 {
                amount_after_fee / price_token1_per_token0
            } else {
                return None;
            }
        };

        Some((output, fee_rate))
    }

    /// 获取代币的 USD 价格（通过稳定币池子）
    fn get_token_usd_price(
        &self,
        token_address: &str,
        token_symbol: &str,
        block_prices: &HashMap<String, PriceSnapshot>,
    ) -> Option<f64> {
        let symbol_upper = token_symbol.to_uppercase();

        // 稳定币直接返回 1.0
        if matches!(symbol_upper.as_str(), "USDC" | "USDT" | "DAI") {
            return Some(1.0);
        }

        // 对于非稳定币，查找与稳定币配对的池子
        let stablecoins = ["USDC", "USDT", "DAI"];

        for stable in &stablecoins {
            let stable_lower = stable.to_lowercase();

            // 尝试找 token/stable 或 stable/token 的池子
            if let Some(pools) = self.token_pair_pools.get(&(token_address.to_string(), stable_lower.clone())) {
                for pool in pools {
                    let pool_addr = pool.address.to_lowercase();
                    if let Some(price_snapshot) = block_prices.get(&pool_addr) {
                        let dec0 = get_token_decimals(&pool.token0_symbol);
                        let dec1 = get_token_decimals(&pool.token1_symbol);

                        if let Some(price) = sqrt_price_x96_to_price(&price_snapshot.sqrt_price_x96, dec0, dec1) {
                            // price = token1/token0
                            let token0_upper = pool.token0_symbol.to_uppercase();

                            if token0_upper == symbol_upper {
                                // token 是 token0, stable 是 token1
                                // price = stable/token, 所以 1 token = price USD
                                return Some(price);
                            } else {
                                // stable 是 token0, token 是 token1
                                // price = token/stable, 所以 1 token = 1/price USD
                                if price > 0.0 {
                                    return Some(1.0 / price);
                                }
                            }
                        }
                    }
                }
            }
        }

        // 如果找不到稳定币配对，尝试通过 WETH 中转
        if symbol_upper != "WETH" {
            if let Some(weth_usd_price) = self.get_token_usd_price(
                &self.get_token_address("WETH")?,
                "WETH",
                block_prices,
            ) {
                // 找 token/WETH 池子
                let weth_addr = self.get_token_address("WETH")?;
                if let Some(pools) = self.token_pair_pools.get(&(token_address.to_string(), weth_addr.clone())) {
                    for pool in pools {
                        let pool_addr = pool.address.to_lowercase();
                        if let Some(price_snapshot) = block_prices.get(&pool_addr) {
                            let dec0 = get_token_decimals(&pool.token0_symbol);
                            let dec1 = get_token_decimals(&pool.token1_symbol);

                            if let Some(price) = sqrt_price_x96_to_price(&price_snapshot.sqrt_price_x96, dec0, dec1) {
                                let token0_upper = pool.token0_symbol.to_uppercase();

                                if token0_upper == symbol_upper {
                                    // token/WETH: price = WETH/token, 1 token = price WETH = price * weth_usd_price USD
                                    return Some(price * weth_usd_price);
                                } else {
                                    // WETH/token: price = token/WETH, 1 token = (1/price) WETH
                                    if price > 0.0 {
                                        return Some(weth_usd_price / price);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// 获取代币地址
    fn get_token_address(&self, symbol: &str) -> Option<String> {
        for pool in &self.pools {
            if pool.token0_symbol.to_uppercase() == symbol.to_uppercase() {
                return Some(pool.token0.to_lowercase());
            }
            if pool.token1_symbol.to_uppercase() == symbol.to_uppercase() {
                return Some(pool.token1.to_lowercase());
            }
        }
        None
    }

    /// 使用简化模型分析（向后兼容）
    pub async fn analyze_simple(&self, start_block: u64, end_block: u64) -> Result<BacktestStatistics> {
        info!("使用简化模型分析...");

        // 获取简化的 Swap 数据
        let swaps = self.db.get_swaps_in_range(
            self.config.chain_id as i64,
            start_block,
            end_block,
        ).await?;

        info!("共有 {} 条 Swap 记录", swaps.len());

        // 按区块分组
        let mut block_volumes: HashMap<u64, HashMap<String, f64>> = HashMap::new();
        let mut block_timestamps: HashMap<u64, u64> = HashMap::new();

        for (block_number, block_timestamp, pool_address, usd_volume) in &swaps {
            let pool_volumes = block_volumes.entry(*block_number).or_default();
            *pool_volumes.entry(pool_address.to_lowercase()).or_default() += usd_volume;
            block_timestamps.entry(*block_number).or_insert(*block_timestamp);
        }

        let blocks_with_swaps = block_volumes.len() as u64;
        let total_volume: f64 = block_volumes.values()
            .flat_map(|pv| pv.values())
            .sum();

        info!("有交易的区块数: {}", blocks_with_swaps);
        info!("总交易量: ${:.2}", total_volume);

        // 初始化路径统计
        let mut path_stats_map: HashMap<String, PathStatistics> = HashMap::new();
        for path in &self.paths {
            path_stats_map.insert(path.path_name.clone(), PathStatistics {
                path_name: path.path_name.clone(),
                triangle_name: path.triangle_name.clone(),
                analysis_count: 0,
                profitable_count: 0,
                max_profit_usd: f64::NEG_INFINITY,
                avg_profit_usd: 0.0,
                total_profit_usd: 0.0,
            });
        }

        let mut all_opportunities: Vec<ArbitrageOpportunity> = Vec::new();

        let pb = ProgressBar::new(block_volumes.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} 分析中...")
                .unwrap()
                .progress_chars("#>-"),
        );

        for (block_number, pool_volumes) in &block_volumes {
            for path in &self.paths {
                let trigger_vol = pool_volumes.get(&path.trigger_pool.to_lowercase()).copied().unwrap_or(0.0);

                if trigger_vol < 100.0 {
                    continue;
                }

                let trigger_pool_config = match self.pool_map.get(&path.trigger_pool.to_lowercase()) {
                    Some(c) => c,
                    None => continue,
                };

                // 计算真实的总手续费
                let token_a = path.token_a.to_lowercase();
                let token_b = path.token_b.to_lowercase();
                let token_c = path.token_c.to_lowercase();

                let fee1 = self.get_lowest_fee(&token_a, &token_b)
                    .unwrap_or(trigger_pool_config.fee_percent());
                let fee2 = self.get_lowest_fee(&token_b, &token_c)
                    .unwrap_or(trigger_pool_config.fee_percent());
                let fee3 = self.get_lowest_fee(&token_c, &token_a)
                    .unwrap_or(trigger_pool_config.fee_percent());

                let total_fee_pct = fee1 + fee2 + fee3;

                // 保守的价格偏差估计
                let price_diff_pct = 0.01; // 1个基点

                for capture_pct in &self.config.capture_percentages {
                    let input_amount = trigger_vol * (*capture_pct as f64 / 100.0);

                    if input_amount < 100.0 {
                        continue;
                    }

                    let gross_profit_pct = price_diff_pct - total_fee_pct;
                    let gross_profit = input_amount * gross_profit_pct / 100.0;

                    let gas_price_gwei = 10.0;
                    let gas_cost = self.config.gas_cost_usd(gas_price_gwei);

                    let net_profit = gross_profit - gas_cost;
                    let is_profitable = net_profit > 0.0;

                    let block_ts = block_timestamps.get(block_number).copied().unwrap_or(0);

                    // 简化模式下也构建触发事件信息
                    let trigger_event = Some(TriggerEventInfo {
                        pool_address: path.trigger_pool.to_lowercase(),
                        pool_name: format!("{}/{}", trigger_pool_config.token0_symbol, trigger_pool_config.token1_symbol),
                        pool_fee_percent: trigger_pool_config.fee_percent(),
                        pool_volume_usd: trigger_vol,
                        swap_direction: "Unknown".to_string(), // 简化模式没有详细方向
                        user_sell_token: "Unknown".to_string(),
                        user_buy_token: "Unknown".to_string(),
                        price_impact: "Unknown".to_string(),
                    });

                    let flash_loan_fee_percent = fee1; // 假设从第一个池子借
                    let flash_loan_fee_usd = input_amount * flash_loan_fee_percent / 100.0;
                    let real_net_profit_usd = gross_profit - gas_cost - flash_loan_fee_usd;

                    let opp = ArbitrageOpportunity {
                        block_number: *block_number,
                        block_timestamp: block_ts,
                        datetime_shanghai: timestamp_to_shanghai_str(block_ts),
                        path_name: path.path_name.clone(),
                        triangle_name: path.triangle_name.clone(),
                        real_volume_usd: trigger_vol,
                        capture_percent: *capture_pct,
                        input_amount_usd: input_amount,
                        output_amount_usd: input_amount + gross_profit,
                        gross_profit_usd: gross_profit,
                        gas_cost_usd: gas_cost,
                        net_profit_usd: net_profit,
                        is_profitable,
                        trigger_event,
                        arb_steps: Vec::new(), // 简化模式没有详细步骤
                        price_deviation_percent: 0.0,
                        total_fee_percent: total_fee_pct,
                        arb_spread_percent: gross_profit_pct,
                        flash_loan_fee_usd,
                        flash_loan_fee_percent,
                        real_net_profit_usd,
                    };

                    if let Some(stats) = path_stats_map.get_mut(&path.path_name) {
                        stats.analysis_count += 1;
                        stats.total_profit_usd += net_profit;
                        if net_profit > stats.max_profit_usd {
                            stats.max_profit_usd = net_profit;
                        }
                        if is_profitable {
                            stats.profitable_count += 1;
                        }
                    }

                    all_opportunities.push(opp);
                }
            }

            pb.inc(1);
        }

        pb.finish_with_message("分析完成");

        // 计算平均利润
        for stats in path_stats_map.values_mut() {
            if stats.analysis_count > 0 {
                stats.avg_profit_usd = stats.total_profit_usd / stats.analysis_count as f64;
            }
            if stats.max_profit_usd == f64::NEG_INFINITY {
                stats.max_profit_usd = 0.0;
            }
        }

        let profitable_opportunities: Vec<_> = all_opportunities
            .iter()
            .filter(|o| o.is_profitable)
            .cloned()
            .collect();

        info!("总分析机会数: {}", all_opportunities.len());
        info!("盈利机会数: {}", profitable_opportunities.len());

        let stats = BacktestStatistics {
            start_block,
            end_block,
            start_timestamp: 0,
            end_timestamp: 0,
            total_blocks: end_block - start_block,
            blocks_with_swaps,
            total_volume_usd: total_volume,
            path_stats: path_stats_map.into_values().collect(),
            profitable_opportunities,
        };

        Ok(stats)
    }

    /// 获取交易对的最低费率
    fn get_lowest_fee(&self, token_a: &str, token_b: &str) -> Option<f64> {
        let pools = self.token_pair_pools.get(&(token_a.to_string(), token_b.to_string()))?;
        pools.first().map(|p| p.fee_percent())
    }
}
