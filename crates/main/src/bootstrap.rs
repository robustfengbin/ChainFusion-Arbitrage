//! 应用启动引导模块
//!
//! 封装应用初始化、服务启动和关闭逻辑
//! 支持多链并行运行 (EVM + Solana)

use anyhow::Result;
use config_crate::{AppConfig, ChainConfig};
use ethers::prelude::*;
use ethers::signers::LocalWallet;
use models::DexType;
use rust_decimal::Decimal;
use services::{
    BlockSubscriber, BlockSubscriberConfig, Database, PriceService, PriceServiceConfig,
    ArbitrageConfigDb,
};
use std::collections::HashMap;
use std::sync::Arc;
use strategies::{
    ArbitrageStrategyManager, EventDrivenScanner, EventDrivenScannerConfig, ExecutorSettings,
    PoolState, ChainContractsConfig,
};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};
use ::utils::{RpcStatsProvider, StatsHttp};

// Solana 模块
use solana_arb::{SolanaConfig, EventDrivenSolanaScanner};

/// 单链服务句柄
pub struct ChainServiceHandles {
    #[allow(dead_code)]
    pub chain_id: u64,
    pub chain_name: String,
    pub block_handle: Option<JoinHandle<()>>,
    pub scanner_handle: Option<JoinHandle<()>>,
}

/// 应用程序实例
///
/// 管理所有服务的生命周期，支持多链 (EVM + Solana)
pub struct Application {
    config: AppConfig,
    database: Database,
    /// 主链 (ETH) 的 RPC Provider - 保持向后兼容
    rpc_stats_provider: RpcStatsProvider,
    /// 各链的 RPC Provider
    #[allow(dead_code)]
    chain_providers: HashMap<u64, Arc<Provider<Http>>>,
    price_service: Arc<PriceService>,
    strategy_manager: Arc<ArbitrageStrategyManager<Provider<StatsHttp>>>,

    // 后台任务句柄
    price_handle: JoinHandle<()>,
    /// 各链的服务句柄
    chain_handles: Vec<ChainServiceHandles>,
    // 保持向后兼容
    block_handle: Option<JoinHandle<()>>,
    event_scanner_handle: Option<JoinHandle<()>>,

    /// Solana 扫描器句柄
    solana_scanner_handle: Option<JoinHandle<()>>,
}

impl Application {
    /// 初始化并启动应用
    pub async fn start() -> Result<Self> {
        // 加载配置
        info!("加载配置文件...");
        let config = AppConfig::load()?;
        Self::log_config(&config);

        // 初始化数据库
        let database = Self::init_database(&config).await?;

        // 初始化钱包
        let wallet = Self::init_wallet(&config);

        // 检查配置
        Self::check_config(&config);

        // 创建主链 Provider（带 RPC 统计）- 保持向后兼容
        info!("初始化以太坊 Provider（带 RPC 统计）...");
        let log_interval_secs = 10;
        let rpc_stats_provider = RpcStatsProvider::new(&config.ethereum.rpc_url, log_interval_secs)?;
        let eth_provider = rpc_stats_provider.provider();

        // 创建各链的 Provider
        let mut chain_providers: HashMap<u64, Arc<Provider<Http>>> = HashMap::new();
        for chain_id in &config.enabled_chains {
            if let Some(chain_config) = config.chains.get(chain_id) {
                if chain_config.enabled {
                    match Provider::<Http>::try_from(&chain_config.rpc_url) {
                        Ok(provider) => {
                            chain_providers.insert(*chain_id, Arc::new(provider));
                            info!("[{}] Provider 创建成功: {}", chain_config.name, chain_config.rpc_url);
                        }
                        Err(e) => {
                            warn!("[{}] Provider 创建失败: {}", chain_config.name, e);
                        }
                    }
                }
            }
        }

        // 启动价格服务
        let price_service = Arc::new(PriceService::new(PriceServiceConfig {
            update_interval_secs: 10,
            ..Default::default()
        }));
        let price_handle = Self::spawn_price_service(price_service.clone());

        // 启动各链的事件驱动服务
        let mut chain_handles = Vec::new();

        for chain_id in &config.enabled_chains {
            if let Some(chain_config) = config.chains.get(chain_id) {
                if !chain_config.enabled {
                    continue;
                }

                info!("========================================");
                info!("启动 {} (chain_id={}) 链服务...", chain_config.name, chain_id);
                info!("========================================");

                if let Some(provider) = chain_providers.get(chain_id) {
                    let (block_handle, scanner_handle) = Self::start_chain_services(
                        chain_config,
                        &config,
                        &database,
                        provider.clone(),
                        price_service.clone(),
                        wallet.clone(),
                    )
                    .await;

                    chain_handles.push(ChainServiceHandles {
                        chain_id: *chain_id,
                        chain_name: chain_config.name.clone(),
                        block_handle,
                        scanner_handle,
                    });
                }
            }
        }

        // 创建策略管理器 (暂时只支持主链)
        let strategy_manager = Self::create_strategy_manager(
            &config,
            &database,
            eth_provider.clone(),
            wallet,
        );

        // 启动 Solana 扫描器（如果启用）
        let solana_scanner_handle = Self::start_solana_scanner(&config).await;

        Ok(Self {
            config,
            database,
            rpc_stats_provider,
            chain_providers,
            price_service,
            strategy_manager,
            price_handle,
            chain_handles,
            block_handle: None,  // 已废弃，使用 chain_handles
            event_scanner_handle: None,  // 已废弃，使用 chain_handles
            solana_scanner_handle,
        })
    }

