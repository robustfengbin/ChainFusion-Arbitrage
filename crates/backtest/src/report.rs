//! 回测报告生成

use anyhow::Result;
use std::fs;
use std::path::Path;
use tracing::info;

use crate::models::BacktestStatistics;

/// 生成回测报告
pub fn generate_report(stats: &BacktestStatistics, output_dir: &str) -> Result<()> {
    fs::create_dir_all(output_dir)?;

    // 生成文本报告
    let report = format_text_report(stats);
    let report_path = Path::new(output_dir).join("backtest_report.txt");
    fs::write(&report_path, &report)?;
    info!("文本报告已保存: {:?}", report_path);

    // 生成 JSON 报告
    let json_report = serde_json::to_string_pretty(stats)?;
    let json_path = Path::new(output_dir).join("backtest_report.json");
    fs::write(&json_path, &json_report)?;
    info!("JSON 报告已保存: {:?}", json_path);

    // 打印摘要
    println!("{}", report);

    Ok(())
}

/// 格式化文本报告
fn format_text_report(stats: &BacktestStatistics) -> String {
    let mut report = String::new();

    report.push_str(&"=".repeat(80));
    report.push('\n');
    report.push_str("三角套利历史回测报告\n");
    report.push_str(&"=".repeat(80));
    report.push_str("\n\n");

    // 基本信息
    report.push_str("【回测范围】\n");
    report.push_str(&format!("  起始区块: {}\n", stats.start_block));
    report.push_str(&format!("  结束区块: {}\n", stats.end_block));
    report.push_str(&format!("  总区块数: {}\n", stats.total_blocks));
    report.push('\n');

    // 交易统计
    report.push_str("【交易统计】\n");
    report.push_str(&format!("  有交易的区块数: {}\n", stats.blocks_with_swaps));
    report.push_str(&format!("  总交易量: ${:.2}\n", stats.total_volume_usd));
    report.push_str(&format!(
        "  平均每区块交易量: ${:.2}\n",
        if stats.blocks_with_swaps > 0 {
            stats.total_volume_usd / stats.blocks_with_swaps as f64
        } else {
            0.0
        }
    ));
    report.push('\n');

    // 套利机会统计
    let total_opportunities: u64 = stats.path_stats.iter().map(|p| p.analysis_count).sum();
    let profitable_count: u64 = stats.path_stats.iter().map(|p| p.profitable_count).sum();

    report.push_str("【套利机会统计】\n");
    report.push_str(&format!("  总分析次数: {}\n", total_opportunities));
    report.push_str(&format!("  盈利次数: {}\n", profitable_count));
    report.push_str(&format!(
        "  盈利比例: {:.2}%\n",
        if total_opportunities > 0 {
            profitable_count as f64 / total_opportunities as f64 * 100.0
        } else {
            0.0
        }
    ));
    report.push('\n');

    // 各路径统计
    report.push_str("【各路径统计】\n");
    report.push_str(&"-".repeat(120));
    report.push('\n');
    report.push_str(&format!(
        "{:<50} {:>10} {:>10} {:>15} {:>15} {:>15}\n",
        "路径", "分析次数", "盈利次数", "最大利润$", "平均利润$", "总利润$"
    ));
    report.push_str(&"-".repeat(120));
    report.push('\n');

    let mut sorted_stats: Vec<_> = stats.path_stats.iter().collect();
    sorted_stats.sort_by(|a, b| b.max_profit_usd.partial_cmp(&a.max_profit_usd).unwrap());

    for path_stat in sorted_stats {
        report.push_str(&format!(
            "{:<50} {:>10} {:>10} {:>15.2} {:>15.2} {:>15.2}\n",
            truncate_string(&path_stat.path_name, 48),
            path_stat.analysis_count,
            path_stat.profitable_count,
            path_stat.max_profit_usd,
            path_stat.avg_profit_usd,
            path_stat.total_profit_usd,
        ));
    }
    report.push('\n');

    // TOP 盈利机会
    if !stats.profitable_opportunities.is_empty() {
        report.push_str("【TOP 20 盈利机会】\n");
        report.push_str(&"-".repeat(120));
        report.push('\n');

        let mut sorted_opps: Vec<_> = stats.profitable_opportunities.iter().collect();
        sorted_opps.sort_by(|a, b| b.net_profit_usd.partial_cmp(&a.net_profit_usd).unwrap());

        for (i, opp) in sorted_opps.iter().take(20).enumerate() {
            report.push_str(&format!(
                "\n{:2}. 区块 {} | {} (上海时间)\n",
                i + 1,
                opp.block_number,
                opp.datetime_shanghai
            ));
            report.push_str(&"-".repeat(80));
            report.push('\n');

            // 输出触发事件信息（用户操作）
            if let Some(ref trigger) = opp.trigger_event {
                report.push_str("\n    【用户交易】\n");
                report.push_str(&format!(
                    "    池子: {} (fee: {:.2}%) | 地址: {}\n",
                    trigger.pool_name, trigger.pool_fee_percent,
                    truncate_string(&trigger.pool_address, 42)
                ));
                report.push_str(&format!(
                    "    用户操作: 卖出 {} 买入 {} (交易量: ${:.2})\n",
                    trigger.user_sell_token, trigger.user_buy_token, trigger.pool_volume_usd
                ));
                report.push_str(&format!(
                    "    价格影响: {}\n",
                    trigger.price_impact
                ));
            }

            // 输出套利步骤详情
            if !opp.arb_steps.is_empty() {
                report.push_str("\n    【我们的套利操作】\n");
                for step in &opp.arb_steps {
                    report.push_str(&format!(
                        "    第{}步: 在 {} 池 (fee: {:.2}%)\n",
                        step.step, step.pool_name, step.fee_percent
                    ));
                    report.push_str(&format!(
                        "           {}\n",
                        step.description
                    ));
                    report.push_str(&format!(
                        "           池子地址: {}\n",
                        truncate_string(&step.pool_address, 42)
                    ));
                }
            }

            // 输出价格偏离分析
            report.push_str("\n    【价格偏离分析】\n");
            report.push_str(&format!(
                "    价格偏离: {:.4}% | 总手续费: {:.4}% | 净套利空间: {:.4}%\n",
                opp.price_deviation_percent, opp.total_fee_percent, opp.arb_spread_percent
            ));
            report.push_str(&format!(
                "    说明: 三池价格形成 {:.4}% 的偏离，扣除 {:.4}% 手续费后，剩余 {:.4}% 毛利空间\n",
                opp.price_deviation_percent, opp.total_fee_percent, opp.arb_spread_percent
            ));

            // 输出结果统计
            report.push_str("\n    【套利结果】(假设有自有资金)\n");
            report.push_str(&format!(
                "    捕获比例: {}% | 输入: ${:.2} | 输出: ${:.2}\n",
                opp.capture_percent, opp.input_amount_usd, opp.output_amount_usd
            ));
            report.push_str(&format!(
                "    毛利润: ${:.2} | Gas成本: ${:.2} | 净利润: ${:.2}\n",
                opp.gross_profit_usd, opp.gas_cost_usd, opp.net_profit_usd
            ));

            // 输出闪电贷成本分析
            report.push_str("\n    【闪电贷成本】(无自有资金，需借贷)\n");
            report.push_str(&format!(
                "    闪电贷费率: {:.2}% | 闪电贷费用: ${:.2}\n",
                opp.flash_loan_fee_percent, opp.flash_loan_fee_usd
            ));
            report.push_str(&format!(
                "    真实净利润: ${:.2} (毛利润 - Gas - 闪电贷费)\n",
                opp.real_net_profit_usd
            ));

            // 判断是否仍然盈利
            if opp.real_net_profit_usd > 0.0 {
                report.push_str(&format!(
                    "    ✅ 使用闪电贷仍可盈利，收益率: {:.4}%\n",
                    if opp.input_amount_usd > 0.0 { opp.real_net_profit_usd / opp.input_amount_usd * 100.0 } else { 0.0 }
                ));
            } else {
                report.push_str(&format!(
                    "    ❌ 使用闪电贷将亏损 ${:.2}，不建议执行\n",
                    -opp.real_net_profit_usd
                ));
            }
            report.push('\n');
        }
    }

    // 结论
    report.push_str(&"=".repeat(80));
    report.push('\n');
    report.push_str("【结论】\n");

    if profitable_count == 0 {
        report.push_str("\n❌ 在回测期间，未发现净利润 > 0 的套利机会。\n");
        report.push_str("\n可能原因:\n");
        report.push_str("1. 稳定币之间的价格偏离太小，不足以覆盖交易成本\n");
        report.push_str("2. Gas 成本过高，需要更大的价格偏离\n");
        report.push_str("3. 主流池子的套利机会被高频交易者快速捕获\n");
        report.push_str("4. 简化的价格模型忽略了实际价格波动\n");
    } else {
        let total_profit: f64 = stats.profitable_opportunities.iter().map(|o| o.net_profit_usd).sum();
        let avg_profit = total_profit / stats.profitable_opportunities.len() as f64;
        let max_profit = stats.profitable_opportunities
            .iter()
            .map(|o| o.net_profit_usd)
            .fold(f64::NEG_INFINITY, f64::max);

        report.push_str(&format!("\n✅ 发现 {} 次潜在盈利机会\n", profitable_count));
        report.push_str(&format!("  总利润: ${:.2}\n", total_profit));
        report.push_str(&format!("  平均利润: ${:.2}\n", avg_profit));
        report.push_str(&format!("  最大单次利润: ${:.2}\n", max_profit));
    }

    report.push_str(&"=".repeat(80));
    report.push('\n');

    report
}

/// 截断字符串
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
