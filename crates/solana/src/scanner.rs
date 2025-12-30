//! Solana 套利扫描器模块
//!
//! 支持:
//! - Jupiter 聚合器三角套利检测
//! - Raydium 池子监控
//! - WebSocket 事件订阅

use anyhow::Result;
use rust_decimal::Decimal;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{info, warn, debug, error};

use crate::client::SolanaClient;
use crate::config::SolanaConfig;
use crate::dex::jupiter::{JupiterApi, TriangleArbitrageResult};
use crate::types::{SolanaArbitrageOpportunity, SolanaDexType, known_tokens};

/// Solana 套利扫描器
pub struct SolanaArbitrageScanner {
    /// Solana 客户端
    client: Arc<SolanaClient>,
    /// Jupiter API
    jupiter: JupiterApi,
    /// 配置
    config: SolanaConfig,
    /// 监控的代币列表
    monitored_tokens: RwLock<Vec<MonitoredToken>>,
    /// 三角套利路径
    triangle_paths: RwLock<Vec<TrianglePath>>,
    /// 发现的套利机会
    opportunities: RwLock<Vec<SolanaArbitrageOpportunity>>,
    /// 是否运行中
    running: RwLock<bool>,
}

/// 监控的代币
#[derive(Debug, Clone)]
pub struct MonitoredToken {
    pub mint: Pubkey,
    pub symbol: String,
    pub decimals: u8,
    pub enabled: bool,
}

/// 三角套利路径
#[derive(Debug, Clone)]
pub struct TrianglePath {
    pub name: String,
    pub token_a: Pubkey,
    pub token_b: Pubkey,
    pub token_c: Pubkey,
    pub enabled: bool,
    pub priority: i32,
}

impl SolanaArbitrageScanner {
    /// 创建新的扫描器
    pub fn new(config: SolanaConfig) -> Result<Self> {
        let client = SolanaClient::new(config.clone())?;
        let jupiter = JupiterApi::with_url(&config.jupiter_api_url);

        info!("[Solana] 创建套利扫描器");

        Ok(Self {
            client: Arc::new(client),
            jupiter,
            config,
            monitored_tokens: RwLock::new(Vec::new()),
            triangle_paths: RwLock::new(Vec::new()),
            opportunities: RwLock::new(Vec::new()),
            running: RwLock::new(false),
        })
    }

    /// 添加监控代币
    pub async fn add_token(&self, mint: &str, symbol: &str, decimals: u8) -> Result<()> {
        let pubkey = Pubkey::from_str(mint)?;
        let token = MonitoredToken {
            mint: pubkey,
            symbol: symbol.to_string(),
            decimals,
            enabled: true,
        };

        self.monitored_tokens.write().await.push(token);
        info!("[Solana] 添加监控代币: {} ({})", symbol, mint);

        Ok(())
    }

    /// 添加三角套利路径
    pub async fn add_triangle_path(
        &self,
        name: &str,
        token_a: &str,
        token_b: &str,
        token_c: &str,
        priority: i32,
    ) -> Result<()> {
        let path = TrianglePath {
            name: name.to_string(),
            token_a: Pubkey::from_str(token_a)?,
            token_b: Pubkey::from_str(token_b)?,
            token_c: Pubkey::from_str(token_c)?,
            enabled: true,
            priority,
        };

        self.triangle_paths.write().await.push(path);
        info!("[Solana] 添加三角套利路径: {}", name);

        Ok(())
    }

