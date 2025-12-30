//! 三角套利历史回测工具
//!
//! 使用方法:
//!   # 下载数据
//!   cargo run -p backtest -- download --days 90
//!
//!   # 分析数据
//!   cargo run -p backtest -- analyze
//!
//!   # 一次性下载并分析
//!   cargo run -p backtest -- all

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use backtest::{
    config::BacktestConfig,
    database::BacktestDatabase,
    downloader::SwapDataDownloader,
    analyzer::ArbitrageAnalyzer,
    report::generate_report,
};

#[derive(Parser)]
#[command(name = "backtest")]
#[command(about = "三角套利历史回测工具")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 下载历史 Swap 数据
    Download {
        /// 下载天数（默认 90 天）
        #[arg(short, long, default_value = "90")]
        days: u64,

        /// 采样间隔（每 N 个区块采样一次，默认 100）
        #[arg(short, long, default_value = "100")]
        interval: u64,
    },

    /// 分析已下载的数据
    Analyze {
        /// 开始区块（默认使用已下载数据的最早区块）
        #[arg(long)]
        start_block: Option<u64>,

        /// 结束区块（默认使用已下载数据的最新区块）
        #[arg(long)]
        end_block: Option<u64>,

        /// 输出目录
        #[arg(short, long, default_value = "backtest_data")]
        output: String,

        /// 使用简化模型（不使用真实价格）
        #[arg(long, default_value = "false")]
        simple: bool,
    },

    /// 下载并分析
    All {
        /// 下载天数（默认 90 天）
        #[arg(short, long, default_value = "90")]
        days: u64,

        /// 采样间隔
        #[arg(short, long, default_value = "100")]
        interval: u64,

        /// 输出目录
        #[arg(short, long, default_value = "backtest_data")]
        output: String,
    },

    /// 显示池子和路径配置
    Show,
}

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    // 解析命令行参数
    let cli = Cli::parse();

    // 加载配置
    let mut config = BacktestConfig::from_env()?;

    // 创建数据库连接
    let db = Arc::new(BacktestDatabase::new(&config.database_url).await?);
    db.initialize_tables().await?;

    // 获取池子和路径配置
    let pools = db.get_enabled_pools(config.chain_id as i64).await?;
    let paths = db.get_enabled_paths(config.chain_id as i64).await?;

    info!("已加载 {} 个池子配置", pools.len());
    info!("已加载 {} 条路径配置", paths.len());

    match cli.command {
        Commands::Download { days, interval } => {
            config.days = days;
            config.sample_interval = interval;

            let downloader = SwapDataDownloader::new(config, db, pools).await?;
            let count = downloader.download().await?;
            info!("下载完成，共 {} 条记录", count);
        }

        Commands::Analyze { start_block, end_block, output, simple } => {
            // 获取区块范围
            let (start, end) = if let (Some(s), Some(e)) = (start_block, end_block) {
                (s, e)
            } else {
                // 从数据库获取范围
                let latest = db.get_latest_downloaded_block(config.chain_id as i64).await?;
                let end = latest.unwrap_or(0);
                let blocks_per_day = 24 * 60 * 60 / 12;
                let start = end.saturating_sub(config.days * blocks_per_day);
                (start, end)
            };

            if start >= end {
                anyhow::bail!("没有可分析的数据，请先运行 download 命令");
            }

            let analyzer = ArbitrageAnalyzer::new(config, db, pools, paths);
            let stats = if simple {
                info!("使用简化模型分析（保守估计）...");
                analyzer.analyze_simple(start, end).await?
            } else {
                info!("使用真实价格数据分析...");
                analyzer.analyze(start, end).await?
            };

            generate_report(&stats, &output)?;
        }

        Commands::All { days, interval, output } => {
            config.days = days;
            config.sample_interval = interval;

            // 下载
            info!("=== 阶段 1: 下载数据 ===");
            let downloader = SwapDataDownloader::new(config.clone(), db.clone(), pools.clone()).await?;
            let count = downloader.download().await?;
            info!("下载完成，共 {} 条记录", count);

            // 分析
            info!("\n=== 阶段 2: 分析数据 ===");
            let latest = db.get_latest_downloaded_block(config.chain_id as i64).await?;
            let end = latest.unwrap_or(0);
            let blocks_per_day = 24 * 60 * 60 / 12;
            let start = end.saturating_sub(config.days * blocks_per_day);

            if start >= end {
                anyhow::bail!("没有可分析的数据");
            }

            let analyzer = ArbitrageAnalyzer::new(config, db, pools, paths);
            let stats = analyzer.analyze(start, end).await?;

            generate_report(&stats, &output)?;
        }

        Commands::Show => {
            println!("\n=== 池子配置 ({} 个) ===", pools.len());
            println!("{:-<100}", "");
            println!("{:<45} {:<20} {:<10} {:<10}", "地址", "交易对", "DEX", "费率");
            println!("{:-<100}", "");
            for pool in &pools {
                println!(
                    "{:<45} {:<20} {:<10} {:.2}%",
                    pool.address,
                    format!("{}/{}", pool.token0_symbol, pool.token1_symbol),
                    pool.dex_type,
                    pool.fee_percent()
                );
            }

            println!("\n=== 套利路径配置 ({} 条) ===", paths.len());
            println!("{:-<120}", "");
            println!("{:<60} {:<30} {:<10}", "路径名称", "三角组合", "优先级");
            println!("{:-<120}", "");
            for path in &paths {
                println!(
                    "{:<60} {:<30} {:<10}",
                    path.path_name,
                    path.triangle_name,
                    path.priority
                );
            }
        }
    }

    Ok(())
}
