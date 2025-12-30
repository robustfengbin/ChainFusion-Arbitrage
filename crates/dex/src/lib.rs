pub mod uniswap;
pub mod curve;
pub mod pancakeswap;
pub mod common;
pub mod flashloan;

// Re-export main types to avoid ambiguous glob re-exports
pub use uniswap::{
    UniswapV2Protocol, UniswapV3Protocol, UniswapV4Protocol,
    PoolKey, SwapParams, FlashAccounting, V4ArbitragePathBuilder, HooksConfig,
    v2_addresses, v3_addresses, v4_addresses,
};
pub use curve::CurveProtocol;
pub use pancakeswap::{PancakeSwapV2Protocol, PancakeSwapV3Protocol, pancake_v2_addresses, pancake_v3_addresses};
pub use common::DexProtocol;
pub use flashloan::{
    FlashLoanExecutor, FlashLoanProvider, ArbitrageRequest, ArbitrageResult,
    SwapStep, SimulationResult, ArbitrageTransaction,
    TriangularArbitrageBuilder, CrossDexArbitrageBuilder, DexInfo,
};