    /// 初始化默认的监控配置
    pub async fn init_default_config(&self) -> Result<()> {
        // 添加主要代币
        self.add_token(
            known_tokens::WSOL,
            "WSOL",
            9,
        ).await?;

        self.add_token(
            known_tokens::USDC,
            "USDC",
            6,
        ).await?;

        self.add_token(
            known_tokens::USDT,
            "USDT",
            6,
        ).await?;

        // 添加用户请求的代币
        self.add_token(
            "EjamcKN1PixSzm3GiFgUaqCFXBMy3F51JKmbUqNF99S",
            "TARGET",
            9, // 需要从链上获取实际精度
        ).await?;

        // 添加常见的三角套利路径
        // SOL -> USDC -> USDT -> SOL
        self.add_triangle_path(
            "SOL-USDC-USDT",
            known_tokens::WSOL,
            known_tokens::USDC,
            known_tokens::USDT,
            1,
        ).await?;

        // 包含目标代币的三角路径
        // TARGET -> SOL -> USDC -> TARGET
        self.add_triangle_path(
            "TARGET-SOL-USDC",
            "EjamcKN1PixSzm3GiFgUaqCFXBMy3F51JKmbUqNF99S",
            known_tokens::WSOL,
            known_tokens::USDC,
            2,
        ).await?;

        // TARGET -> USDC -> USDT -> TARGET
        self.add_triangle_path(
            "TARGET-USDC-USDT",
            "EjamcKN1PixSzm3GiFgUaqCFXBMy3F51JKmbUqNF99S",
            known_tokens::USDC,
            known_tokens::USDT,
            3,
        ).await?;

        info!("[Solana] 默认配置初始化完成");
        Ok(())
    }

    /// 启动扫描器
    pub async fn start(&self) -> Result<()> {
        *self.running.write().await = true;
        info!("[Solana] 套利扫描器启动");

        // 健康检查
        if !self.client.health_check().await? {
            warn!("[Solana] RPC 连接不健康");
        }

        // 获取当前 slot
        let slot = self.client.get_slot().await?;
        info!("[Solana] 当前 slot: {}", slot);

        Ok(())
    }

    /// 停止扫描器
    pub async fn stop(&self) {
        *self.running.write().await = false;
        info!("[Solana] 套利扫描器停止");
    }

    /// 运行扫描循环
    pub async fn run_scan_loop(&self, interval_ms: u64) -> Result<()> {
        let interval = Duration::from_millis(interval_ms);

        info!("[Solana] 开始扫描循环，间隔 {}ms", interval_ms);

        while *self.running.read().await {
            if let Err(e) = self.scan_opportunities().await {
                error!("[Solana] 扫描出错: {}", e);
            }

            tokio::time::sleep(interval).await;
        }

        Ok(())
    }

    /// 扫描套利机会
    pub async fn scan_opportunities(&self) -> Result<()> {
        let paths = self.triangle_paths.read().await.clone();

        for path in paths.iter().filter(|p| p.enabled) {
            debug!("[Solana] 扫描路径: {}", path.name);

            // 使用不同的输入金额进行测试
            let test_amounts = vec![
                1_000_000_000u64,   // 1 SOL / 1 TOKEN
                10_000_000_000u64,  // 10 SOL / 10 TOKEN
                100_000_000_000u64, // 100 SOL / 100 TOKEN
            ];

            for amount in test_amounts {
                if let Some(result) = self.check_triangle_arbitrage(
                    &path.token_a,
                    &path.token_b,
                    &path.token_c,
                    amount,
                ).await? {
                    // 检查是否满足最小利润要求
                    let profit_usd = self.estimate_profit_usd(&result).await?;

                    if profit_usd >= self.config.min_profit_usd {
                        info!("[Solana] 发现有利可图的套利机会!");
                        info!("  路径: {}", path.name);
                        info!("  输入: {}, 输出: {}", result.input_amount, result.final_amount);
                        info!("  利润: {} ({:.4}%)", result.profit, result.profit_percent);
                        info!("  预估 USD 利润: ${:.2}", profit_usd);

                        // 保存机会
                        let opportunity = SolanaArbitrageOpportunity {
                            id: uuid::Uuid::new_v4().to_string(),
                            path_name: path.name.clone(),
                            input_token: path.token_a,
                            input_amount: result.input_amount,
                            output_amount: result.final_amount,
                            net_profit_usd: Decimal::from_f64_retain(profit_usd)
                                .unwrap_or(Decimal::ZERO),
                            dex_path: vec![SolanaDexType::Jupiter, SolanaDexType::Jupiter, SolanaDexType::Jupiter],
                            discovered_at: chrono::Utc::now().timestamp() as u64,
                            slot: self.client.cached_slot().await,
                        };

                        self.opportunities.write().await.push(opportunity);
                    }
                }
            }
        }

        Ok(())
    }

