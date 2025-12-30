//! 价格计算模块 - 基于 Uniswap V3 sqrtPriceX96

use ethers::types::U256;
use rust_decimal::Decimal;
use std::str::FromStr;

/// Q96 常量 (2^96)
const Q96: u128 = 1u128 << 96;

/// 从 sqrtPriceX96 计算价格
/// 价格 = (sqrtPriceX96 / 2^96)^2
/// 返回 token1/token0 的价格
pub fn sqrt_price_x96_to_price(sqrt_price_x96: &str, decimals0: u8, decimals1: u8) -> Option<f64> {
    let sqrt_price = U256::from_dec_str(sqrt_price_x96).ok()?;

    // 转换为 f64 进行计算
    // sqrtPrice = sqrtPriceX96 / 2^96
    let sqrt_price_f64 = sqrt_price.as_u128() as f64 / Q96 as f64;

    // price = sqrtPrice^2
    let raw_price = sqrt_price_f64 * sqrt_price_f64;

    // 调整小数位数: price * 10^(decimals0 - decimals1)
    let decimal_adjustment = 10f64.powi(decimals0 as i32 - decimals1 as i32);

    Some(raw_price * decimal_adjustment)
}

/// 从 sqrtPriceX96 计算高精度价格 (使用 Decimal)
pub fn sqrt_price_x96_to_price_decimal(sqrt_price_x96: &str, decimals0: u8, decimals1: u8) -> Option<Decimal> {
    let sqrt_price_big = U256::from_dec_str(sqrt_price_x96).ok()?;

    // 使用字符串转换来保持精度
    let sqrt_price_str = sqrt_price_big.to_string();
    let sqrt_price_dec = Decimal::from_str(&sqrt_price_str).ok()?;

    let q96_dec = Decimal::from_str(&Q96.to_string()).ok()?;

    // sqrtPrice = sqrtPriceX96 / 2^96
    let sqrt_price = sqrt_price_dec / q96_dec;

    // price = sqrtPrice^2
    let raw_price = sqrt_price * sqrt_price;

    // 调整小数位数
    let decimal_diff = decimals0 as i32 - decimals1 as i32;
    let adjustment = Decimal::from(10i64.pow(decimal_diff.unsigned_abs()));

    let price = if decimal_diff >= 0 {
        raw_price * adjustment
    } else {
        raw_price / adjustment
    };

    Some(price)
}

/// 计算 V3 swap 输出金额 (精确计算)
///
/// # Arguments
/// * `amount_in` - 输入金额 (原始数值，未调整小数)
/// * `sqrt_price_x96` - 当前价格
/// * `liquidity` - 当前流动性
/// * `fee` - 费率 (如 500 表示 0.05%)
/// * `zero_for_one` - true 表示 token0 换 token1
/// * `decimals_in` - 输入代币小数位
/// * `decimals_out` - 输出代币小数位
pub fn calculate_swap_output(
    amount_in_usd: f64,
    sqrt_price_x96: &str,
    _liquidity: &str,
    fee: u32,
    zero_for_one: bool,
    decimals_in: u8,
    decimals_out: u8,
) -> Option<f64> {
    // 先计算当前价格
    let price = if zero_for_one {
        // token0 -> token1, 价格是 token1/token0
        sqrt_price_x96_to_price(sqrt_price_x96, decimals_in, decimals_out)?
    } else {
        // token1 -> token0, 需要取倒数
        1.0 / sqrt_price_x96_to_price(sqrt_price_x96, decimals_out, decimals_in)?
    };

    // 扣除手续费
    let fee_rate = fee as f64 / 1_000_000.0;
    let amount_after_fee = amount_in_usd * (1.0 - fee_rate);

    // 简化计算：假设流动性足够，直接用价格计算
    // 实际上大额交易会有滑点，但这里先简化
    let amount_out = amount_after_fee * price;

    Some(amount_out)
}

/// 计算稳定币对之间的价格偏差
/// 返回相对于 1:1 的偏差百分比
pub fn calculate_stablecoin_deviation(sqrt_price_x96: &str, decimals0: u8, decimals1: u8) -> Option<f64> {
    let price = sqrt_price_x96_to_price(sqrt_price_x96, decimals0, decimals1)?;

    // 稳定币理论价格是 1.0
    let deviation = (price - 1.0).abs() * 100.0;

    Some(deviation)
}

