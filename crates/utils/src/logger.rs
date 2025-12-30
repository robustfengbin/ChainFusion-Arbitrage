use std::fs;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, fmt, Layer};
use tracing_subscriber::filter::{LevelFilter, FilterFn};
use tracing_subscriber::fmt::time::OffsetTime;
use tracing_appender::{non_blocking, rolling};
use time::macros::offset;

/// 日志管理器 - 基于target分类的日志系统
pub struct LoggerManager {
    _guards: Vec<non_blocking::WorkerGuard>,
}

impl LoggerManager {
    /// 初始化日志系统
    ///
    /// 日志分类：
    /// - app.log: 通用应用日志
    /// - rpc_stats.log: RPC 请求统计专用日志
    /// - strategy.log: 套利策略执行日志
    /// - trade.log: 交易执行日志
    /// - api.log: API请求日志
    /// - opportunity.log: 套利机会发现日志 (盈利机会专用)
    pub fn init() -> Self {
        let mut guards = Vec::new();

        // 创建日志目录
        fs::create_dir_all("logs").ok();

        // 配置时区为东八区 (UTC+8 上海时间)
        let timer = OffsetTime::new(
            offset!(+8),
            time::format_description::well_known::Rfc3339,
        );

        // 1. 控制台输出 - 显示INFO级别，排除rpc_stats的详细日志
        let console_layer = fmt::layer()
            .compact()
            .with_target(true)
            .with_timer(timer.clone())
            .with_filter(LevelFilter::INFO)
            .with_filter(FilterFn::new(|metadata| {
                // rpc_stats 只输出到文件，不输出到控制台（除非是 INFO 级别以上的汇总）
                metadata.target() != "rpc_stats" || metadata.level() <= &tracing::Level::INFO
            }));

        // 2. 通用应用日志 (app.log)
        let (app_writer, app_guard) = {
            let appender = rolling::daily("logs", "app.log");
            non_blocking(appender)
        };
        guards.push(app_guard);

        let app_layer = fmt::layer()
            .compact()
            .with_writer(app_writer)
            .with_ansi(false)
            .with_target(true)
            .with_timer(timer.clone())
            .with_filter(LevelFilter::INFO)
            .with_filter(FilterFn::new(|metadata| {
                // 排除特定 target 的日志
                !matches!(metadata.target(),
                    "rpc_stats" | "strategy" | "trade_executor" | "api::handlers" | "arbitrage_opportunity" | "arbitrage_execution"
                )
            }));

        // 3. RPC 统计日志 (rpc_stats.log)
        let (rpc_stats_writer, rpc_stats_guard) = {
            let appender = rolling::daily("logs", "rpc_stats.log");
            non_blocking(appender)
        };
        guards.push(rpc_stats_guard);

        let rpc_stats_layer = fmt::layer()
            .compact()
            .with_writer(rpc_stats_writer)
            .with_ansi(false)
            .with_target(true)
            .with_timer(timer.clone())
            .with_filter(FilterFn::new(|metadata| {
                metadata.target() == "rpc_stats"
            }));

        // 4. 策略执行日志 (strategy.log)
        let (strategy_writer, strategy_guard) = {
            let appender = rolling::daily("logs", "strategy.log");
            non_blocking(appender)
        };
        guards.push(strategy_guard);

        let strategy_layer = fmt::layer()
            .compact()
            .with_writer(strategy_writer)
            .with_ansi(false)
            .with_target(true)
            .with_timer(timer.clone())
            .with_filter(FilterFn::new(|metadata| {
                metadata.target() == "strategy"
            }));

        // 5. 交易执行日志 (trade.log)
        let (trade_writer, trade_guard) = {
            let appender = rolling::daily("logs", "trade.log");
            non_blocking(appender)
        };
        guards.push(trade_guard);

        let trade_layer = fmt::layer()
            .compact()
            .with_writer(trade_writer)
            .with_ansi(false)
            .with_target(true)
            .with_timer(timer.clone())
            .with_filter(FilterFn::new(|metadata| {
                metadata.target() == "trade_executor"
            }));

        // 6. API请求日志 (api.log)
        let (api_writer, api_guard) = {
            let appender = rolling::daily("logs", "api.log");
            non_blocking(appender)
        };
        guards.push(api_guard);

        let api_layer = fmt::layer()
            .compact()
            .with_writer(api_writer)
            .with_ansi(false)
            .with_target(true)
            .with_timer(timer.clone())
            .with_filter(FilterFn::new(|metadata| {
                metadata.target() == "api::handlers"
            }));

        // 7. 套利机会日志 (opportunity.log) - 发现盈利机会及执行日志
        let (opportunity_writer, opportunity_guard) = {
            let appender = rolling::daily("logs", "opportunity.log");
            non_blocking(appender)
        };
        guards.push(opportunity_guard);

        let opportunity_layer = fmt::layer()
            .compact()
            .with_writer(opportunity_writer)
            .with_ansi(false)
            .with_target(true)
            .with_timer(timer.clone())
            .with_filter(FilterFn::new(|metadata| {
                // 同时捕获发现机会和执行套利的日志
                matches!(metadata.target(), "arbitrage_opportunity" | "arbitrage_execution")
            }));

        // 初始化tracing订阅器
        tracing_subscriber::registry()
            .with(console_layer)
            .with(app_layer)
            .with(rpc_stats_layer)
            .with(strategy_layer)
            .with(trade_layer)
            .with(api_layer)
            .with(opportunity_layer)
            .init();

        Self { _guards: guards }
    }
}
