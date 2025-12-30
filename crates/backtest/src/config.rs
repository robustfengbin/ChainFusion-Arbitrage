//! 回测配置

use anyhow::Result;
use std::env;

/// 回测配置
#[derive(Debug, Clone)]
pub struct BacktestConfig {
    /// 数据库连接 URL
    pub database_url: String,
    /// 以太坊 RPC URL
    pub eth_rpc_url: String,
    /// Chain ID
    pub chain_id: u64,
    /// 回测天数（默认 90 天）
    pub days: u64,
    /// 采样间隔（每 N 个区块采样一次）
    pub sample_interval: u64,
    /// 捕获比例列表
    pub capture_percentages: Vec<u32>,
    /// ETH 价格（用于计算 Gas 成本）
    pub eth_price_usd: f64,
    /// 每次 Swap 的 Gas 消耗
    pub gas_per_swap: u64,
    /// 闪电贷 Gas 消耗
    pub flash_loan_gas: u64,
    /// 基础 Gas
    pub base_gas: u64,
    /// 数据保存目录
    pub data_dir: String,
}

impl BacktestConfig {
    /// 从环境变量加载配置
    pub fn from_env() -> Result<Self> {
        dotenv::dotenv().ok();

        let db_host = env::var("DB_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let db_port = env::var("DB_PORT").unwrap_or_else(|_| "33090".to_string());
        let db_user = env::var("DB_USER").unwrap_or_else(|_| "root".to_string());
        let db_password = env::var("DB_PASSWORD").unwrap_or_default();
        let db_name = env::var("DB_NAME").unwrap_or_else(|_| "chainfusion_arbitrage".to_string());

        // URL encode password
        let encoded_password = urlencoding::encode(&db_password);

        let database_url = format!(
            "mysql://{}:{}@{}:{}/{}",
            db_user, encoded_password, db_host, db_port, db_name
        );

        let eth_rpc_url = env::var("ETH_RPC_URL")
            .unwrap_or_else(|_| "https://eth-mainnet.g.alchemy.com/v2/yourkey".to_string());

        Ok(Self {
            database_url,
            eth_rpc_url,
            chain_id: 1, // Ethereum mainnet
            days: 90,
            sample_interval: 100, // 每 100 个区块采样一次（约 20 分钟）
            capture_percentages: vec![10, 25, 50, 100],
            eth_price_usd: 3800.0,
            gas_per_swap: 180_000,
            flash_loan_gas: 150_000,
            base_gas: 21_000,
            data_dir: "backtest_data".to_string(),
        })
    }

    /// 计算总 Gas 消耗（3 跳 + 闪电贷）
    pub fn total_gas(&self) -> u64 {
        self.base_gas + self.flash_loan_gas + (3 * self.gas_per_swap)
    }

    /// 计算 Gas 成本（USD）
    pub fn gas_cost_usd(&self, gas_price_gwei: f64) -> f64 {
        let gas_cost_eth = self.total_gas() as f64 * gas_price_gwei * 1e-9;
        gas_cost_eth * self.eth_price_usd
    }
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            eth_rpc_url: String::new(),
            chain_id: 1,
            days: 90,
            sample_interval: 100,
            capture_percentages: vec![10, 25, 50, 100],
            eth_price_usd: 3800.0,
            gas_per_swap: 180_000,
            flash_loan_gas: 150_000,
            base_gas: 21_000,
            data_dir: "backtest_data".to_string(),
        }
    }
}
