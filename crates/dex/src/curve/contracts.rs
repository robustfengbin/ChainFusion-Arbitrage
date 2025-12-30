use ethers::types::Address;
use std::str::FromStr;

/// Curve 合约地址 (Ethereum Mainnet)
pub mod curve_addresses {
    use super::*;

    lazy_static::lazy_static! {
        /// Curve 3Pool (USDT/USDC/DAI)
        pub static ref THREE_POOL: Address = Address::from_str("0xbEbc44782C7dB0a1A60Cb6fe97d0b483032FF1C7").unwrap();
        /// Curve stETH/ETH Pool
        pub static ref STETH_POOL: Address = Address::from_str("0xDC24316b9AE028F1497c275EB9192a3Ea0f67022").unwrap();
        /// Curve Registry
        pub static ref REGISTRY: Address = Address::from_str("0x90E00ACe148ca3b23Ac1bC8C240C2a7Dd9c2d7f5").unwrap();
        /// Curve Address Provider
        pub static ref ADDRESS_PROVIDER: Address = Address::from_str("0x0000000022D53366457F9d5E68Ec105046FC4383").unwrap();
    }
}

// 引入 lazy_static
use lazy_static;