    /// 运行 API 服务器（阻塞）
    pub async fn run_server(&self) {
        let app_state = api::AppState::new(
            // 需要 clone database，因为 AppState::new 需要所有权
            Database::from_pool(self.database.pool().clone()),
            self.strategy_manager.clone(),
        );

        let app = api::create_server(
            app_state,
            &self.config.api.host,
            self.config.api.port,
        )
        .await;

        self.log_startup_complete();

        // 启动服务器（阻塞）
        api::start_server(app, &self.config.api.host, self.config.api.port).await;
    }

    /// 停止所有服务
    pub async fn shutdown(mut self) -> Result<()> {
        info!("正在停止服务...");

        self.strategy_manager.stop_all().await?;
        self.price_service.stop().await;
        self.rpc_stats_provider.stop();

        let _ = self.price_handle.await;

        // 停止所有 EVM 链的服务
        for handles in self.chain_handles {
            info!("停止 {} 链服务...", handles.chain_name);
            if let Some(handle) = handles.block_handle {
                let _ = handle.await;
            }
            if let Some(handle) = handles.scanner_handle {
                let _ = handle.await;
            }
        }

        // 停止 Solana 扫描器
        if let Some(handle) = self.solana_scanner_handle {
            info!("停止 Solana 扫描器...");
            let _ = handle.await;
        }

        info!("系统已停止");
        Ok(())
    }

    // ========== 私有辅助方法 ==========

    fn log_config(config: &AppConfig) {
        info!("配置加载成功");
        info!("========================================");
        info!("启用的链: {:?}", config.enabled_chains);
        for chain_id in &config.enabled_chains {
            if let Some(chain_config) = config.chains.get(chain_id) {
                info!("  [{}] chain_id={}", chain_config.name, chain_id);
                info!("    RPC: {}", chain_config.rpc_url);
                info!("    WS:  {}", chain_config.ws_url);
            }
        }
        info!("========================================");
        info!("最大滑点: {}%", config.arbitrage.max_slippage * 100.0);
        info!("最低利润阈值: ${}", config.arbitrage.min_profit_threshold);
        info!("闪电贷提供商: {:?}", config.flash_loan.provider);
    }

