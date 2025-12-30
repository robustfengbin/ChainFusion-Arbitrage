//! Solana 套利模块
//!
//! 支持 Solana 链上的 DEX 套利，包括：
//! - Raydium (CLMM 集中流动性)
//! - Orca (Whirlpools)
//! - Jupiter (聚合器)
//!
//! 使用 WebSocket 事件驱动监控 swap 事件

pub mod client;
pub mod dex;
pub mod scanner;
pub mod types;
pub mod config;
pub mod ws_subscriber;

pub use client::SolanaClient;
pub use scanner::SolanaArbitrageScanner;
pub use config::SolanaConfig;
pub use ws_subscriber::{SolanaWsSubscriber, EventDrivenSolanaScanner, SwapEvent};
pub use types::*;
