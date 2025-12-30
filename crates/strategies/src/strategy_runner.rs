use anyhow::{anyhow, Result};
use ethers::prelude::*;
use ethers::signers::LocalWallet;
use ethers::types::Address;
use rust_decimal::Decimal;
use sqlx::{MySql, Pool};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tracing::{error, info, warn, debug};

use crate::arbitrage_scanner::{ArbitrageScanner, ArbitrageScannerConfig};
use crate::path_finder::PoolInfo;
use models::{ArbitrageOpportunity, ArbitrageStatus, DexType};

// 使用 executor crate 的执行器和闪电贷池选择器
use executor::{
    ArbitrageExecutor as RealExecutor, ExecutorConfig, GasStrategy, SendMode,
    ArbitrageParamsBuilder, FlashbotsConfig,
};

/// 策略配置（从数据库加载）
#[derive(Debug, Clone)]
pub struct StrategyConfig {
    pub id: i64,
    pub name: String,
    pub chain_id: u64,
    pub min_profit_threshold_usd: f64,
    pub max_slippage: f64,
    pub target_tokens: Vec<String>,
    pub target_dexes: Vec<String>,
    pub status: String,
}

/// 执行器配置
#[derive(Debug, Clone)]
pub struct ExecutorSettings {
    pub arbitrage_contract: Option<Address>,
    /// 最大 Gas 价格 (Gwei) - 支持小数，如 0.08
    pub max_gas_price_gwei: f64,
    pub use_flashbots: bool,
    pub flashbots_rpc_url: Option<String>,
    /// 是否同时使用公开 mempool（Both 模式）
    pub use_public_mempool: bool,
    pub dry_run: bool,
    /// 优先费（Gwei）- 支持小数，如 0.005
    pub priority_fee_gwei: f64,
}

impl Default for ExecutorSettings {
    fn default() -> Self {
        Self {
            arbitrage_contract: None,
            max_gas_price_gwei: 100.0,
            use_flashbots: false,
            flashbots_rpc_url: Some("https://relay.flashbots.net".to_string()),
            use_public_mempool: false,
            dry_run: true,
            priority_fee_gwei: 2.0,
        }
    }
}

/// 带优先级的套利机会 (用于优先队列)
#[derive(Debug, Clone)]
struct PrioritizedOpportunity {
    opportunity: ArbitrageOpportunity,
    /// 发现时间 (用于 TTL)
    discovered_at: std::time::Instant,
}

impl Eq for PrioritizedOpportunity {}

impl PartialEq for PrioritizedOpportunity {
    fn eq(&self, other: &Self) -> bool {
        self.opportunity.id == other.opportunity.id
    }
}

impl PartialOrd for PrioritizedOpportunity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedOpportunity {
    fn cmp(&self, other: &Self) -> Ordering {
        // 按净利润降序排列 (利润高的优先)
        self.opportunity.net_profit_usd.cmp(&other.opportunity.net_profit_usd)
    }
}

/// 异步数据库操作消息
#[allow(dead_code)]
enum DbOperation {
    SaveOpportunity {
        strategy_id: i64,
        opportunity: ArbitrageOpportunity,
    },
    UpdateOpportunityStatus {
        opportunity_id: i64,
        executed: bool,
        tx_hash: Option<String>,
        error_message: Option<String>,
    },
    SaveTradeRecord {
        strategy_id: i64,
        opportunity: ArbitrageOpportunity,
        result: models::ArbitrageResult,
    },
}

/// 单个策略运行器
pub struct ArbitrageStrategyRunner<M: Middleware> {
    strategy_id: i64,
    strategy: Arc<RwLock<StrategyConfig>>,
    db: Pool<MySql>,
    provider: Arc<M>,
    scanner: Arc<ArbitrageScanner<M>>,
    executor_settings: ExecutorSettings,
    wallet: Option<LocalWallet>,
    auto_execute: bool,