/// 估算滑点影响
/// 基于流动性和交易量估算滑点
pub fn estimate_slippage(
    amount_usd: f64,
    liquidity: &str,
    _sqrt_price_x96: &str,
) -> f64 {
    // 解析流动性
    let liq = U256::from_dec_str(liquidity).unwrap_or(U256::zero());
    if liq.is_zero() {
        return 1.0; // 100% 滑点
    }

    // 简化的滑点估算
    // 滑点 ≈ 交易量 / (2 * 流动性深度)
    // 这里用一个经验公式
    let liq_f64 = liq.low_u128() as f64;

    // 假设每单位流动性对应约 $10 的深度（非常粗略的估计）
    let estimated_depth = liq_f64 * 10.0 / 1e18;

    if estimated_depth <= 0.0 {
        return 0.5; // 50% 滑点作为默认值
    }

    // 滑点 = 交易量 / 深度 * 系数
    let slippage = (amount_usd / estimated_depth) * 0.5;

    // 限制在 0-50% 之间
    slippage.min(0.5).max(0.0)
}

/// 三角套利路径计算
/// 计算 A -> B -> C -> A 的最终输出
pub struct TriangleArbitrageCalculator {
    /// 第一跳价格数据
    pub pool1_sqrt_price: String,
    pub pool1_liquidity: String,
    pub pool1_fee: u32,
    pub pool1_zero_for_one: bool,
    pub pool1_decimals_in: u8,
    pub pool1_decimals_out: u8,

    /// 第二跳价格数据
    pub pool2_sqrt_price: String,
    pub pool2_liquidity: String,
    pub pool2_fee: u32,
    pub pool2_zero_for_one: bool,
    pub pool2_decimals_in: u8,
    pub pool2_decimals_out: u8,

    /// 第三跳价格数据
    pub pool3_sqrt_price: String,
    pub pool3_liquidity: String,
    pub pool3_fee: u32,
    pub pool3_zero_for_one: bool,
    pub pool3_decimals_in: u8,
    pub pool3_decimals_out: u8,
}

impl TriangleArbitrageCalculator {
    /// 计算三角套利的输出
    /// 返回 (最终输出金额, 毛利润, 滑点估计)
    pub fn calculate(&self, input_amount_usd: f64) -> Option<(f64, f64, f64)> {
        // 第一跳
        let output1 = calculate_swap_output(
            input_amount_usd,
            &self.pool1_sqrt_price,
            &self.pool1_liquidity,
            self.pool1_fee,
            self.pool1_zero_for_one,
            self.pool1_decimals_in,
            self.pool1_decimals_out,
        )?;

        let slippage1 = estimate_slippage(input_amount_usd, &self.pool1_liquidity, &self.pool1_sqrt_price);
        let output1_after_slippage = output1 * (1.0 - slippage1);

        // 第二跳
        let output2 = calculate_swap_output(
            output1_after_slippage,
            &self.pool2_sqrt_price,
            &self.pool2_liquidity,
            self.pool2_fee,
            self.pool2_zero_for_one,
            self.pool2_decimals_in,
            self.pool2_decimals_out,
        )?;

        let slippage2 = estimate_slippage(output1_after_slippage, &self.pool2_liquidity, &self.pool2_sqrt_price);
        let output2_after_slippage = output2 * (1.0 - slippage2);

        // 第三跳
        let output3 = calculate_swap_output(
            output2_after_slippage,
            &self.pool3_sqrt_price,
            &self.pool3_liquidity,
            self.pool3_fee,
            self.pool3_zero_for_one,
            self.pool3_decimals_in,
            self.pool3_decimals_out,
        )?;

        let slippage3 = estimate_slippage(output2_after_slippage, &self.pool3_liquidity, &self.pool3_sqrt_price);
        let final_output = output3 * (1.0 - slippage3);

        // 计算毛利润
        let gross_profit = final_output - input_amount_usd;

        // 总滑点估计
        let total_slippage = slippage1 + slippage2 + slippage3;

        Some((final_output, gross_profit, total_slippage))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqrt_price_to_price() {
        // USDC-WETH 池子的典型 sqrtPriceX96
        // 假设 ETH = $2000, USDC decimals = 6, WETH decimals = 18
        // sqrtPriceX96 ≈ sqrt(2000 * 10^12) * 2^96

        // DAI-USDC 池子 (都是稳定币)
        // 如果 1 DAI ≈ 1 USDC, sqrtPriceX96 ≈ 79228162514264337593543950336 (约等于 2^96)
        let sqrt_price = "79225718686740701537553";
        let price = sqrt_price_x96_to_price(sqrt_price, 18, 6);
        println!("DAI/USDC price: {:?}", price);

        // 对于稳定币对，价格应该接近 1
        if let Some(p) = price {
            assert!(p > 0.99 && p < 1.01, "Stablecoin price should be near 1.0, got {}", p);
        }
    }

    #[test]
    fn test_stablecoin_deviation() {
        let sqrt_price = "79225718686740701537553";
        let deviation = calculate_stablecoin_deviation(sqrt_price, 18, 6);
        println!("Deviation: {:?}%", deviation);

        // 稳定币偏差应该很小
        if let Some(d) = deviation {
            assert!(d < 1.0, "Stablecoin deviation should be < 1%, got {}%", d);
        }
    }
}
