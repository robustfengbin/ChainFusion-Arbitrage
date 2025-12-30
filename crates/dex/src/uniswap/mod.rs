pub mod v2;
pub mod v3;
pub mod v4;
pub mod contracts;

// Re-export main protocol types
pub use v2::UniswapV2Protocol;
pub use v3::UniswapV3Protocol;
pub use v4::{UniswapV4Protocol, PoolKey, SwapParams, FlashAccounting, V4ArbitragePathBuilder, HooksConfig};
pub use contracts::{v2_addresses, v3_addresses, v4_addresses};