    /// 扫描循环句柄
    scan_loop_handle: Option<JoinHandle<()>>,
    /// 异步数据库写入句柄
    db_writer_handle: Option<JoinHandle<()>>,
    /// 数据库操作发送通道
    db_tx: Option<mpsc::Sender<DbOperation>>,
    /// 是否正在运行
    running: Arc<RwLock<bool>>,
    /// 机会 TTL (毫秒)
    opportunity_ttl_ms: u64,
}

impl<M: Middleware + 'static> ArbitrageStrategyRunner<M> {
    /// 创建策略运行器
    pub async fn new(
        strategy_id: i64,
        db: Pool<MySql>,
        provider: Arc<M>,
        executor_settings: ExecutorSettings,
        wallet: Option<LocalWallet>,
        auto_execute: bool,
    ) -> Result<Self> {
        // 从数据库加载策略
        let strategy_data = Self::load_strategy(&db, strategy_id).await?
            .ok_or_else(|| anyhow!("策略不存在: {}", strategy_id))?;

        // 创建扫描器配置
        let target_tokens: Vec<Address> = strategy_data
            .target_tokens
            .iter()
            .filter_map(|s| s.parse().ok())
            .collect();

        info!("策略 {} 目标代币: {:?} (原始: {:?})",
              strategy_id,
              target_tokens.iter().map(|a| format!("{:?}", a)).collect::<Vec<_>>(),
              strategy_data.target_tokens);

        let opportunity_ttl_ms = 200; // 200ms TTL

        let scanner_config = ArbitrageScannerConfig {
            scan_interval_ms: 5000, // 5秒扫描一次（兜底），减少 RPC 调用
            max_concurrent_checks: 10,
            min_profit_usd: Decimal::from_f64_retain(strategy_data.min_profit_threshold_usd)
                .unwrap_or(Decimal::from(10)),
            max_slippage: Decimal::from_f64_retain(strategy_data.max_slippage)
                .unwrap_or(Decimal::from_str_exact("0.005").unwrap()),
            target_tokens,
            opportunity_ttl_ms: 5000, // TTL 也改为 5 秒
            verify_before_execute: false, // 暂时禁用执行前验证，减少 RPC
            v3_quoter_address: None, // 禁用 V3 Quoter，使用本地计算
        };

        let scanner = Arc::new(ArbitrageScanner::new(provider.clone(), scanner_config));

        // 加载池子数据
        Self::load_pools(&db, &scanner, &strategy_data).await?;

        let strategy = Arc::new(RwLock::new(strategy_data));

        Ok(Self {
            strategy_id,
            strategy,
            db,
            provider,
            scanner,
            executor_settings,
            wallet,
            auto_execute,
            scan_loop_handle: None,
            db_writer_handle: None,
            db_tx: None,
            running: Arc::new(RwLock::new(false)),
            opportunity_ttl_ms,
        })
    }

    /// 从数据库加载策略
    async fn load_strategy(db: &Pool<MySql>, id: i64) -> Result<Option<StrategyConfig>> {
        use rust_decimal::prelude::ToPrimitive;

        let row = sqlx::query_as::<_, (i64, String, i64, Decimal, Decimal, serde_json::Value, serde_json::Value, String)>(
            r#"
            SELECT id, name, chain_id, min_profit_threshold_usd, max_slippage,
                   target_tokens, target_dexes, status
            FROM arbitrage_strategies
            WHERE id = ?
            "#
        )
        .bind(id)
        .fetch_optional(db)
        .await?;

        Ok(row.map(|(id, name, chain_id, min_profit, max_slippage, tokens, dexes, status)| {
            let target_tokens: Vec<String> = serde_json::from_value(tokens).unwrap_or_default();
            let target_dexes: Vec<String> = serde_json::from_value(dexes).unwrap_or_default();
            StrategyConfig {
                id,
                name,
                chain_id: chain_id as u64,
                min_profit_threshold_usd: min_profit.to_f64().unwrap_or(10.0),
                max_slippage: max_slippage.to_f64().unwrap_or(0.005),
                target_tokens,
                target_dexes,
                status,
            }
        }))
    }

    /// 为扫描器加载池子数据
    async fn load_pools(db: &Pool<MySql>, scanner: &ArbitrageScanner<M>, strategy: &StrategyConfig) -> Result<()> {
        let pools = sqlx::query_as::<_, (String, String, String, String, i32, String)>(
            r#"
            SELECT address, token0, token1, dex_type, fee, liquidity
            FROM pool_cache
            WHERE chain_id = ?
            LIMIT 1000
            "#
        )
        .bind(strategy.chain_id as i64)
        .fetch_all(db)
        .await?;

        let target_dexes: std::collections::HashSet<&str> = strategy.target_dexes.iter().map(|s| s.as_str()).collect();

        for (address, token0, token1, dex_type, fee, liquidity) in pools {
            if !target_dexes.is_empty() && !target_dexes.contains(dex_type.as_str()) {
                continue;
            }

            let pool_info = PoolInfo {
                address: address.parse().unwrap_or_default(),
                token0: token0.parse().unwrap_or_default(),
                token1: token1.parse().unwrap_or_default(),
                dex_type: parse_dex_type(&dex_type),
                fee: fee as u32,
                liquidity: liquidity.parse().unwrap_or_default(),
            };

            scanner.add_pool(pool_info).await;
        }

        info!("策略 {} 加载了 {} 个池子", strategy.id, scanner.pool_count().await);

        Ok(())
    }

    /// 启动策略
    pub async fn start(&mut self) -> Result<()> {
        info!("🚀 启动套利策略: {}", self.strategy_id);

        // 检查状态
        {
            let strategy = self.strategy.read().await;
            if strategy.status == "running" {
                return Err(anyhow!("策略已在运行中"));
            }
        }

        // 更新数据库状态
        sqlx::query("UPDATE arbitrage_strategies SET status = 'running', updated_at = NOW() WHERE id = ?")
            .bind(self.strategy_id)
            .execute(&self.db)
            .await?;

        // 更新内存状态
        {
            let mut strategy = self.strategy.write().await;
            strategy.status = "running".to_string();
        }

        // 设置运行标志
        {
            let mut running = self.running.write().await;
            *running = true;
        }

        // 启动异步数据库写入器
        let (db_tx, db_rx) = mpsc::channel::<DbOperation>(1000);
        self.db_tx = Some(db_tx);

        let db_for_writer = self.db.clone();
        let db_writer_handle = tokio::spawn(async move {
            Self::db_writer_loop(db_for_writer, db_rx).await;
        });
        self.db_writer_handle = Some(db_writer_handle);

        // 启动扫描器的扫描循环
        let scanner_for_scanning = self.scanner.clone();
        tokio::spawn(async move {
            if let Err(e) = scanner_for_scanning.start_scanning().await {
                error!("扫描器错误: {}", e);
            }
        });

        // 启动主循环 (优先队列处理)
        let running = self.running.clone();
        let scanner = self.scanner.clone();
        let db_tx = self.db_tx.clone();
        let strategy_id = self.strategy_id;
        let provider = self.provider.clone();
        let executor_settings = self.executor_settings.clone();
        let wallet = self.wallet.clone();
        let auto_execute = self.auto_execute;
        let opportunity_ttl_ms = self.opportunity_ttl_ms;
        let chain_id = {
            let strategy = self.strategy.read().await;
            strategy.chain_id
        };

        let handle = tokio::spawn(async move {
            Self::priority_queue_process_loop(
                running,
                scanner,
                db_tx,
                strategy_id,
                provider,
                executor_settings,
                wallet,
                auto_execute,
                opportunity_ttl_ms,
                chain_id,
            ).await;
        });

        self.scan_loop_handle = Some(handle);

        info!("✅ 套利策略启动成功: {} (优先队列模式, TTL={}ms)", self.strategy_id, self.opportunity_ttl_ms);
        Ok(())
    }

    /// 停止策略
    pub async fn stop(&mut self) -> Result<()> {
        info!("⏹️  停止套利策略: {}", self.strategy_id);

        // 设置停止标志
        {
            let mut running = self.running.write().await;
            *running = false;
        }

        // 停止扫描器
        self.scanner.stop_scanning().await;

        // 等待扫描循环结束
        if let Some(handle) = self.scan_loop_handle.take() {
            let _ = handle.await;
        }

        // 关闭数据库写入通道
        self.db_tx = None;
        if let Some(handle) = self.db_writer_handle.take() {
            let _ = handle.await;
        }

        // 更新数据库状态
        sqlx::query("UPDATE arbitrage_strategies SET status = 'stopped', updated_at = NOW() WHERE id = ?")
            .bind(self.strategy_id)
            .execute(&self.db)
            .await?;

        // 更新内存状态
        {
            let mut strategy = self.strategy.write().await;
            strategy.status = "stopped".to_string();
        }

        info!("✅ 套利策略已停止: {}", self.strategy_id);
        Ok(())
    }

    /// 异步数据库写入循环
    async fn db_writer_loop(db: Pool<MySql>, mut rx: mpsc::Receiver<DbOperation>) {
        info!("异步数据库写入器启动");

        while let Some(op) = rx.recv().await {
            match op {
                DbOperation::SaveOpportunity { strategy_id, opportunity } => {
                    if let Err(e) = Self::save_opportunity_impl(&db, strategy_id, &opportunity).await {
                        error!("异步保存机会失败: {}", e);
                    }
                }
                DbOperation::UpdateOpportunityStatus { opportunity_id, executed, tx_hash, error_message } => {
                    if let Err(e) = Self::update_opportunity_status_impl(&db, opportunity_id, executed, tx_hash, error_message).await {
                        error!("异步更新机会状态失败: {}", e);
                    }
                }
                DbOperation::SaveTradeRecord { strategy_id, opportunity, result } => {
                    if let Err(e) = Self::save_trade_record_impl(&db, strategy_id, &opportunity, &result).await {
                        error!("异步保存交易记录失败: {}", e);
                    }
                }
            }
        }

        info!("异步数据库写入器停止");
    }

    /// 优先队列处理循环
    async fn priority_queue_process_loop(
        running: Arc<RwLock<bool>>,
        scanner: Arc<ArbitrageScanner<M>>,
        db_tx: Option<mpsc::Sender<DbOperation>>,
        strategy_id: i64,
        provider: Arc<M>,
        executor_settings: ExecutorSettings,
        wallet: Option<LocalWallet>,
        auto_execute: bool,
        opportunity_ttl_ms: u64,
        chain_id: u64,
    ) {
        info!("策略 {} 优先队列处理循环启动", strategy_id);

        // 优先队列: 按利润排序
        let mut priority_queue: BinaryHeap<PrioritizedOpportunity> = BinaryHeap::new();

        loop {
            // 检查是否停止
            {
                let is_running = running.read().await;
                if !*is_running {
                    break;
                }
            }

            // 获取新发现的机会并加入优先队列
            let new_opportunities = scanner.take_opportunities().await;
            let now = std::time::Instant::now();

            for opp in new_opportunities {
                priority_queue.push(PrioritizedOpportunity {
                    opportunity: opp,
                    discovered_at: now,
                });
            }

            // 清理过期的机会
            let mut valid_queue: BinaryHeap<PrioritizedOpportunity> = BinaryHeap::new();
            while let Some(item) = priority_queue.pop() {
                if item.discovered_at.elapsed().as_millis() < opportunity_ttl_ms as u128 {
                    valid_queue.push(item);
                } else {
                    debug!("丢弃过期机会: id={}, age={}ms",
                           item.opportunity.id,
                           item.discovered_at.elapsed().as_millis());
                }
            }
            priority_queue = valid_queue;

            // 处理最高优先级的机会
            if let Some(best) = priority_queue.pop() {
                let opp = best.opportunity;

                // 检查 TTL
                if best.discovered_at.elapsed().as_millis() >= opportunity_ttl_ms as u128 {
                    debug!("机会已过期，跳过: id={}", opp.id);
                    continue;
                }

                info!("策略 {} 处理最佳机会: profit=${:.2}, age={}ms",
                      strategy_id,
                      opp.net_profit_usd,
                      best.discovered_at.elapsed().as_millis());

                // 执行前重新验证
                let verified_opp = match scanner.verify_opportunity(&opp).await {
                    Ok(Some(verified)) => verified,
                    Ok(None) => {
                        debug!("机会验证失败，跳过");
                        continue;
                    }
                    Err(e) => {
                        warn!("验证机会出错: {}", e);
                        continue;
                    }
                };

                // 异步保存机会到数据库 (不阻塞执行)
                if let Some(ref tx) = db_tx {
                    let _ = tx.send(DbOperation::SaveOpportunity {
                        strategy_id,
                        opportunity: verified_opp.clone(),
                    }).await;
                }

                // 自动执行
                if auto_execute {
                    info!("自动执行套利: strategy={}, profit=${:.2}", strategy_id, verified_opp.net_profit_usd);

                    match Self::execute_opportunity(
                        &provider,
                        &executor_settings,
                        &wallet,
                        verified_opp.clone(),
                        chain_id,
                    ).await {
                        Ok(result) => {
                            // 异步更新状态
                            if let Some(ref tx) = db_tx {
                                let _ = tx.send(DbOperation::SaveTradeRecord {
                                    strategy_id,
                                    opportunity: verified_opp,
                                    result,
                                }).await;
                            }
                        }
                        Err(e) => {
                            error!("执行套利失败: {}", e);
                        }
                    }

                    // 执行后短暂等待，避免同一区块多次尝试
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                }
            }

            // 处理间隔 (更短的间隔以快速响应)
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }

        info!("策略 {} 优先队列处理循环结束", strategy_id);
    }

    /// 执行套利机会 - 使用 Uniswap V3 闪电贷并自动选择最优池
    async fn execute_opportunity(
        provider: &Arc<M>,
        settings: &ExecutorSettings,
        wallet: &Option<LocalWallet>,
        opportunity: ArbitrageOpportunity,
        chain_id: u64,
    ) -> Result<models::ArbitrageResult> {
        // 1. 验证路径长度 (目前只支持三角套利)
        if opportunity.path.hops.len() != 3 {
            return Ok(models::ArbitrageResult {
                opportunity: opportunity.clone(),
                tx_hash: None,
                status: ArbitrageStatus::Failed,
                actual_profit: None,
                actual_gas_used: None,
                error_message: Some(format!(
                    "不支持的套利路径长度: {} (目前只支持3跳)",
                    opportunity.path.hops.len()
                )),
                executed_at: chrono::Utc::now(),
            });
        }

        // 2. 计算 min_profit (使用预期利润的 50% 作为安全边际)
        // 这样即使价格波动，也能保证至少获得预期利润的一半
        let min_profit_wei = opportunity.expected_profit / U256::from(2);
        info!(
            "💰 最小利润阈值: {} wei (预期利润 {} 的 50%)",
            min_profit_wei, opportunity.expected_profit
        );

        // 3. 使用闪电贷池选择器构建参数
        let params_builder = ArbitrageParamsBuilder::new(provider.clone(), chain_id)
            .with_min_profit(min_profit_wei);

        let hops = &opportunity.path.hops;
        let swap_pools: Vec<Address> = hops.iter().map(|h| h.pool_address).collect();

        let params = match params_builder
            .build_manual(
                hops[0].token_in,   // token_a
                hops[0].token_out,  // token_b
                hops[1].token_out,  // token_c
                hops[0].fee,        // fee1
                hops[1].fee,        // fee2
                hops[2].fee,        // fee3
                opportunity.input_amount,
                swap_pools,
                opportunity.expected_profit_usd,
                opportunity.gas_cost_usd,
            )
            .await
        {
            Ok(p) => p,
            Err(e) => {
                return Ok(models::ArbitrageResult {
                    opportunity: opportunity.clone(),
                    tx_hash: None,
                    status: ArbitrageStatus::Failed,
                    actual_profit: None,
                    actual_gas_used: None,
                    error_message: Some(format!("选择闪电贷池失败: {}", e)),
                    executed_at: chrono::Utc::now(),
                });
            }
        };

        info!(
            "闪电贷池自动选择: {:?}, 费率: {:.4}%",
            params.flash_pool,
            params.flash_pool_fee as f64 / 10000.0
        );

        // 3. 如果是 dry_run 模式，直接返回
        if settings.dry_run {
            return Ok(models::ArbitrageResult {
                opportunity: opportunity.clone(),
                tx_hash: None,
                status: ArbitrageStatus::Pending,
                actual_profit: None,
                actual_gas_used: None,
                error_message: Some("Dry run 模式".to_string()),
                executed_at: chrono::Utc::now(),
            });
        }

        // 4. 检查合约地址
        let contract_address = match settings.arbitrage_contract {
            Some(addr) => addr,
            None => {
                return Ok(models::ArbitrageResult {
                    opportunity: opportunity.clone(),
                    tx_hash: None,
                    status: ArbitrageStatus::Failed,
                    actual_profit: None,
                    actual_gas_used: None,
                    error_message: Some("未配置套利合约地址".to_string()),
                    executed_at: chrono::Utc::now(),
                });
            }
        };

        // 5. 创建执行器配置
        // 根据配置决定发送模式:
        // - Both: 同时使用 Flashbots 和公开 mempool
        // - Flashbots: 仅使用 Flashbots
        // - Normal: 仅使用公开 mempool
        let send_mode = if settings.use_flashbots && settings.use_public_mempool {
            SendMode::Both
        } else if settings.use_flashbots {
            SendMode::Flashbots
        } else {
            SendMode::Normal
        };

        let executor_config = ExecutorConfig {
            contract_address,
            chain_id,
            gas_strategy: GasStrategy {
                gas_price_multiplier: 1.2,
                max_gas_price_gwei: settings.max_gas_price_gwei,
                gas_limit_multiplier: 1.3,
                use_eip1559: true,
                priority_fee_gwei: settings.priority_fee_gwei,
                fixed_gas_limit: None, // 动态估算
            },
            confirmation_timeout_secs: 120,
            confirmations: 1,
            simulate_before_execute: true, // 先模拟再执行
            private_key: wallet.as_ref().map(|w| format!("{:?}", w)),
            send_mode,
            flashbots_config: FlashbotsConfig {
                enabled: settings.use_flashbots,
                relay_url: settings.flashbots_rpc_url.clone().unwrap_or_default(),
                chain_id,
                ..Default::default()
            },
        };

        // 6. 执行套利
        let executor = match RealExecutor::new(executor_config, provider.clone()) {
            Ok(e) => e,
            Err(e) => {
                return Ok(models::ArbitrageResult {
                    opportunity: opportunity.clone(),
                    tx_hash: None,
                    status: ArbitrageStatus::Failed,
                    actual_profit: None,
                    actual_gas_used: None,
                    error_message: Some(format!("创建执行器失败: {}", e)),
                    executed_at: chrono::Utc::now(),
                });
            }
        };

        match executor.execute(params).await {
            Ok(result) => {
                let status = if result.success {
                    ArbitrageStatus::Confirmed
                } else {
                    ArbitrageStatus::Reverted
                };

                Ok(models::ArbitrageResult {
                    opportunity,
                    tx_hash: Some(result.tx_hash),
                    status,
                    actual_profit: Some(result.profit),
                    actual_gas_used: Some(result.gas_used),
                    error_message: None,
                    executed_at: chrono::Utc::now(),
                })
            }
            Err(e) => Ok(models::ArbitrageResult {
                opportunity,
                tx_hash: None,
                status: ArbitrageStatus::Failed,
                actual_profit: None,
                actual_gas_used: None,
                error_message: Some(format!("执行失败: {:?}", e)),
                executed_at: chrono::Utc::now(),
            }),
        }
    }

    /// 保存套利机会到数据库 (内部实现)
    async fn save_opportunity_impl(db: &Pool<MySql>, strategy_id: i64, opp: &ArbitrageOpportunity) -> Result<i64> {
        let path_json = serde_json::to_value(&opp.path)?;

        let result = sqlx::query(
            r#"
            INSERT INTO arbitrage_opportunities
            (strategy_id, path, input_amount, expected_output, expected_profit_usd,
             gas_estimate, gas_cost_usd, net_profit_usd, profit_percentage,
             block_number, executed, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, FALSE, NOW())
            "#
        )
        .bind(strategy_id)
        .bind(&path_json)
        .bind(opp.input_amount.to_string())
        .bind(opp.expected_output.to_string())
        .bind(opp.expected_profit_usd.to_string())
        .bind(opp.gas_estimate.to_string())
        .bind(opp.gas_cost_usd.to_string())
        .bind(opp.net_profit_usd.to_string())
        .bind(opp.profit_percentage.to_string())
        .bind(opp.block_number as i64)
        .execute(db)
        .await?;

        Ok(result.last_insert_id() as i64)
    }

    /// 更新机会状态 (内部实现)
    async fn update_opportunity_status_impl(
        db: &Pool<MySql>,
        opportunity_id: i64,
        executed: bool,
        tx_hash: Option<String>,
        error_message: Option<String>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE arbitrage_opportunities
            SET executed = ?, tx_hash = ?, error_message = ?, updated_at = NOW()
            WHERE id = ?
            "#
        )
        .bind(executed)
        .bind(tx_hash)
        .bind(error_message)
        .bind(opportunity_id)
        .execute(db)
        .await?;

        Ok(())
    }

    /// 保存交易记录 (内部实现)
    async fn save_trade_record_impl(
        db: &Pool<MySql>,
        strategy_id: i64,
        opportunity: &ArbitrageOpportunity,
        result: &models::ArbitrageResult,
    ) -> Result<()> {
        let path_json = serde_json::to_value(&opportunity.path)?;
        let tx_hash = result.tx_hash.map(|h| format!("{:?}", h)).unwrap_or_default();

        let status = match result.status {
            ArbitrageStatus::Confirmed => "confirmed",
            ArbitrageStatus::Reverted => "reverted",
            ArbitrageStatus::Failed => "failed",
            ArbitrageStatus::Submitted => "pending",
            ArbitrageStatus::Pending => "pending",
        };

        sqlx::query(
            r#"
            INSERT INTO trade_records
            (strategy_id, tx_hash, arbitrage_type, path, input_token, input_amount,
             output_amount, profit_usd, gas_used, gas_price_gwei, gas_cost_usd,
             net_profit_usd, status, error_message, block_number, created_at)
            VALUES (?, ?, 'triangular', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NOW())
            "#
        )
        .bind(strategy_id)
        .bind(&tx_hash)
        .bind(&path_json)
        .bind(format!("{:?}", opportunity.path.start_token))
        .bind(opportunity.input_amount.to_string())
        .bind(opportunity.expected_output.to_string())
        .bind(opportunity.expected_profit_usd.to_string())
        .bind(result.actual_gas_used.map(|g| g.to_string()).unwrap_or_default())
        .bind("0")
        .bind(opportunity.gas_cost_usd.to_string())
        .bind(opportunity.net_profit_usd.to_string())
        .bind(status)
        .bind(&result.error_message)
        .bind(opportunity.block_number as i64)
        .execute(db)
        .await?;

        info!("交易记录已保存: tx={}", tx_hash);
        Ok(())
    }

    /// 获取策略ID
    pub fn get_strategy_id(&self) -> i64 {
        self.strategy_id
    }

    /// 获取策略状态
    pub async fn get_status(&self) -> String {
        let strategy = self.strategy.read().await;
        strategy.status.clone()
    }
}

