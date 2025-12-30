//! 三角套利历史回测模块
//!
//! 功能：
//! 1. 下载最近 3 个月的区块 Swap 事件数据
//! 2. 从数据库读取池子配置
//! 3. 分析 24 条三角套利路径
//! 4. 生成分析报告

pub mod config;
pub mod database;
pub mod downloader;
pub mod analyzer;
pub mod models;
pub mod price;
pub mod report;

pub use config::BacktestConfig;
pub use database::BacktestDatabase;
pub use downloader::SwapDataDownloader;
pub use analyzer::ArbitrageAnalyzer;
