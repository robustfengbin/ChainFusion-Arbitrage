//! Solana 配置模块

use serde::{Deserialize, Serialize};
use std::env;

/// Solana 链配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaConfig {
    /// RPC URL
    pub rpc_url: String,
    /// WebSocket URL
    pub ws_url: String,
    /// 是否启用
    pub enabled: bool,
    /// 链名称
    pub name: String,
    /// 私钥 (可选，用于执行交易)
    pub private_key: Option<String>,
    /// 最小利润阈值 (USD)
    pub min_profit_usd: f64,
    /// 最大滑点 (如 0.01 = 1%)
    pub max_slippage: f64,
    /// Jupiter API URL
    pub jupiter_api_url: String,
}

impl Default for SolanaConfig {
    fn default() -> Self {
        Self {
            rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
            ws_url: "wss://api.mainnet-beta.solana.com".to_string(),
            enabled: false,
            name: "Solana".to_string(),
            private_key: None,
            min_profit_usd: 1.0,
            max_slippage: 0.01,
            jupiter_api_url: "https://quote-api.jup.ag/v6".to_string(),
        }
    }
}

impl SolanaConfig {
    /// 从环境变量加载配置
    pub fn from_env() -> Self {
        let enabled = env::var("SOLANA_ENABLED")
            .unwrap_or_else(|_| "false".to_string())
            .parse()
            .unwrap_or(false);

        Self {
            rpc_url: env::var("SOLANA_RPC_URL")
                .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string()),
            ws_url: env::var("SOLANA_WS_URL")
                .unwrap_or_else(|_| "wss://api.mainnet-beta.solana.com".to_string()),
            enabled,
            name: "Solana".to_string(),
            private_key: env::var("SOLANA_PRIVATE_KEY").ok(),
            min_profit_usd: env::var("SOLANA_MIN_PROFIT_USD")
                .unwrap_or_else(|_| "1.0".to_string())
                .parse()
                .unwrap_or(1.0),
            max_slippage: env::var("SOLANA_MAX_SLIPPAGE")
                .unwrap_or_else(|_| "0.01".to_string())
                .parse()
                .unwrap_or(0.01),
            jupiter_api_url: env::var("JUPITER_API_URL")
                .unwrap_or_else(|_| "https://quote-api.jup.ag/v6".to_string()),
        }
    }

    /// 验证配置是否有效
    pub fn is_valid(&self) -> bool {
        !self.rpc_url.is_empty() && !self.ws_url.is_empty()
    }
}

/// Solana 套利策略配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaArbitrageConfig {
    /// 目标代币列表 (mint 地址)
    pub target_tokens: Vec<String>,
    /// 监控的池子列表
    pub monitored_pools: Vec<String>,
    /// 三角套利组合
    pub triangles: Vec<SolanaTriangleConfig>,
    /// 最小交易金额 (USD)
    pub min_trade_amount_usd: f64,
    /// 最大交易金额 (USD)
    pub max_trade_amount_usd: f64,
    /// 是否使用 Jupiter 聚合
    pub use_jupiter: bool,
}

impl Default for SolanaArbitrageConfig {
    fn default() -> Self {
        Self {
            target_tokens: vec![
                "So11111111111111111111111111111111111111112".to_string(), // WSOL
                "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(), // USDC
                "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB".to_string(), // USDT
            ],
            monitored_pools: vec![],
            triangles: vec![],
            min_trade_amount_usd: 100.0,
            max_trade_amount_usd: 10000.0,
            use_jupiter: true,
        }
    }
}

/// 三角套利组合配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaTriangleConfig {
    pub name: String,
    pub token_a: String,
    pub token_b: String,
    pub token_c: String,
    pub priority: i32,
}