/// 套利策略管理器（管理多个策略）
pub struct ArbitrageStrategyManager<M: Middleware> {
    runners: Arc<RwLock<HashMap<i64, Arc<RwLock<ArbitrageStrategyRunner<M>>>>>>,
    db: Pool<MySql>,
    provider: Arc<M>,
    #[allow(dead_code)]
    chain_id: u64,
    executor_settings: ExecutorSettings,
    wallet: Option<LocalWallet>,
    auto_execute: bool,
}

impl<M: Middleware + 'static> ArbitrageStrategyManager<M> {
    pub fn new(
        db: Pool<MySql>,
        provider: Arc<M>,
        chain_id: u64,
        executor_settings: ExecutorSettings,
        wallet: Option<LocalWallet>,
        auto_execute: bool,
    ) -> Self {
        Self {
            runners: Arc::new(RwLock::new(HashMap::new())),
            db,
            provider,
            chain_id,
            executor_settings,
            wallet,
            auto_execute,
        }
    }

    /// 启动策略
    pub async fn start_strategy(&self, strategy_id: i64) -> Result<()> {
        // 检查是否已在运行
        {
            let runners = self.runners.read().await;
            if runners.contains_key(&strategy_id) {
                return Err(anyhow!("策略已在运行中"));
            }
        }

        // 创建并启动运行器
        let mut runner = ArbitrageStrategyRunner::new(
            strategy_id,
            self.db.clone(),
            self.provider.clone(),
            self.executor_settings.clone(),
            self.wallet.clone(),
            self.auto_execute,
        ).await?;

        runner.start().await?;

        // 添加到管理列表
        let runner_arc = Arc::new(RwLock::new(runner));
        self.runners.write().await.insert(strategy_id, runner_arc);

        Ok(())
    }

    /// 停止策略
    pub async fn stop_strategy(&self, strategy_id: i64) -> Result<()> {
        let runner_arc = {
            let mut runners = self.runners.write().await;
            runners.remove(&strategy_id)
        };

        if let Some(runner_arc) = runner_arc {
            let mut runner = runner_arc.write().await;
            runner.stop().await?;
            info!("✅ 策略 {} 已停止", strategy_id);
            Ok(())
        } else {
            // 内存中没有，检查数据库状态
            let status: Option<String> = sqlx::query_scalar(
                "SELECT status FROM arbitrage_strategies WHERE id = ?"
            )
            .bind(strategy_id)
            .fetch_optional(&self.db)
            .await?;

            match status {
                Some(s) if s == "running" => {
                    // 同步数据库状态
                    sqlx::query("UPDATE arbitrage_strategies SET status = 'stopped', updated_at = NOW() WHERE id = ?")
                        .bind(strategy_id)
                        .execute(&self.db)
                        .await?;
                    info!("策略 {} 状态已同步为 stopped", strategy_id);
                    Ok(())
                }
                Some(_) => Ok(()),
                None => Err(anyhow!("策略 {} 不存在", strategy_id)),
            }
        }
    }

    /// 获取所有运行中的策略ID
    pub async fn get_running_strategy_ids(&self) -> Vec<i64> {
        let runners = self.runners.read().await;
        runners.keys().cloned().collect()
    }

    /// 停止所有策略
    pub async fn stop_all(&self) -> Result<()> {
        let strategy_ids: Vec<i64> = {
            let runners = self.runners.read().await;
            runners.keys().cloned().collect()
        };

        for id in strategy_ids {
            if let Err(e) = self.stop_strategy(id).await {
                error!("停止策略 {} 失败: {}", id, e);
            }
        }

        Ok(())
    }
}

/// 解析 DEX 类型字符串
fn parse_dex_type(s: &str) -> DexType {
    match s.to_lowercase().as_str() {
        "uniswap_v2" | "uniswapv2" => DexType::UniswapV2,
        "uniswap_v3" | "uniswapv3" => DexType::UniswapV3,
        "uniswap_v4" | "uniswapv4" => DexType::UniswapV4,
        "curve" => DexType::Curve,
        "pancakeswap_v2" | "pancakeswapv2" => DexType::PancakeSwapV2,
        "pancakeswap_v3" | "pancakeswapv3" => DexType::PancakeSwapV3,
        "sushiswap" => DexType::SushiSwap,
        "sushiswap_v2" | "sushiswapv2" => DexType::SushiSwapV2,
        "sushiswap_v3" | "sushiswapv3" => DexType::SushiSwapV3,
        _ => DexType::UniswapV2,
    }
}
