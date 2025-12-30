use anyhow::Result;
use ethers::prelude::*;
use ethers::types::{Address, U256};
use models::{ArbitrageOpportunity, ArbitragePath, DexType};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, debug};

use crate::path_finder::{PathFinder, PathFinderConfig, PoolInfo};
use crate::profit_calculator::{ProfitCalculator, ProfitCalculatorConfig};

// Uniswap V2 Router ABI for getAmountsOut
abigen!(
    IUniswapV2Router,
    r#"[
        function getAmountsOut(uint amountIn, address[] memory path) public view returns (uint[] memory amounts)
    ]"#
);

// Uniswap V3 Quoter V2 ABI
abigen!(
    IQuoterV2,
    r#"[
        function quoteExactInputSingle(address tokenIn, address tokenOut, uint24 fee, uint256 amountIn, uint160 sqrtPriceLimitX96) external returns (uint256 amountOut, uint160 sqrtPriceX96After, uint32 initializedTicksCrossed, uint256 gasEstimate)
    ]"#
);

/// 套利扫描器配置
#[derive(Debug, Clone)]
pub struct ArbitrageScannerConfig {
    pub scan_interval_ms: u64,
    pub max_concurrent_checks: usize,
    pub min_profit_usd: Decimal,
    pub max_slippage: Decimal,
    pub target_tokens: Vec<Address>,
    /// V3 Quoter 合约地址
    pub v3_quoter_address: Option<Address>,
    /// 机会过期时间 (毫秒) - 用于 TTL 检查
    pub opportunity_ttl_ms: u64,
    /// 是否在执行前重新验证机会
    pub verify_before_execute: bool,
}

impl Default for ArbitrageScannerConfig {
    fn default() -> Self {
        Self {
            scan_interval_ms: 100,
            max_concurrent_checks: 10,
            min_profit_usd: dec!(10),
            max_slippage: dec!(0.005),
            target_tokens: Vec::new(),
            // Uniswap V3 Quoter V2 (Ethereum Mainnet)
            v3_quoter_address: "0x61fFE014bA17989E743c5F6cB21bF9697530B21e".parse().ok(),
            opportunity_ttl_ms: 200, // 200ms TTL - 套利机会有效期很短
            verify_before_execute: true,
        }
    }
}

/// V3 池状态缓存
#[derive(Debug, Clone)]
pub struct V3PoolState {
    pub pool_address: Address,
    pub sqrt_price_x96: U256,
    pub tick: i32,
    pub liquidity: u128,
    pub fee: u32,
    pub updated_at: std::time::Instant,
}

/// 套利扫描器
#[allow(dead_code)]
pub struct ArbitrageScanner<M: Middleware> {
    provider: Arc<M>,
    config: ArbitrageScannerConfig,
    path_finder: RwLock<PathFinder>,
    profit_calculator: RwLock<ProfitCalculator>,
    /// 发现的机会队列
    opportunities: RwLock<Vec<ArbitrageOpportunity>>,
    /// V3 池状态缓存
    v3_pool_states: RwLock<std::collections::HashMap<Address, V3PoolState>>,
    /// 是否正在运行
    running: RwLock<bool>,
}

