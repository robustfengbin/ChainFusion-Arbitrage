use anyhow::Result;
use ethers::prelude::*;
use ethers::types::transaction::eip2718::TypedTransaction;
use rust_decimal::Decimal;
use std::sync::Arc;

/// Gas 估算器
pub struct GasEstimator {
    provider: Arc<Provider<Http>>,
    eth_price_usd: Decimal,
}

impl GasEstimator {
    pub fn new(provider: Arc<Provider<Http>>, eth_price_usd: Decimal) -> Self {
        Self {
            provider,
            eth_price_usd,
        }
    }

    /// 获取当前 gas 价格
    pub async fn get_gas_price(&self) -> Result<U256> {
        let gas_price = self.provider.get_gas_price().await?;
        Ok(gas_price)
    }

    /// 获取 EIP-1559 费用信息
    pub async fn get_eip1559_fees(&self) -> Result<(U256, U256)> {
        let (max_fee, priority_fee) = self.provider.estimate_eip1559_fees(None).await?;
        Ok((max_fee, priority_fee))
    }

    /// 估算交易 gas
    pub async fn estimate_gas(&self, tx: &TypedTransaction) -> Result<U256> {
        let gas = self.provider.estimate_gas(tx, None).await?;
        Ok(gas)
    }

    /// 计算 gas 成本 (USD)
    pub fn calculate_gas_cost_usd(&self, gas_used: U256, gas_price: U256) -> Decimal {
        // gas_cost_eth = gas_used * gas_price / 1e18
        let gas_used_decimal = Decimal::from_str_exact(&gas_used.to_string())
            .unwrap_or(Decimal::ZERO);
        let gas_price_decimal = Decimal::from_str_exact(&gas_price.to_string())
            .unwrap_or(Decimal::ZERO);

        let gas_cost_wei = gas_used_decimal * gas_price_decimal;
        let gas_cost_eth = gas_cost_wei / Decimal::from(10u128.pow(18));

        gas_cost_eth * self.eth_price_usd
    }

    /// 估算套利交易的 gas 限制
    ///
    /// 根据跳数估算 gas:
    /// - 基础消耗: 21000 (base tx)
    /// - 每跳 swap: ~150000 gas (V3) 或 ~100000 gas (V2)
    /// - 闪电贷开销: ~100000 gas
    pub fn estimate_arbitrage_gas(num_hops: u32, use_flash_loan: bool) -> U256 {
        let base_gas: u64 = 21000;
        let per_hop_gas: u64 = 150000;
        let flash_loan_gas: u64 = if use_flash_loan { 100000 } else { 0 };

        let total = base_gas + (num_hops as u64 * per_hop_gas) + flash_loan_gas;

        // 添加 20% 缓冲
        let with_buffer = total * 120 / 100;

        U256::from(with_buffer)
    }

    /// 更新 ETH 价格
    pub fn update_eth_price(&mut self, price: Decimal) {
        self.eth_price_usd = price;
    }
}

/// Gas 价格追踪器
pub struct GasPriceTracker {
    provider: Arc<Provider<Http>>,
    /// 历史 gas 价格记录
    history: Vec<(chrono::DateTime<chrono::Utc>, U256)>,
    max_history_size: usize,
}

impl GasPriceTracker {
    pub fn new(provider: Arc<Provider<Http>>) -> Self {
        Self {
            provider,
            history: Vec::new(),
            max_history_size: 100,
        }
    }

    /// 记录当前 gas 价格
    pub async fn record_gas_price(&mut self) -> Result<U256> {
        let gas_price = self.provider.get_gas_price().await?;
        let now = chrono::Utc::now();

        self.history.push((now, gas_price));

        // 保持历史记录在限制范围内
        if self.history.len() > self.max_history_size {
            self.history.remove(0);
        }

        Ok(gas_price)
    }

    /// 获取平均 gas 价格
    pub fn get_average_gas_price(&self) -> Option<U256> {
        if self.history.is_empty() {
            return None;
        }

        let sum: U256 = self.history.iter().map(|(_, p)| *p).fold(U256::zero(), |a, b| a + b);
        Some(sum / U256::from(self.history.len()))
    }

    /// 获取最近的 gas 价格
    pub fn get_latest_gas_price(&self) -> Option<U256> {
        self.history.last().map(|(_, p)| *p)
    }

    /// 判断当前 gas 价格是否高于平均值
    pub fn is_gas_price_high(&self, current: U256, threshold_multiplier: f64) -> bool {
        if let Some(avg) = self.get_average_gas_price() {
            let threshold = avg * U256::from((threshold_multiplier * 100.0) as u64) / U256::from(100);
            current > threshold
        } else {
            false
        }
    }
}
