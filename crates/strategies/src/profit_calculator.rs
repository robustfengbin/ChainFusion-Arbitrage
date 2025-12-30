use ethers::types::U256;
use models::{ArbitragePath, DexType};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// 利润计算器配置
#[derive(Debug, Clone)]
pub struct ProfitCalculatorConfig {
    /// 最大滑点 (例如 0.005 = 0.5%)
    pub max_slippage: Decimal,
    /// 当前 gas 价格 (Gwei)
    pub gas_price_gwei: Decimal,
    /// 优先费 (Gwei) - 用于 MEV 保护
    pub priority_fee_gwei: Decimal,
    /// ETH 价格 (USD)
    pub eth_price_usd: Decimal,
    /// 最低净利润阈值 (USD)
    pub min_profit_usd: Decimal,
    /// 安全边际系数 (例如 0.8 表示预期利润的 80% 作为实际可接受利润)
    pub safety_margin: Decimal,
    /// 滑点缓冲系数 (例如 1.5 表示预估滑点的 1.5 倍)
    pub slippage_buffer: Decimal,
    /// Gas 估算缓冲系数 (例如 1.3 表示 gas 估算的 130%)
    pub gas_buffer: Decimal,
}

impl Default for ProfitCalculatorConfig {
    fn default() -> Self {
        Self {
            max_slippage: dec!(0.005),        // 0.5%
            gas_price_gwei: dec!(30),
            priority_fee_gwei: dec!(2),        // 2 Gwei priority fee for MEV
            eth_price_usd: dec!(2000),
            min_profit_usd: dec!(10),
            safety_margin: dec!(0.8),          // 保留 20% 安全边际
            slippage_buffer: dec!(1.5),        // 滑点 1.5 倍缓冲
            gas_buffer: dec!(1.3),             // Gas 1.3 倍缓冲
        }
    }
}

/// 完整的利润计算结果
#[derive(Debug, Clone)]
pub struct ProfitAnalysis {
    /// 预期输出金额
    pub expected_output: Decimal,
    /// 原始利润 (输出 - 输入)
    pub raw_profit: Decimal,
    /// Gas 基础成本 (USD)
    pub gas_cost_usd: Decimal,
    /// 优先费成本 (USD) - MEV 保护
    pub priority_fee_cost_usd: Decimal,
    /// 总 gas 成本 (USD)
    pub total_gas_cost_usd: Decimal,
    /// 预估滑点损失 (USD)
    pub slippage_cost_usd: Decimal,
    /// 安全边际扣除 (USD)
    pub safety_margin_cost_usd: Decimal,
    /// 净利润 (USD) - 扣除所有成本后
    pub net_profit_usd: Decimal,
    /// 利润百分比
    pub profit_percentage: Decimal,
    /// 是否有利可图
    pub is_profitable: bool,
    /// 盈亏平衡输出金额
    pub break_even_output: Decimal,
}

/// 利润计算器
pub struct ProfitCalculator {
    config: ProfitCalculatorConfig,
}

impl ProfitCalculator {
    pub fn new(config: ProfitCalculatorConfig) -> Self {
        Self { config }
    }

    /// 完整的利润分析
    ///
    /// 计算所有成本并返回详细的利润分析结果
    pub fn analyze_profit(
        &self,
        input_amount: U256,
        expected_output: U256,
        gas_estimate: U256,
        path: &ArbitragePath,
    ) -> ProfitAnalysis {
        let input = u256_to_decimal(input_amount);
        let output = u256_to_decimal(expected_output);

        // 1. 原始利润
        let raw_profit = output - input;

        // 2. Gas 成本计算 (包含缓冲)
        let gas = u256_to_decimal(gas_estimate);
        let buffered_gas = gas * self.config.gas_buffer;

        // 基础 gas 成本
        let gas_cost_eth = buffered_gas * self.config.gas_price_gwei / dec!(1_000_000_000);
        let gas_cost_usd = gas_cost_eth * self.config.eth_price_usd;

        // 优先费成本 (MEV 保护)
        let priority_fee_eth = buffered_gas * self.config.priority_fee_gwei / dec!(1_000_000_000);
        let priority_fee_cost_usd = priority_fee_eth * self.config.eth_price_usd;

        let total_gas_cost_usd = gas_cost_usd + priority_fee_cost_usd;

        // 3. 滑点成本估算
        let slippage_cost_usd = self.estimate_slippage_cost(path, input);

        // 4. 计算净利润 (扣除 gas 和滑点)
        let profit_after_costs = raw_profit - total_gas_cost_usd - slippage_cost_usd;

        // 5. 应用安全边际
        let safety_margin_cost_usd = if profit_after_costs > Decimal::ZERO {
            profit_after_costs * (Decimal::ONE - self.config.safety_margin)
        } else {
            Decimal::ZERO
        };

        let net_profit_usd = profit_after_costs - safety_margin_cost_usd;

        // 6. 利润百分比
        let profit_percentage = if input > Decimal::ZERO {
            (net_profit_usd / input) * dec!(100)
        } else {
            Decimal::ZERO
        };

        // 7. 盈亏平衡点
        let break_even_output = input + total_gas_cost_usd + slippage_cost_usd;

        // 8. 是否有利可图
        let is_profitable = net_profit_usd > self.config.min_profit_usd;

        ProfitAnalysis {
            expected_output: output,
            raw_profit,
            gas_cost_usd,
            priority_fee_cost_usd,
            total_gas_cost_usd,
            slippage_cost_usd,
            safety_margin_cost_usd,
            net_profit_usd,
            profit_percentage,
            is_profitable,
            break_even_output,
        }
    }

