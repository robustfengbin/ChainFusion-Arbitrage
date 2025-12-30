//! Flashbots 模块
//!
//! 提供私密交易发送功能，防止 MEV 攻击（三明治攻击、抢跑等）
//!
//! ## 工作原理
//!
//! ```text
//! 普通交易:  策略 → 公开 Mempool → 所有人可见 → 可能被攻击
//! Flashbots: 策略 → Flashbots 中继 → 私密发给验证者 → 直接打包
//! ```
//!
//! ## 特点
//! - 交易不会出现在公开 mempool，其他人看不到
//! - 失败的交易不会上链，不浪费 gas
//! - 覆盖约 90% 的以太坊验证者

mod client;
mod bundle;
mod types;

pub use client::FlashbotsClient;
pub use bundle::BundleBuilder;
pub use types::*;