    /// 检查三角套利机会
    async fn check_triangle_arbitrage(
        &self,
        token_a: &Pubkey,
        token_b: &Pubkey,
        token_c: &Pubkey,
        input_amount: u64,
    ) -> Result<Option<TriangleArbitrageResult>> {
        let slippage_bps = (self.config.max_slippage * 10000.0) as u16;

        self.jupiter.check_triangle_arbitrage(
            token_a,
            token_b,
            token_c,
            input_amount,
            slippage_bps,
        ).await
    }

    /// 估算 USD 利润
    async fn estimate_profit_usd(&self, result: &TriangleArbitrageResult) -> Result<f64> {
        // 简化实现：假设 SOL 价格约 $100，USDC/USDT = $1
        // 实际应该从价格预言机获取
        let token_a_str = result.token_a.to_string();

        let price_per_unit = if token_a_str == known_tokens::WSOL {
            100.0 / 1_000_000_000.0  // SOL 精度 9
        } else if token_a_str == known_tokens::USDC || token_a_str == known_tokens::USDT {
            1.0 / 1_000_000.0  // USDC/USDT 精度 6
        } else {
            // 未知代币，假设价格为 $0.01 每 token
            0.01 / 1_000_000_000.0
        };

        let profit_usd = result.profit as f64 * price_per_unit;
        Ok(profit_usd)
    }

    /// 获取最新的套利机会
    pub async fn get_opportunities(&self) -> Vec<SolanaArbitrageOpportunity> {
        self.opportunities.read().await.clone()
    }

    /// 清除旧的套利机会
    pub async fn clear_old_opportunities(&self, max_age_secs: u64) {
        let now = chrono::Utc::now().timestamp() as u64;
        let mut opportunities = self.opportunities.write().await;
        opportunities.retain(|o| now - o.discovered_at < max_age_secs);
    }

    /// 获取统计信息
    pub async fn get_stats(&self) -> ScannerStats {
        let opportunities = self.opportunities.read().await;
        let paths = self.triangle_paths.read().await;
        let tokens = self.monitored_tokens.read().await;

        ScannerStats {
            total_opportunities: opportunities.len(),
            active_paths: paths.iter().filter(|p| p.enabled).count(),
            monitored_tokens: tokens.len(),
            running: *self.running.read().await,
        }
    }
}

/// 扫描器统计信息
#[derive(Debug, Clone)]
pub struct ScannerStats {
    pub total_opportunities: usize,
    pub active_paths: usize,
    pub monitored_tokens: usize,
    pub running: bool,
}

// 注意: 数据库配置的扫描器已移除以避免 sqlx 编译时检查
// 如需从数据库加载配置，请在运行时使用 SolanaArbitrageScanner 的 add_token 和 add_triangle_path 方法

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_scanner_creation() {
        let config = SolanaConfig::default();
        let scanner = SolanaArbitrageScanner::new(config);
        assert!(scanner.is_ok());
    }

    #[tokio::test]
    async fn test_add_token() {
        let config = SolanaConfig::default();
        let scanner = SolanaArbitrageScanner::new(config).unwrap();

        let result = scanner.add_token(known_tokens::WSOL, "WSOL", 9).await;
        assert!(result.is_ok());

        let tokens = scanner.monitored_tokens.read().await;
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].symbol, "WSOL");
    }

    #[tokio::test]
    async fn test_add_triangle_path() {
        let config = SolanaConfig::default();
        let scanner = SolanaArbitrageScanner::new(config).unwrap();

        let result = scanner.add_triangle_path(
            "TEST",
            known_tokens::WSOL,
            known_tokens::USDC,
            known_tokens::USDT,
            1,
        ).await;
        assert!(result.is_ok());

        let paths = scanner.triangle_paths.read().await;
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].name, "TEST");
    }
}