    /// 简化的利润计算 (向后兼容)
    ///
    /// 返回 (预期输出, 净利润, 利润百分比)
    pub fn calculate_profit(
        &self,
        input_amount: U256,
        expected_output: U256,
        gas_estimate: U256,
    ) -> (Decimal, Decimal, Decimal) {
        let input = u256_to_decimal(input_amount);
        let output = u256_to_decimal(expected_output);

        // 原始利润
        let raw_profit = output - input;

        // Gas 成本 (包含缓冲和优先费)
        let gas = u256_to_decimal(gas_estimate);
        let buffered_gas = gas * self.config.gas_buffer;
        let total_gas_price = self.config.gas_price_gwei + self.config.priority_fee_gwei;
        let gas_cost_eth = buffered_gas * total_gas_price / dec!(1_000_000_000);
        let gas_cost_usd = gas_cost_eth * self.config.eth_price_usd;

        // 滑点成本 (使用最大滑点 * 缓冲)
        let slippage_cost = input * self.config.max_slippage * self.config.slippage_buffer;

        // 净利润 (含安全边际)
        let profit_after_costs = raw_profit - gas_cost_usd - slippage_cost;
        let net_profit = profit_after_costs * self.config.safety_margin;

        // 利润百分比
        let profit_percentage = if input > Decimal::ZERO {
            (net_profit / input) * dec!(100)
        } else {
            Decimal::ZERO
        };

        (output, net_profit, profit_percentage)
    }

    /// 检查套利是否有利可图
    pub fn is_profitable(
        &self,
        input_amount: U256,
        expected_output: U256,
        gas_estimate: U256,
    ) -> bool {
        let (_, net_profit, _) = self.calculate_profit(input_amount, expected_output, gas_estimate);
        net_profit > self.config.min_profit_usd
    }

    /// 计算最小可接受输出
    ///
    /// 用于设置交易的 minAmountOut 参数
    pub fn calculate_min_output(
        &self,
        input_amount: U256,
        expected_output: U256,
        gas_estimate: U256,
    ) -> U256 {
        let input = u256_to_decimal(input_amount);
        let output = u256_to_decimal(expected_output);
        let gas = u256_to_decimal(gas_estimate);

        // 计算总成本
        let buffered_gas = gas * self.config.gas_buffer;
        let total_gas_price = self.config.gas_price_gwei + self.config.priority_fee_gwei;
        let gas_cost_eth = buffered_gas * total_gas_price / dec!(1_000_000_000);
        let gas_cost_usd = gas_cost_eth * self.config.eth_price_usd;

        // 最小可接受输出 = 输入 + gas成本 + 最低利润
        let min_output = input + gas_cost_usd + self.config.min_profit_usd;

        // 同时考虑滑点，取两者较大值
        let slippage_adjusted = output * (Decimal::ONE - self.config.max_slippage * self.config.slippage_buffer);

        let final_min = min_output.max(slippage_adjusted);

        decimal_to_u256(final_min)
    }

    /// 估算滑点成本
    fn estimate_slippage_cost(&self, path: &ArbitragePath, input_amount: Decimal) -> Decimal {
        let mut total_slippage = Decimal::ZERO;

        for hop in &path.hops {
            // 不同 DEX 类型的滑点特性不同
            let base_slippage = match hop.dex_type {
                // V2 类型: 恒定乘积，滑点与交易量成正比
                DexType::UniswapV2 | DexType::SushiSwap | DexType::SushiSwapV2 | DexType::PancakeSwapV2 => {
                    self.config.max_slippage
                }
                // V3 类型: 集中流动性，小额交易滑点较低
                DexType::UniswapV3 | DexType::PancakeSwapV3 | DexType::SushiSwapV3 => {
                    self.config.max_slippage * dec!(0.7) // V3 通常滑点更低
                }
                // Curve: 稳定币优化，滑点很低
                DexType::Curve => {
                    self.config.max_slippage * dec!(0.3)
                }
                // V4: 更高效
                DexType::UniswapV4 => {
                    self.config.max_slippage * dec!(0.5)
                }
            };

            total_slippage += base_slippage;
        }

        // 应用滑点缓冲
        let buffered_slippage = total_slippage * self.config.slippage_buffer;

        input_amount * buffered_slippage
    }