    async fn init_database(config: &AppConfig) -> Result<Database> {
        info!("初始化数据库连接...");
        let database = Database::new(&config.database.url, config.database.max_connections).await?;
        database.initialize_tables().await?;

        // 初始化套利配置（代币、三角组合、池子、池子-路径映射）
        let config_db = ArbitrageConfigDb::new(database.pool().clone());

        // ETH Mainnet 配置
        if config.enabled_chains.contains(&1) {
            info!("初始化 Ethereum Mainnet 配置...");
            config_db.init_default_tokens().await?;
            config_db.init_default_triangles().await?;
            config_db.init_default_pools().await?;
            config_db.init_pool_paths().await?;
        }

        // BSC Mainnet 配置
        if config.enabled_chains.contains(&56) {
            info!("初始化 BSC Mainnet 配置...");
            config_db.init_bsc_default_tokens().await?;
            config_db.init_bsc_default_triangles().await?;
            config_db.init_bsc_default_pools().await?;
            config_db.init_bsc_pool_paths().await?;
        }

        info!("数据库初始化完成");
        Ok(database)
    }

    fn init_wallet(config: &AppConfig) -> Option<LocalWallet> {
        if let Some(ref private_key) = config.wallet.private_key {
            match private_key.parse::<LocalWallet>() {
                Ok(w) => {
                    let w = w.with_chain_id(config.ethereum.chain_id);
                    info!("✅ 钱包私钥已配置");
                    Some(w)
                }
                Err(e) => {
                    warn!("⚠️  解析私钥失败: {} - 将以只读模式运行", e);
                    None
                }
            }
        } else {
            warn!("⚠️  钱包私钥未配置 - 系统将以只读模式运行");
            None
        }
    }

    fn check_config(config: &AppConfig) {
        if config.wallet.arbitrage_contract_address.is_some() {
            info!(
                "✅ 套利合约地址已配置: {}",
                config.wallet.arbitrage_contract_address.as_ref().unwrap()
            );
        } else {
            warn!("⚠️  套利合约未配置 - 无法执行交易");
        }

        if config.mev.use_flashbots {
            info!("✅ Flashbots MEV 保护已启用");
        } else {
            info!("⚠️  Flashbots 未启用 - 交易可能被抢跑");
        }
    }

    fn spawn_price_service(service: Arc<PriceService>) -> JoinHandle<()> {
        info!("启动价格服务...");
        tokio::spawn(async move {
            if let Err(e) = service.start().await {
                error!("价格服务错误: {}", e);
            }
        })
    }

