//! 套利机会转换器
//!
//! 将 ArbitrageOpportunity 转换为 ArbitrageParams，并自动选择闪电贷池

use anyhow::{anyhow, Result};
use ethers::prelude::*;
use ethers::types::{Address, U256};
use models::{ArbitrageOpportunity, DexType};
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::info;

use crate::types::ArbitrageParams;

// 重新导出 dex crate 的闪电贷选择器
pub use dex::flashloan::{
    FlashPoolSelector, CachedFlashPoolSelector, FlashPoolSelection,
    FlashPoolSelectorConfig, V3PoolInfo,
};

/// 套利参数构建器
pub struct ArbitrageParamsBuilder<M: Middleware> {
    #[allow(dead_code)]
    provider: Arc<M>,
    flash_selector: FlashPoolSelector<M>,
    /// 默认最小利润 (wei)
    default_min_profit: U256,
}

impl<M: Middleware + 'static> ArbitrageParamsBuilder<M> {
    /// 创建新的构建器
    pub fn new(provider: Arc<M>, chain_id: u64) -> Self {
        let config = match chain_id {
            56 | 97 => FlashPoolSelectorConfig::bsc(),
            _ => FlashPoolSelectorConfig::default(),
        };

        Self {
            provider: provider.clone(),
            flash_selector: FlashPoolSelector::new(provider, config),
            default_min_profit: U256::zero(),
        }
    }

    /// 设置默认最小利润
    pub fn with_min_profit(mut self, min_profit: U256) -> Self {
        self.default_min_profit = min_profit;
        self
    }

    /// 从 ArbitrageOpportunity 构建 ArbitrageParams
    ///
    /// 自动选择最优闪电贷池
    pub async fn build_from_opportunity(
        &self,
        opportunity: &ArbitrageOpportunity,
    ) -> Result<ArbitrageParams> {
        // 验证路径长度
        if opportunity.path.hops.len() != 3 {
            return Err(anyhow!(
                "目前只支持三角套利 (3 跳), 当前路径有 {} 跳",
                opportunity.path.hops.len()
            ));
        }

        let hops = &opportunity.path.hops;

        // 提取代币
        let token_a = hops[0].token_in;
        let token_b = hops[0].token_out;
        let token_c = hops[1].token_out;

        // 验证路径闭环
        if hops[2].token_out != token_a {
            return Err(anyhow!(
                "套利路径未形成闭环: 起始={:?}, 结束={:?}",
                token_a,
                hops[2].token_out
            ));
        }

        // 提取 swap 池子地址
        let swap_pools: Vec<Address> = hops.iter().map(|h| h.pool_address).collect();

        // 提取费率
        let fee1 = hops[0].fee;
        let fee2 = hops[1].fee;
        let fee3 = hops[2].fee;

        // 选择闪电贷池
        let flash_selection = self
            .flash_selector
            .select_for_triangular(
                token_a,
                token_b,
                token_c,
                opportunity.input_amount,
                &swap_pools,
            )
            .await?;

        info!(
            "套利转换完成: {:?} -> {:?} -> {:?} -> {:?}",
            token_a, token_b, token_c, token_a
        );
        info!(
            "闪电贷池: {:?}, 费率: {}bps",
            flash_selection.pool_address,
            flash_selection.pool_fee as f64 / 100.0
        );

        Ok(ArbitrageParams {
            flash_pool: flash_selection.pool_address,
            flash_pool_fee: flash_selection.pool_fee,
            token_a,
            token_b,
            token_c,
            fee1,
            fee2,
            fee3,
            amount_in: opportunity.input_amount,
            min_profit: self.default_min_profit,
            estimated_profit_usd: opportunity.expected_profit_usd,
            estimated_gas_cost_usd: opportunity.gas_cost_usd,
            estimated_flash_fee: flash_selection.estimated_fee,
            profit_token: None,
            profit_convert_fee: 0,
            swap_pools,
        })
    }

    /// 从手动参数构建 ArbitrageParams (自动选择闪电贷池)
    pub async fn build_manual(
        &self,
        token_a: Address,
        token_b: Address,
        token_c: Address,
        fee1: u32,
        fee2: u32,
        fee3: u32,
        amount_in: U256,
        swap_pools: Vec<Address>,
        estimated_profit_usd: Decimal,
        estimated_gas_cost_usd: Decimal,
    ) -> Result<ArbitrageParams> {
        // 选择闪电贷池
        let flash_selection = self
            .flash_selector
            .select_for_triangular(token_a, token_b, token_c, amount_in, &swap_pools)
            .await?;

        Ok(ArbitrageParams {
            flash_pool: flash_selection.pool_address,
            flash_pool_fee: flash_selection.pool_fee,
            token_a,
            token_b,
            token_c,
            fee1,
            fee2,
            fee3,
            amount_in,
            min_profit: self.default_min_profit,
            estimated_profit_usd,
            estimated_gas_cost_usd,
            estimated_flash_fee: flash_selection.estimated_fee,
            profit_token: None,
            profit_convert_fee: 0,
            swap_pools,
        })
    }
}

/// 验证套利路径是否为纯 V3 路径
pub fn is_v3_only_path(opportunity: &ArbitrageOpportunity) -> bool {
    opportunity.path.hops.iter().all(|hop| {
        matches!(
            hop.dex_type,
            DexType::UniswapV3 | DexType::PancakeSwapV3 | DexType::SushiSwapV3
        )
    })
}

/// 从 ArbitrageOpportunity 提取代币列表
pub fn extract_tokens(opportunity: &ArbitrageOpportunity) -> Vec<Address> {
    let mut tokens: Vec<Address> = opportunity
        .path
        .hops
        .iter()
        .flat_map(|hop| vec![hop.token_in, hop.token_out])
        .collect();

    tokens.sort();
    tokens.dedup();
    tokens
}

/// 计算闪电贷费用 (wei)
pub fn calculate_flash_fee(amount: U256, fee_bps: u32) -> U256 {
    amount * U256::from(fee_bps) / U256::from(1_000_000)
}

/// 检查套利是否仍然盈利 (考虑闪电贷费用)
pub fn is_still_profitable(
    expected_profit: U256,
    flash_fee: U256,
    gas_cost_wei: U256,
    min_profit_wei: U256,
) -> bool {
    if expected_profit <= flash_fee + gas_cost_wei {
        return false;
    }

    let net_profit = expected_profit - flash_fee - gas_cost_wei;
    net_profit >= min_profit_wei
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_flash_fee() {
        // 1000 USDT * 0.05% = 0.5 USDT
        let amount = U256::from(1000) * U256::exp10(6); // 1000 USDT (6 decimals)
        let fee = calculate_flash_fee(amount, 500); // 0.05%
        assert_eq!(fee, U256::from(500_000)); // 0.5 USDT
    }

    #[test]
    fn test_is_still_profitable() {
        let profit = U256::from(100);
        let flash_fee = U256::from(10);
        let gas_cost = U256::from(20);
        let min_profit = U256::from(50);

        // 100 - 10 - 20 = 70 >= 50, profitable
        assert!(is_still_profitable(profit, flash_fee, gas_cost, min_profit));

        // 100 - 10 - 20 = 70 < 80, not profitable
        let high_min = U256::from(80);
        assert!(!is_still_profitable(profit, flash_fee, gas_cost, high_min));
    }
}