    /// 估算最优输入量
    ///
    /// 使用二分搜索找到最大化利润的输入量
    pub fn find_optimal_input(
        &self,
        min_input: U256,
        max_input: U256,
        simulate_fn: impl Fn(U256) -> Option<U256>,
        gas_estimate: U256,
    ) -> Option<(U256, U256, Decimal)> {
        let mut best_input = U256::zero();
        let mut best_output = U256::zero();
        let mut best_profit = Decimal::MIN;

        // 使用更细的网格搜索
        let steps = 20;
        let step_size = (max_input - min_input) / U256::from(steps);

        for i in 0..=steps {
            let input = min_input + step_size * U256::from(i);

            if let Some(output) = simulate_fn(input) {
                let (_, profit, _) = self.calculate_profit(input, output, gas_estimate);

                if profit > best_profit {
                    best_profit = profit;
                    best_input = input;
                    best_output = output;
                }
            }
        }

        // 只有净利润超过阈值才返回
        if best_profit > self.config.min_profit_usd {
            Some((best_input, best_output, best_profit))
        } else {
            None
        }
    }

    /// 估算 gas 消耗
    pub fn estimate_gas(&self, path: &ArbitragePath, use_flash_loan: bool) -> U256 {
        let base_gas: u64 = 21000;

        // 闪电贷开销
        let flash_loan_gas: u64 = if use_flash_loan {
            150000 // Aave V3 闪电贷约 150k gas
        } else {
            0
        };

        // 合约调用基础开销
        let contract_overhead: u64 = 50000;

        let mut swap_gas: u64 = 0;
        for hop in &path.hops {
            swap_gas += match hop.dex_type {
                DexType::UniswapV2 | DexType::SushiSwap | DexType::SushiSwapV2 => 120000,
                DexType::PancakeSwapV2 => 110000,
                DexType::UniswapV3 | DexType::SushiSwapV3 => 180000,  // V3 tick 跨越可能消耗更多
                DexType::PancakeSwapV3 => 170000,
                DexType::Curve => 250000,       // Curve 复杂池
                DexType::UniswapV4 => 100000,   // V4 更高效
            };
        }

        // 应用 gas 缓冲
        let total = base_gas + flash_loan_gas + contract_overhead + swap_gas;
        let buffered_total = (Decimal::from(total) * self.config.gas_buffer)
            .to_string()
            .parse::<u64>()
            .unwrap_or(total);

        U256::from(buffered_total)
    }

    /// 计算盈亏平衡 gas 价格
    ///
    /// 返回在当前利润下，最高可接受的 gas 价格 (Gwei)
    pub fn calculate_break_even_gas_price(
        &self,
        input_amount: U256,
        expected_output: U256,
        gas_estimate: U256,
    ) -> Decimal {
        let input = u256_to_decimal(input_amount);
        let output = u256_to_decimal(expected_output);
        let gas = u256_to_decimal(gas_estimate);

        let raw_profit = output - input;
        let slippage_cost = input * self.config.max_slippage * self.config.slippage_buffer;

        // 可用于 gas 的利润
        let available_for_gas = raw_profit - slippage_cost - self.config.min_profit_usd;

        if available_for_gas <= Decimal::ZERO || gas.is_zero() {
            return Decimal::ZERO;
        }

        // 反推 gas 价格
        // available_for_gas = gas * gas_price_gwei * eth_price / 1e9
        let max_gas_price_gwei = available_for_gas * dec!(1_000_000_000) / (gas * self.config.eth_price_usd);

        max_gas_price_gwei
    }

    /// 更新 ETH 价格
    pub fn update_eth_price(&mut self, price: Decimal) {
        self.config.eth_price_usd = price;
    }

    /// 更新 gas 价格
    pub fn update_gas_price(&mut self, gas_price_gwei: Decimal) {
        self.config.gas_price_gwei = gas_price_gwei;
    }

    /// 更新优先费
    pub fn update_priority_fee(&mut self, priority_fee_gwei: Decimal) {
        self.config.priority_fee_gwei = priority_fee_gwei;
    }

    /// 获取当前 ETH 价格
    pub fn get_eth_price(&self) -> Decimal {
        self.config.eth_price_usd
    }

    /// 获取当前 gas 价格 (Gwei)
    pub fn get_gas_price(&self) -> Decimal {
        self.config.gas_price_gwei
    }

    /// 获取当前优先费 (Gwei)
    pub fn get_priority_fee(&self) -> Decimal {
        self.config.priority_fee_gwei
    }