    /// 启动单链的事件驱动服务 (区块订阅器 + 套利扫描器)
    async fn start_chain_services(
        chain_config: &ChainConfig,
        app_config: &AppConfig,
        database: &Database,
        provider: Arc<Provider<Http>>,
        price_service: Arc<PriceService>,
        wallet: Option<LocalWallet>,
    ) -> (Option<JoinHandle<()>>, Option<JoinHandle<()>>) {
        let chain_id = chain_config.chain_id;
        let chain_name = &chain_config.name;

        if chain_config.ws_url.is_empty() {
            warn!("[{}] ⚠️ 未配置 WebSocket URL - 区块订阅器未启动", chain_name);
            return (None, None);
        }

        // 获取链合约配置
        let chain_contracts = match ChainContractsConfig::for_chain(chain_id) {
            Some(contracts) => contracts,
            None => {
                warn!("[{}] ⚠️ 不支持的链 chain_id={}", chain_name, chain_id);
                return (None, None);
            }
        };

        // 从 arbitrage_pools 表加载套利池子
        info!("[{}] 从数据库加载套利池子配置...", chain_name);
        let pools = Self::load_arbitrage_pools(database, chain_id).await;

        let monitored_pool_addresses: Vec<ethers::types::Address> = pools
            .iter()
            .filter_map(|p| p.address.parse().ok())
            .collect();

        info!("[{}] 将监控 {} 个池子的 Swap 事件", chain_name, monitored_pool_addresses.len());

        // 创建区块订阅器
        info!("[{}] 启动区块订阅器...", chain_name);
        let block_subscriber = Arc::new(BlockSubscriber::new(BlockSubscriberConfig {
            ws_url: chain_config.ws_url.clone(),
            chain_id,
            reconnect_delay_secs: 5,
            monitored_pools: monitored_pool_addresses,
        }));

        let swap_rx = block_subscriber.subscribe_swaps();
        let block_rx = block_subscriber.subscribe_blocks();

        let subscriber = block_subscriber.clone();
        let chain_name_for_block = chain_name.clone();
        let block_handle = tokio::spawn(async move {
            if let Err(e) = subscriber.start().await {
                error!("[{}] 区块订阅器错误: {}", chain_name_for_block, e);
            }
        });

        // 从数据库加载目标代币配置
        info!("[{}] 从数据库加载套利代币配置...", chain_name);
        let config_db = ArbitrageConfigDb::new(database.pool().clone());
        let target_tokens: Vec<ethers::types::Address> = match config_db.get_enabled_tokens(chain_id).await {
            Ok(tokens) => {
                let addrs: Vec<ethers::types::Address> = tokens
                    .iter()
                    .filter_map(|t| t.address.parse().ok())
                    .collect();
                info!("[{}] 从数据库加载了 {} 个目标代币: {:?}",
                    chain_name,
                    addrs.len(),
                    tokens.iter().map(|t| &t.symbol).collect::<Vec<_>>()
                );
                addrs
            }
            Err(e) => {
                warn!("[{}] 加载代币配置失败: {}, 该链暂无代币配置", chain_name, e);
                vec![]
            }
        };

        // 如果没有配置任何代币，跳过该链
        if target_tokens.is_empty() {
            warn!("[{}] ⚠️ 没有配置任何代币，跳过扫描器启动", chain_name);
            return (Some(block_handle), None);
        }

        // 创建事件驱动扫描器
        info!("[{}] 启动事件驱动套利扫描器...", chain_name);
        let min_swap_value = Decimal::from_f64_retain(app_config.arbitrage.min_swap_value_usd)
            .unwrap_or_else(|| Decimal::from(1));
        let skip_local_calc_threshold = Decimal::from_f64_retain(app_config.arbitrage.skip_local_calc_threshold_usd)
            .unwrap_or_else(|| Decimal::from(5000));

        // 构建动态利润门槛配置
        let dynamic_profit_config = strategies::DynamicProfitConfig {
            ultra_low_gas_min_profit: Decimal::from_f64_retain(app_config.arbitrage.min_profit_ultra_low_gas)
                .unwrap_or_else(|| Decimal::from(1)),
            low_gas_min_profit: Decimal::from_f64_retain(app_config.arbitrage.min_profit_low_gas)
                .unwrap_or_else(|| Decimal::from(3)),
            normal_gas_min_profit: Decimal::from_f64_retain(app_config.arbitrage.min_profit_normal_gas)
                .unwrap_or_else(|| Decimal::from(5)),
            high_gas_min_profit: Decimal::from_f64_retain(app_config.arbitrage.min_profit_high_gas)
                .unwrap_or_else(|| Decimal::from(15)),
            very_high_gas_min_profit: Decimal::from_f64_retain(app_config.arbitrage.min_profit_very_high_gas)
                .unwrap_or_else(|| Decimal::from(30)),
        };

        // 构建执行器配置
        let auto_execute = app_config.arbitrage.auto_execute.unwrap_or(false);
        let dry_run = app_config.arbitrage.dry_run.unwrap_or(true);

        let executor_config = strategies::ScannerExecutorConfig {
            auto_execute,
            arbitrage_contract: app_config.wallet.arbitrage_contract_address
                .as_ref()
                .and_then(|s| s.parse().ok()),
            max_gas_price_gwei: app_config.arbitrage.max_gas_price_gwei.unwrap_or(0.08),
            use_flashbots: app_config.mev.use_flashbots,
            flashbots_rpc_url: if app_config.mev.use_flashbots {
                Some(app_config.mev.flashbots_rpc.clone()
                    .unwrap_or_else(|| "https://relay.flashbots.net".to_string()))
            } else {
                None
            },
            use_public_mempool: app_config.mev.use_public_mempool,
            dry_run,
            priority_fee_gwei: app_config.mev.priority_fee_gwei.unwrap_or(0.005),
            // 默认使用 80% 的最优输入金额
            amount_strategy: strategies::ExecutionAmountStrategy::Percentage(0.8),
            simulate_before_execute: true,
        };

        // 输出配置
        info!("[{}] 📊 套利配置:", chain_name);
        info!("[{}]    最大滑点: {}% ({})", chain_name, app_config.arbitrage.max_slippage * 100.0, app_config.arbitrage.max_slippage);
        info!("[{}]    最小交易金额过滤阈值: ${}", chain_name, min_swap_value);
        info!("[{}]    跳过本地计算阈值: ${} (超过此金额直接链上计算)", chain_name, skip_local_calc_threshold);
        info!("[{}]    自动执行: {}", chain_name, auto_execute);
        info!("[{}]    干运行模式: {}", chain_name, dry_run);
        info!("[{}]    使用Flashbots: {}", chain_name, app_config.mev.use_flashbots);
        info!("[{}]    使用公开Mempool: {}", chain_name, app_config.mev.use_public_mempool);
        // 计算发送模式
        let send_mode_desc = if app_config.mev.use_flashbots && app_config.mev.use_public_mempool {
            "Both (同时发送到Flashbots和公开Mempool)"
        } else if app_config.mev.use_flashbots {
            "Flashbots (仅私密发送)"
        } else {
            "Normal (仅公开Mempool)"
        };
        info!("[{}]    发送模式: {}", chain_name, send_mode_desc);
        if let Some(ref addr) = app_config.wallet.arbitrage_contract_address {
            info!("[{}]    套利合约: {}", chain_name, addr);
        }

        // 从配置读取最大滑点
        // max_slippage 表示允许的最大价格偏差比例，例如:
        // - 0.005 = 0.5% 滑点
        // - 0.001 = 0.1% 滑点
        // 如果实际执行价格与预期价格偏差超过此值，交易将被拒绝
        let max_slippage = Decimal::from_f64_retain(app_config.arbitrage.max_slippage)
            .unwrap_or_else(|| Decimal::new(5, 3)); // 默认 0.5%

        let scanner_config = EventDrivenScannerConfig {
            chain_id,
            min_profit_usd: Decimal::from(1),
            max_slippage,
            target_tokens,
            fallback_scan_interval_ms: 5000,
            price_change_threshold: Decimal::new(1, 3), // 价格变化阈值 0.1%，触发重新扫描
            dynamic_profit_config,
            enable_dynamic_profit: true,
            min_swap_value_usd: min_swap_value,
            skip_local_calc_threshold_usd: skip_local_calc_threshold,
            executor_config,
            max_concurrent_handlers: 5, // 最多同时处理 5 个 swap 事件
        };

        // 使用链特定的合约配置创建扫描器
        let event_scanner = Arc::new(EventDrivenScanner::with_chain_config(
            scanner_config,
            provider,
            price_service,
            chain_contracts,
        ));

        // 加载代币配置到 scanner
        match config_db.get_enabled_tokens(chain_id).await {
            Ok(tokens) => {
                let token_configs: Vec<strategies::TokenConfig> = tokens
                    .iter()
                    .filter_map(|t| {
                        let address: ethers::types::Address = t.address.parse().ok()?;
                        let optimal_input = ethers::types::U256::from_dec_str(&t.optimal_input_amount).ok()?;
                        Some(strategies::TokenConfig {
                            address,
                            symbol: t.symbol.clone(),
                            decimals: t.decimals as u8,
                            is_stable: t.is_stable,
                            price_symbol: t.price_symbol.clone(),
                            optimal_input_amount: optimal_input,
                        })
                    })
                    .collect();
                event_scanner.add_token_configs(token_configs).await;
                info!("[{}] 加载了 {} 个代币配置到扫描器", chain_name, tokens.len());
            }
            Err(e) => {
                warn!("[{}] 加载代币配置到扫描器失败: {}", chain_name, e);
            }
        }

        // 加载三角套利组合配置到扫描器
        info!("[{}] 从数据库加载三角套利组合配置...", chain_name);
        match config_db.get_enabled_triangles(chain_id).await {
            Ok(triangles) => {
                let triangle_configs: Vec<strategies::TriangleConfig> = triangles
                    .iter()
                    .filter_map(|t| {
                        let token_a: ethers::types::Address = t.token_a.parse().ok()?;
                        let token_b: ethers::types::Address = t.token_b.parse().ok()?;
                        let token_c: ethers::types::Address = t.token_c.parse().ok()?;
                        Some(strategies::TriangleConfig {
                            name: t.name.clone(),
                            token_a,
                            token_b,
                            token_c,
                            priority: t.priority,
                            category: t.category.clone(),
                        })
                    })
                    .collect();
                let count = triangle_configs.len();
                event_scanner.add_triangle_configs(triangle_configs).await;
                info!("[{}] 加载了 {} 个三角套利组合配置到扫描器", chain_name, count);
            }
            Err(e) => {
                warn!("[{}] 加载三角套利组合配置失败: {}", chain_name, e);
            }
        }

        // 加载池子-路径映射配置到扫描器
        info!("[{}] 从数据库加载池子-路径映射配置...", chain_name);
        match config_db.get_all_pool_paths(chain_id).await {
            Ok(pool_paths) => {
                info!("[{}] 从数据库读取到 {} 条路径配置", chain_name, pool_paths.len());

                // 按 trigger_pool 分组
                let mut mappings: std::collections::HashMap<ethers::types::Address, Vec<strategies::PoolPathConfig>> =
                    std::collections::HashMap::new();

                let mut skipped = 0u32;
                for path in pool_paths {
                    let trigger_pool: ethers::types::Address = match path.trigger_pool.parse() {
                        Ok(addr) => addr,
                        Err(_) => { skipped += 1; continue; }
                    };
                    let token_a: ethers::types::Address = match path.token_a.parse() {
                        Ok(addr) => addr,
                        Err(_) => { skipped += 1; continue; }
                    };
                    let token_b: ethers::types::Address = match path.token_b.parse() {
                        Ok(addr) => addr,
                        Err(_) => { skipped += 1; continue; }
                    };
                    let token_c: ethers::types::Address = match path.token_c.parse() {
                        Ok(addr) => addr,
                        Err(_) => { skipped += 1; continue; }
                    };

                    let path_config = strategies::PoolPathConfig {
                        path_name: path.path_name,
                        triangle_name: path.triangle_name,
                        token_a,
                        token_b,
                        token_c,
                        priority: path.priority,
                    };

                    mappings.entry(trigger_pool)
                        .or_insert_with(Vec::new)
                        .push(path_config);
                }

                if skipped > 0 {
                    warn!("[{}] ⚠️ 跳过了 {} 条无效的路径配置", chain_name, skipped);
                }

                let pool_count = mappings.len();
                let path_count: usize = mappings.values().map(|v| v.len()).sum();

                let mappings_list: Vec<(ethers::types::Address, Vec<strategies::PoolPathConfig>)> =
                    mappings.into_iter().collect();
                event_scanner.add_pool_path_mappings(mappings_list).await;

                info!("[{}] 加载了 {} 个池子的 {} 条路径映射到扫描器", chain_name, pool_count, path_count);
            }
            Err(e) => {
                warn!("[{}] 加载池子-路径映射失败: {}", chain_name, e);
            }
        }

        // 加载套利池子到扫描器
        let pool_count = Self::load_pools_to_scanner_generic(&event_scanner, pools).await;
        info!("[{}] 事件驱动扫描器已加载 {} 个套利池子", chain_name, pool_count);

        // 如果启用了自动执行并且有钱包，设置钱包到扫描器
        if app_config.arbitrage.auto_execute.unwrap_or(false) {
            if let (Some(w), Some(pk)) = (wallet, app_config.wallet.private_key.clone()) {
                // 克隆钱包并设置正确的 chain_id
                let chain_wallet = w.with_chain_id(chain_id);
                event_scanner.set_wallet(chain_wallet, pk).await;
                info!("[{}] ✅ 钱包已设置到扫描器，自动执行已启用", chain_name);
            } else {
                warn!("[{}] ⚠️ 自动执行已启用但钱包未配置，将以干运行模式运行", chain_name);
            }
        }

        let scanner = event_scanner.clone();
        let chain_name_for_scanner = chain_name.clone();
        let scanner_handle = tokio::spawn(async move {
            if let Err(e) = scanner.start(swap_rx, block_rx).await {
                error!("[{}] 事件驱动扫描器错误: {}", chain_name_for_scanner, e);
            }
        });

        (Some(block_handle), Some(scanner_handle))
    }