impl<M: Middleware + 'static> ArbitrageScanner<M> {
    pub fn new(provider: Arc<M>, config: ArbitrageScannerConfig) -> Self {
        Self {
            provider,
            config: config.clone(),
            path_finder: RwLock::new(PathFinder::new(PathFinderConfig::default())),
            profit_calculator: RwLock::new(ProfitCalculator::new(ProfitCalculatorConfig {
                min_profit_usd: config.min_profit_usd,
                max_slippage: config.max_slippage,
                ..Default::default()
            })),
            opportunities: RwLock::new(Vec::new()),
            v3_pool_states: RwLock::new(std::collections::HashMap::new()),
            running: RwLock::new(false),
        }
    }

    /// 添加要监控的池子
    pub async fn add_pool(&self, pool: PoolInfo) {
        let mut path_finder = self.path_finder.write().await;
        path_finder.add_pool(pool);
    }

    /// 获取池子数量
    pub async fn pool_count(&self) -> usize {
        let path_finder = self.path_finder.read().await;
        path_finder.pool_count()
    }

    /// 扫描三角套利机会
    pub async fn scan_triangular_opportunities(
        &self,
        start_token: Address,
    ) -> Result<Vec<ArbitrageOpportunity>> {
        let path_finder = self.path_finder.read().await;
        let paths = path_finder.find_triangular_paths(start_token);

        debug!("扫描代币 {:?}: 找到 {} 条三角套利路径, 总池子数: {}",
               start_token, paths.len(), path_finder.pool_count());

        drop(path_finder);

        let mut opportunities = Vec::new();

        for path in paths {
            if let Some(opportunity) = self.evaluate_path(&path).await? {
                opportunities.push(opportunity);
            }
        }

        // 按利润排序
        opportunities.sort_by(|a, b| b.net_profit_usd.cmp(&a.net_profit_usd));

        Ok(opportunities)
    }

    /// 评估套利路径 - 使用完整的利润分析
    async fn evaluate_path(&self, path: &ArbitragePath) -> Result<Option<ArbitrageOpportunity>> {
        if path.hops.is_empty() {
            return Ok(None);
        }

        let profit_calculator = self.profit_calculator.read().await;

        // 估算 gas
        let gas_estimate = profit_calculator.estimate_gas(path, true);

        // 根据起始代币选择合适的输入量
        let input_amount = self.get_optimal_input_amount(&path.start_token);

        // 模拟路径获取预期输出 (使用完整的 V3 tick 模拟)
        let expected_output = match self.simulate_path_full(path, input_amount).await {
            Ok(output) => output,
            Err(e) => {
                debug!("模拟路径失败: {}", e);
                return Ok(None);
            }
        };

        // 如果输出小于输入，没有套利机会
        if expected_output <= input_amount {
            return Ok(None);
        }

        // 使用完整的利润分析
        let analysis = profit_calculator.analyze_profit(input_amount, expected_output, gas_estimate, path);

        // 检查是否有利可图
        if !analysis.is_profitable {
            return Ok(None);
        }

        // 获取当前区块号
        let block_number = self.provider.get_block_number().await.unwrap_or_default().as_u64();

        let expected_profit = expected_output.saturating_sub(input_amount);

        let opportunity = ArbitrageOpportunity {
            id: uuid::Uuid::new_v4().to_string(),
            path: path.clone(),
            input_amount,
            expected_output,
            expected_profit,
            expected_profit_usd: analysis.net_profit_usd,
            gas_estimate,
            gas_cost_usd: analysis.total_gas_cost_usd,
            net_profit_usd: analysis.net_profit_usd,
            profit_percentage: analysis.profit_percentage,
            timestamp: chrono::Utc::now(),
            block_number,
        };

        info!(
            "发现套利机会: 路径长度={}, 输入={}, 输出={}, 原始利润={}, 净利润={} (扣除 gas={}, 滑点={})",
            path.hops.len(),
            input_amount,
            expected_output,
            analysis.raw_profit,
            analysis.net_profit_usd,
            analysis.total_gas_cost_usd,
            analysis.slippage_cost_usd
        );

        Ok(Some(opportunity))
    }

    /// 完整路径模拟 - 包含 V3 tick 模拟
    async fn simulate_path_full(&self, path: &ArbitragePath, input_amount: U256) -> Result<U256> {
        let mut current_amount = input_amount;

        for hop in &path.hops {
            current_amount = match hop.dex_type {
                DexType::UniswapV3 | DexType::PancakeSwapV3 | DexType::SushiSwapV3 => {
                    // 使用 V3 Quoter 进行精确模拟
                    self.simulate_v3_swap_with_quoter(hop, current_amount).await?
                }
                _ => {
                    self.simulate_swap(hop, current_amount).await?
                }
            };

            // 如果中间结果为 0，路径无效
            if current_amount.is_zero() {
                return Ok(U256::zero());
            }
        }

        Ok(current_amount)
    }

    /// 使用 V3 Quoter 进行精确模拟
    async fn simulate_v3_swap_with_quoter(
        &self,
        hop: &models::SwapHop,
        amount_in: U256,
    ) -> Result<U256> {
        // 如果配置了 Quoter 地址，使用链上 Quoter
        if let Some(quoter_address) = self.config.v3_quoter_address {
            let quoter = IQuoterV2::new(quoter_address, self.provider.clone());

            // sqrtPriceLimitX96 = 0 表示不限制价格
            match quoter
                .quote_exact_input_single(
                    hop.token_in,
                    hop.token_out,
                    hop.fee as u32,
                    amount_in,
                    U256::zero(),
                )
                .call()
                .await
            {
                Ok((amount_out, _sqrt_price_after, ticks_crossed, _gas_estimate)) => {
                    debug!(
                        "V3 Quoter 结果: amountOut={}, ticksCrossed={}",
                        amount_out, ticks_crossed
                    );
                    return Ok(amount_out);
                }
                Err(e) => {
                    debug!("V3 Quoter 调用失败，回退到本地计算: {}", e);
                    // 回退到本地模拟
                }
            }
        }

        // 回退: 使用本地 tick 模拟
        self.simulate_v3_swap_local(hop, amount_in).await
    }

    /// 本地 V3 swap 模拟 (基于 tick 数学)
    async fn simulate_v3_swap_local(
        &self,
        hop: &models::SwapHop,
        amount_in: U256,
    ) -> Result<U256> {
        // 获取池状态
        let pool_state = self.get_v3_pool_state(hop.pool_address).await?;

        // V3 swap 模拟 - 基于集中流动性数学
        // 简化版本: 考虑当前 tick 范围内的流动性
        let fee_factor = U256::from(1_000_000 - hop.fee);

        // 计算价格影响 (简化: 基于流动性和交易量)
        let liquidity = U256::from(pool_state.liquidity);
        let price_impact = if liquidity > U256::zero() {
            // 价格影响 = amountIn / liquidity (简化估算)
            let impact_bps = amount_in * U256::from(10000) / liquidity;
            // 限制最大价格影响为 10%
            impact_bps.min(U256::from(1000))
        } else {
            U256::from(100) // 默认 1% 影响
        };

        // 输出 = 输入 * (1 - fee) * (1 - priceImpact)
        let amount_after_fee = amount_in * fee_factor / U256::from(1_000_000);
        let impact_factor = U256::from(10000) - price_impact;
        let amount_out = amount_after_fee * impact_factor / U256::from(10000);

        Ok(amount_out)
    }

    /// 获取 V3 池状态 (带缓存)
    async fn get_v3_pool_state(&self, pool_address: Address) -> Result<V3PoolState> {
        // 检查缓存
        {
            let cache = self.v3_pool_states.read().await;
            if let Some(state) = cache.get(&pool_address) {
                // 缓存有效期 100ms
                if state.updated_at.elapsed().as_millis() < 100 {
                    return Ok(state.clone());
                }
            }
        }

        // 从链上获取
        abigen!(
            IUniswapV3Pool,
            r#"[
                function slot0() external view returns (uint160 sqrtPriceX96, int24 tick, uint16 observationIndex, uint16 observationCardinality, uint16 observationCardinalityNext, uint8 feeProtocol, bool unlocked)
                function liquidity() external view returns (uint128)
                function fee() external view returns (uint24)
            ]"#
        );

        let pool = IUniswapV3Pool::new(pool_address, self.provider.clone());

        let (sqrt_price_x96, tick, ..) = pool.slot_0().call().await?;
        let liquidity = pool.liquidity().call().await?;
        let fee = pool.fee().call().await?;

        let state = V3PoolState {
            pool_address,
            sqrt_price_x96: U256::from(sqrt_price_x96.as_u128()),
            tick,
            liquidity,
            fee,
            updated_at: std::time::Instant::now(),
        };

        // 更新缓存
        {
            let mut cache = self.v3_pool_states.write().await;
            cache.insert(pool_address, state.clone());
        }

        Ok(state)
    }

    /// 获取最优输入金额
    fn get_optimal_input_amount(&self, token: &Address) -> U256 {
        // 常见稳定币地址 (Ethereum)
        let usdt: Address = "0xdAC17F958D2ee523a2206206994597C13D831ec7".parse().unwrap_or_default();
        let usdc: Address = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".parse().unwrap_or_default();
        let dai: Address = "0x6B175474E89094C44Da98b954EedeAC495271d0F".parse().unwrap_or_default();
        let weth: Address = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".parse().unwrap_or_default();

        if *token == usdt || *token == usdc {
            // USDT/USDC: 6 位小数，使用 10000 美元
            U256::from(10000) * U256::exp10(6)
        } else if *token == dai {
            // DAI: 18 位小数，使用 10000 美元
            U256::from(10000) * U256::exp10(18)
        } else if *token == weth {
            // WETH: 18 位小数，使用 5 ETH
            U256::from(5) * U256::exp10(18)
        } else {
            // 默认: 假设 18 位小数，使用 1000 单位
            U256::from(1000) * U256::exp10(18)
        }
    }

    /// 模拟套利路径 (旧版本，保留向后兼容)
    #[allow(dead_code)]
    async fn simulate_path(&self, path: &ArbitragePath, input_amount: U256) -> Result<U256> {
        self.simulate_path_full(path, input_amount).await
    }

    /// 模拟单次交换
    async fn simulate_swap(&self, hop: &models::SwapHop, amount_in: U256) -> Result<U256> {
        match hop.dex_type {
            DexType::UniswapV2 | DexType::SushiSwap | DexType::SushiSwapV2 | DexType::PancakeSwapV2 => {
                self.simulate_v2_swap(hop.pool_address, hop.token_in, hop.token_out, amount_in)
                    .await
            }
            DexType::UniswapV3 | DexType::PancakeSwapV3 | DexType::SushiSwapV3 => {
                self.simulate_v3_swap_with_quoter(hop, amount_in).await
            }
            _ => {
                // 其他 DEX 暂时使用估算
                Ok(amount_in * U256::from(997) / U256::from(1000))
            }
        }
    }

    /// 模拟 V2 交换 (使用储备量计算)
    async fn simulate_v2_swap(
        &self,
        pool_address: Address,
        token_in: Address,
        _token_out: Address,
        amount_in: U256,
    ) -> Result<U256> {
        // 定义 Pair 合约
        abigen!(
            IUniswapV2Pair,
            r#"[
                function token0() external view returns (address)
                function getReserves() external view returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast)
            ]"#
        );

        let pair = IUniswapV2Pair::new(pool_address, self.provider.clone());

        let token0 = pair.token_0().call().await?;
        let (reserve0, reserve1, _) = pair.get_reserves().call().await?;

        let (reserve_in, reserve_out) = if token_in == token0 {
            (U256::from(reserve0), U256::from(reserve1))
        } else {
            (U256::from(reserve1), U256::from(reserve0))
        };

        // 检查储备量
        if reserve_in.is_zero() || reserve_out.is_zero() {
            return Ok(U256::zero());
        }

        // Uniswap V2 公式: amountOut = (amountIn * 997 * reserveOut) / (reserveIn * 1000 + amountIn * 997)
        let amount_in_with_fee = amount_in * U256::from(997);
        let numerator = amount_in_with_fee * reserve_out;
        let denominator = reserve_in * U256::from(1000) + amount_in_with_fee;

        if denominator.is_zero() {
            return Ok(U256::zero());
        }

        Ok(numerator / denominator)
    }

    /// 估算 V3 交换输出 (简化版，用于无 Quoter 的情况)
    #[allow(dead_code)]
    async fn estimate_v3_swap_output(
        &self,
        _pool_address: Address,
        _token_in: Address,
        _token_out: Address,
        amount_in: U256,
        fee: u32,
    ) -> Result<U256> {
        // V3 简化计算: 扣除手续费
        // fee 是以 1/1000000 为单位 (例如 3000 = 0.3%)
        let fee_factor = U256::from(1_000_000 - fee);
        let amount_out = amount_in * fee_factor / U256::from(1_000_000);
        Ok(amount_out)
    }

    /// 计算 gas 成本 (USD)
    #[allow(dead_code)]
    fn calculate_gas_cost_usd(&self, gas_estimate: U256, calculator: &ProfitCalculator) -> Decimal {
        let gas = Decimal::from_str_exact(&gas_estimate.to_string()).unwrap_or(Decimal::ZERO);
        let gas_price_gwei = calculator.get_total_gas_price(); // 使用总 gas 价格 (base + priority)
        let eth_price = calculator.get_eth_price();

        gas * gas_price_gwei / dec!(1_000_000_000) * eth_price
    }

    /// 执行前重新验证机会
    ///
    /// 在执行交易前，重新检查机会是否仍然有效
    pub async fn verify_opportunity(&self, opportunity: &ArbitrageOpportunity) -> Result<Option<ArbitrageOpportunity>> {
        // 检查 TTL
        let age_ms = (chrono::Utc::now() - opportunity.timestamp).num_milliseconds() as u64;
        if age_ms > self.config.opportunity_ttl_ms {
            debug!("机会已过期: age={}ms, ttl={}ms", age_ms, self.config.opportunity_ttl_ms);
            return Ok(None);
        }

        // 重新模拟路径
        let new_output = self.simulate_path_full(&opportunity.path, opportunity.input_amount).await?;

        // 检查输出是否仍然有利可图
        if new_output < opportunity.input_amount {
            debug!("重新验证失败: 输出 {} < 输入 {}", new_output, opportunity.input_amount);
            return Ok(None);
        }

        // 使用完整利润分析
        let profit_calculator = self.profit_calculator.read().await;
        let analysis = profit_calculator.analyze_profit(
            opportunity.input_amount,
            new_output,
            opportunity.gas_estimate,
            &opportunity.path,
        );

        if !analysis.is_profitable {
            debug!(
                "重新验证失败: 净利润 {} 低于阈值",
                analysis.net_profit_usd
            );
            return Ok(None);
        }

        // 检查利润变化
        let profit_change_pct = if opportunity.net_profit_usd > Decimal::ZERO {
            ((analysis.net_profit_usd - opportunity.net_profit_usd) / opportunity.net_profit_usd) * dec!(100)
        } else {
            Decimal::ZERO
        };

        // 如果利润下降超过 20%，警告但仍然返回
        if profit_change_pct < dec!(-20) {
            warn!(
                "利润大幅下降: 原始={}, 当前={}, 变化={}%",
                opportunity.net_profit_usd, analysis.net_profit_usd, profit_change_pct
            );
        }

        // 返回更新后的机会
        let updated_opportunity = ArbitrageOpportunity {
            id: opportunity.id.clone(),
            path: opportunity.path.clone(),
            input_amount: opportunity.input_amount,
            expected_output: new_output,
            expected_profit: new_output.saturating_sub(opportunity.input_amount),
            expected_profit_usd: analysis.net_profit_usd,
            gas_estimate: opportunity.gas_estimate,
            gas_cost_usd: analysis.total_gas_cost_usd,
            net_profit_usd: analysis.net_profit_usd,
            profit_percentage: analysis.profit_percentage,
            timestamp: chrono::Utc::now(),
            block_number: self.provider.get_block_number().await.unwrap_or_default().as_u64(),
        };

        Ok(Some(updated_opportunity))
    }

    /// 批量验证机会并按利润排序
    pub async fn verify_and_rank_opportunities(
        &self,
        opportunities: Vec<ArbitrageOpportunity>,
    ) -> Vec<ArbitrageOpportunity> {
        let mut valid_opportunities = Vec::new();

        for opp in opportunities {
            if let Ok(Some(verified)) = self.verify_opportunity(&opp).await {
                valid_opportunities.push(verified);
            }
        }

        // 按净利润排序
        valid_opportunities.sort_by(|a, b| b.net_profit_usd.cmp(&a.net_profit_usd));

        valid_opportunities
    }

    /// 启动扫描循环
    pub async fn start_scanning(&self) -> Result<()> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(());
        }
        *running = true;
        drop(running);

        info!("套利扫描器启动 (TTL={}ms, 执行前验证={})",
            self.config.opportunity_ttl_ms,
            self.config.verify_before_execute
        );

        loop {
            let running = self.running.read().await;
            if !*running {
                break;
            }
            drop(running);

            // 扫描所有目标代币
            for token in &self.config.target_tokens {
                match self.scan_triangular_opportunities(*token).await {
                    Ok(ops) => {
                        if !ops.is_empty() {
                            info!("发现 {} 个套利机会", ops.len());
                            let mut opportunities = self.opportunities.write().await;

                            // 清理过期机会
                            let now = chrono::Utc::now();
                            opportunities.retain(|o| {
                                let age_ms = (now - o.timestamp).num_milliseconds() as u64;
                                age_ms < self.config.opportunity_ttl_ms
                            });

                            // 添加新机会
                            opportunities.extend(ops);

                            // 按利润排序
                            opportunities.sort_by(|a, b| b.net_profit_usd.cmp(&a.net_profit_usd));
                        }
                    }
                    Err(e) => {
                        warn!("扫描失败: {}", e);
                    }
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(
                self.config.scan_interval_ms,
            ))
            .await;
        }

        info!("套利扫描器停止");
        Ok(())
    }

    /// 停止扫描
    pub async fn stop_scanning(&self) {
        let mut running = self.running.write().await;
        *running = false;
    }

    /// 获取并清空发现的机会
    pub async fn take_opportunities(&self) -> Vec<ArbitrageOpportunity> {
        let mut opportunities = self.opportunities.write().await;

        // 清理过期机会
        let now = chrono::Utc::now();
        opportunities.retain(|o| {
            let age_ms = (now - o.timestamp).num_milliseconds() as u64;
            age_ms < self.config.opportunity_ttl_ms
        });

        std::mem::take(&mut *opportunities)
    }

    /// 获取最佳机会 (不清空队列)
    pub async fn get_best_opportunity(&self) -> Option<ArbitrageOpportunity> {
        let opportunities = self.opportunities.read().await;

        // 找到未过期的最佳机会
        let now = chrono::Utc::now();
        opportunities
            .iter()
            .filter(|o| {
                let age_ms = (now - o.timestamp).num_milliseconds() as u64;
                age_ms < self.config.opportunity_ttl_ms
            })
            .max_by(|a, b| a.net_profit_usd.cmp(&b.net_profit_usd))
            .cloned()
    }

    /// 更新 ETH 价格
    pub async fn update_eth_price(&self, price: Decimal) {
        let mut calculator = self.profit_calculator.write().await;
        calculator.update_eth_price(price);
    }

    /// 更新 gas 价格
    pub async fn update_gas_price(&self, gas_price_gwei: Decimal) {
        let mut calculator = self.profit_calculator.write().await;
        calculator.update_gas_price(gas_price_gwei);
    }

    /// 更新优先费
    pub async fn update_priority_fee(&self, priority_fee_gwei: Decimal) {
        let mut calculator = self.profit_calculator.write().await;
        calculator.update_priority_fee(priority_fee_gwei);
    }
}