    /// 获取总 gas 价格 (base + priority)
    pub fn get_total_gas_price(&self) -> Decimal {
        self.config.gas_price_gwei + self.config.priority_fee_gwei
    }

    /// 获取最低利润阈值
    pub fn get_min_profit_threshold(&self) -> Decimal {
        self.config.min_profit_usd
    }

    /// 设置最低利润阈值
    pub fn set_min_profit_threshold(&mut self, threshold: Decimal) {
        self.config.min_profit_usd = threshold;
    }

    /// 获取安全边际系数
    pub fn get_safety_margin(&self) -> Decimal {
        self.config.safety_margin
    }

    /// 设置安全边际系数
    pub fn set_safety_margin(&mut self, margin: Decimal) {
        self.config.safety_margin = margin;
    }
}

/// U256 转 Decimal
fn u256_to_decimal(value: U256) -> Decimal {
    Decimal::from_str_exact(&value.to_string()).unwrap_or(Decimal::ZERO)
}

/// Decimal 转 U256
fn decimal_to_u256(value: Decimal) -> U256 {
    if value <= Decimal::ZERO {
        return U256::zero();
    }
    // 取整数部分
    let int_value = value.trunc();
    U256::from_dec_str(&int_value.to_string()).unwrap_or(U256::zero())
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::SwapHop;
    use ethers::types::Address;

    fn create_test_path() -> ArbitragePath {
        let mut path = ArbitragePath::new(Address::zero(), 1);
        path.add_hop(SwapHop {
            pool_address: Address::zero(),
            dex_type: DexType::UniswapV2,
            token_in: Address::zero(),
            token_out: Address::zero(),
            fee: 3000,
        });
        path.add_hop(SwapHop {
            pool_address: Address::zero(),
            dex_type: DexType::UniswapV3,
            token_in: Address::zero(),
            token_out: Address::zero(),
            fee: 500,
        });
        path.add_hop(SwapHop {
            pool_address: Address::zero(),
            dex_type: DexType::UniswapV2,
            token_in: Address::zero(),
            token_out: Address::zero(),
            fee: 3000,
        });
        path
    }

    #[test]
    fn test_profit_calculation_with_costs() {
        let calculator = ProfitCalculator::new(ProfitCalculatorConfig::default());

        let input = U256::from(1000) * U256::exp10(18);
        let output = U256::from(1100) * U256::exp10(18); // 10% raw profit
        let gas = U256::from(300000);

        let (_, net_profit, percentage) = calculator.calculate_profit(input, output, gas);

        // 净利润应该小于原始利润 (扣除了 gas、滑点和安全边际)
        let raw_profit = dec!(100) * Decimal::from(10u64.pow(18));
        assert!(net_profit < raw_profit);
        assert!(percentage > Decimal::ZERO);
    }

    #[test]
    fn test_full_profit_analysis() {
        let calculator = ProfitCalculator::new(ProfitCalculatorConfig::default());
        let path = create_test_path();

        let input = U256::from(10000) * U256::exp10(6); // 10000 USDC
        let output = U256::from(10500) * U256::exp10(6); // 10500 USDC (5% profit)
        let gas = U256::from(400000);

        let analysis = calculator.analyze_profit(input, output, gas, &path);

        assert!(analysis.raw_profit > Decimal::ZERO);
        assert!(analysis.total_gas_cost_usd > Decimal::ZERO);
        assert!(analysis.slippage_cost_usd > Decimal::ZERO);
        assert!(analysis.net_profit_usd < analysis.raw_profit);
    }

    #[test]
    fn test_gas_estimation() {
        let calculator = ProfitCalculator::new(ProfitCalculatorConfig::default());
        let path = create_test_path();

        let gas_with_flash = calculator.estimate_gas(&path, true);
        let gas_without_flash = calculator.estimate_gas(&path, false);

        assert!(gas_with_flash > gas_without_flash);
    }

    #[test]
    fn test_min_output_calculation() {
        let calculator = ProfitCalculator::new(ProfitCalculatorConfig::default());

        let input = U256::from(1000) * U256::exp10(18);
        let output = U256::from(1050) * U256::exp10(18);
        let gas = U256::from(200000);

        let min_output = calculator.calculate_min_output(input, output, gas);

        // 最小输出应该大于输入
        assert!(min_output > input);
        // 最小输出应该小于预期输出
        assert!(min_output < output);
    }

    #[test]
    fn test_break_even_gas_price() {
        let calculator = ProfitCalculator::new(ProfitCalculatorConfig::default());

        let input = U256::from(10000) * U256::exp10(18);
        let output = U256::from(10200) * U256::exp10(18); // 2% profit
        let gas = U256::from(300000);

        let break_even_gas = calculator.calculate_break_even_gas_price(input, output, gas);

        // 应该返回一个正的 gas 价格
        assert!(break_even_gas > Decimal::ZERO);
    }
}