    /// 加载套利池子到扫描器 (泛型版本)
    async fn load_pools_to_scanner_generic<M: Middleware + 'static>(
        scanner: &EventDrivenScanner<M>,
        pools: Vec<services::ArbitragePoolConfig>,
    ) -> usize {
        let mut count = 0;
        for pool in pools {
            let parsed_dex_type = match pool.dex_type.as_str() {
                "uniswap_v3" => DexType::UniswapV3,
                "pancakeswap_v3" => DexType::PancakeSwapV3,
                "sushiswap_v3" => DexType::SushiSwapV3,
                "uniswap_v2" => DexType::UniswapV2,
                "sushiswap_v2" => DexType::SushiSwapV2,
                "pancakeswap_v2" => DexType::PancakeSwapV2,
                _ => continue,
            };

            let pool_state = PoolState {
                address: pool.address.parse().unwrap_or_default(),
                token0: pool.token0.parse().unwrap_or_default(),
                token1: pool.token1.parse().unwrap_or_default(),
                dex_type: parsed_dex_type,
                fee: pool.fee as u32,
                reserve0: ethers::types::U256::zero(),
                reserve1: ethers::types::U256::zero(),
                sqrt_price_x96: None,
                liquidity: None,
                tick: None,
                last_block: 0,
                last_updated: std::time::Instant::now(),
            };
            scanner.add_pool(pool_state).await;
            count += 1;
        }
        count
    }

    /// 从 arbitrage_pools 表加载套利池子配置
    async fn load_arbitrage_pools(
        database: &Database,
        chain_id: u64,
    ) -> Vec<services::ArbitragePoolConfig> {
        let config_db = ArbitrageConfigDb::new(database.pool().clone());
        config_db.get_enabled_pools(chain_id).await.unwrap_or_default()
    }

    /// 将套利池子加载到扫描器
    #[allow(dead_code)]
    async fn load_pools_to_scanner(
        scanner: &EventDrivenScanner<Provider<StatsHttp>>,
        pools: Vec<services::ArbitragePoolConfig>,
    ) -> usize {
        let mut count = 0;
        for pool in pools {
            // 解析 DEX 类型
            let parsed_dex_type = match pool.dex_type.as_str() {
                "uniswap_v3" => DexType::UniswapV3,
                "pancakeswap_v3" => DexType::PancakeSwapV3,
                "sushiswap_v3" => DexType::SushiSwapV3,
                "uniswap_v2" => DexType::UniswapV2,
                "sushiswap_v2" => DexType::SushiSwapV2,
                "pancakeswap_v2" => DexType::PancakeSwapV2,
                _ => continue,
            };

            let pool_state = PoolState {
                address: pool.address.parse().unwrap_or_default(),
                token0: pool.token0.parse().unwrap_or_default(),
                token1: pool.token1.parse().unwrap_or_default(),
                dex_type: parsed_dex_type,
                fee: pool.fee as u32,
                reserve0: ethers::types::U256::zero(),
                reserve1: ethers::types::U256::zero(),
                sqrt_price_x96: None,
                liquidity: None,
                tick: None,
                last_block: 0,
                last_updated: std::time::Instant::now(),
            };
            scanner.add_pool(pool_state).await;
            count += 1;
        }
        count
    }

    fn create_strategy_manager(
        config: &AppConfig,
        database: &Database,
        eth_provider: Arc<Provider<StatsHttp>>,
        wallet: Option<LocalWallet>,
    ) -> Arc<ArbitrageStrategyManager<Provider<StatsHttp>>> {
        let executor_settings = ExecutorSettings {
            arbitrage_contract: config
                .wallet
                .arbitrage_contract_address
                .as_ref()
                .and_then(|s| s.parse().ok()),
            max_gas_price_gwei: config.arbitrage.max_gas_price_gwei.unwrap_or(100.0),
            use_flashbots: config.mev.use_flashbots,
            flashbots_rpc_url: if config.mev.use_flashbots {
                Some(
                    config
                        .mev
                        .flashbots_rpc
                        .clone()
                        .unwrap_or_else(|| "https://relay.flashbots.net".to_string()),
                )
            } else {
                None
            },
            use_public_mempool: config.mev.use_public_mempool,
            dry_run: config.arbitrage.dry_run.unwrap_or(true),
            priority_fee_gwei: config.mev.priority_fee_gwei.unwrap_or(2.0),
        };

        let auto_execute = config.arbitrage.auto_execute.unwrap_or(false);

        let manager = Arc::new(ArbitrageStrategyManager::new(
            database.pool().clone(),
            eth_provider,
            config.ethereum.chain_id,
            executor_settings,
            wallet,
            auto_execute,
        ));

        info!("✅ 策略管理器已创建");
        info!("   - 自动执行: {}", auto_execute);
        info!(
            "   - 干运行模式: {}",
            config.arbitrage.dry_run.unwrap_or(true)
        );

        manager
    }

    fn log_startup_complete(&self) {
        info!("========================================");
        info!("  系统启动完成");
        info!(
            "  API 地址: http://{}:{}",
            self.config.api.host, self.config.api.port
        );
        info!("  策略管理器: 就绪");
        info!("  价格服务: 已启动");
        info!(
            "  区块订阅器: {}",
            if self.block_handle.is_some() {
                "已启动"
            } else {
                "未启动"
            }
        );
        info!(
            "  事件驱动扫描器: {}",
            if self.event_scanner_handle.is_some() {
                "已启动"
            } else {
                "未启动"
            }
        );
        info!(
            "  Solana 扫描器: {}",
            if self.solana_scanner_handle.is_some() {
                "已启动"
            } else {
                "未启动"
            }
        );
        info!("========================================");
    }

    /// 启动 Solana 套利扫描器 (事件驱动模式)
    async fn start_solana_scanner(_config: &AppConfig) -> Option<JoinHandle<()>> {
        // 从环境变量获取 Solana 配置
        let solana_config = SolanaConfig::from_env();

        if !solana_config.enabled {
            info!("[Solana] Solana 扫描器未启用 (SOLANA_ENABLED=false)");
            return None;
        }

        // 获取目标代币
        let target_token = std::env::var("SOLANA_TARGET_TOKEN")
            .unwrap_or_else(|_| "EjamcKN1PixSzm3GiFgUaqCFXBMy3F51JKmbUqNF99S".to_string());

        info!("========================================");
        info!("启动 Solana 链服务 (事件驱动模式)...");
        info!("========================================");
        info!("[Solana] WebSocket: {}", solana_config.ws_url);
        info!("[Solana] 目标代币: {}", target_token);
        info!("[Solana] 最小利润阈值: ${}", solana_config.min_profit_usd);
        info!("[Solana] 最大滑点: {}%", solana_config.max_slippage * 100.0);

        // 创建事件驱动扫描器
        let scanner = EventDrivenSolanaScanner::new(
            &solana_config.ws_url,
            &target_token,
        );

        // 启动扫描器
        let handle = tokio::spawn(async move {
            if let Err(e) = scanner.start().await {
                error!("[Solana] 事件驱动扫描器错误: {}", e);
            }
        });

        info!("[Solana] ✅ Solana 事件驱动扫描器已启动");
        info!("[Solana] 正在监控 Raydium CLMM/AMM V4 的 swap 事件...");

        Some(handle)
    }
}

/// 设置全局 panic hook
pub fn setup_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        error!("========================================");
        error!("!!! 系统发生 PANIC !!!");
        error!("========================================");
        error!("Panic 信息: {:?}", panic_info);
        if let Some(location) = panic_info.location() {
            error!(
                "发生位置: {}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            );
        }
        if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            error!("Panic 消息: {}", s);
        }
        error!("========================================");
    }));
}
