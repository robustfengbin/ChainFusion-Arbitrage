//! 套利执行器模块
//!
//! 负责调用链上合约执行套利交易
//!
//! ## 模块结构
//!
//! - `executor`: 核心执行器，支持普通交易和 Flashbots 私密交易
//! - `flashbots`: Flashbots 集成，防止 MEV 攻击
//! - `flash_arbitrage`: 套利合约 ABI 绑定
//! - `types`: 类型定义
//! - `converter`: 套利机会转换器，自动选择闪电贷池

mod flash_arbitrage;
mod executor;
mod types;
pub mod flashbots;
pub mod converter;
pub mod revert_decoder;
pub mod debug_info;

pub use flash_arbitrage::{FlashArbitrageContract, ArbitrageContractParams};
pub use executor::{ArbitrageExecutor, ExecutorConfig, SendMode};
pub use types::{ArbitrageParams, ExecutionResult, ExecutionError, GasStrategy};
pub use flashbots::{FlashbotsClient, FlashbotsConfig, FlashbotsSendResult, BundleBuilder};
pub use converter::{
    ArbitrageParamsBuilder, FlashPoolSelector, FlashPoolSelectorConfig,
    FlashPoolSelection, is_v3_only_path, extract_tokens,
    calculate_flash_fee, is_still_profitable,
};
pub use revert_decoder::{RevertDecoder, DecodedRevertError, RevertErrorType, ErrorAnalysis};
pub use debug_info::{
    ExecutionDebugger, ExecutionSnapshot, ErrorSnapshot, log_execution_start,
    TokenInfoSnapshot, TokenDetail, PoolStateSnapshot, PoolRole, SwapPoolInfo,
};
