use ethers::types::Address;
use std::str::FromStr;

/// PancakeSwap 合约地址 (BSC Mainnet)
pub mod pancake_v2_addresses {
    use super::*;

    lazy_static::lazy_static! {
        /// PancakeSwap V2 Factory
        pub static ref FACTORY: Address = Address::from_str("0xcA143Ce32Fe78f1f7019d7d551a6402fC5350c73").unwrap();
        /// PancakeSwap V2 Router
        pub static ref ROUTER: Address = Address::from_str("0x10ED43C718714eb63d5aA57B78B54704E256024E").unwrap();
    }

    /// 计算 PancakeSwap V2 池子地址
    pub fn compute_pair_address(factory: Address, token0: Address, token1: Address) -> Address {
        use ethers::utils::keccak256;

        let (t0, t1) = if token0 < token1 {
            (token0, token1)
        } else {
            (token1, token0)
        };

        // PancakeSwap V2 init code hash
        let init_code_hash: [u8; 32] = hex::decode(
            "00fb7f630766e6a796048ea87d01acd3068e8ff67d078148a3fa3f4a84f69bd5"
        ).unwrap().try_into().unwrap();

        let mut data = Vec::new();
        data.push(0xff);
        data.extend_from_slice(factory.as_bytes());

        let mut salt_input = Vec::new();
        salt_input.extend_from_slice(t0.as_bytes());
        salt_input.extend_from_slice(t1.as_bytes());
        let salt = keccak256(&salt_input);
        data.extend_from_slice(&salt);
        data.extend_from_slice(&init_code_hash);

        let hash = keccak256(&data);
        Address::from_slice(&hash[12..])
    }
}

/// PancakeSwap V3 合约地址 (BSC Mainnet)
pub mod pancake_v3_addresses {
    use super::*;

    lazy_static::lazy_static! {
        /// PancakeSwap V3 Factory
        pub static ref FACTORY: Address = Address::from_str("0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865").unwrap();
        /// PancakeSwap V3 SwapRouter
        pub static ref SWAP_ROUTER: Address = Address::from_str("0x1b81D678ffb9C0263b24A97847620C99d213eB14").unwrap();
        /// PancakeSwap V3 Quoter
        pub static ref QUOTER: Address = Address::from_str("0xB048Bbc1Ee6b733FFfCFb9e9CeF7375518e25997").unwrap();
    }

    /// 常用费率
    pub const FEE_LOWEST: u32 = 100;    // 0.01%
    pub const FEE_LOW: u32 = 500;       // 0.05%
    pub const FEE_MEDIUM: u32 = 2500;   // 0.25%
    pub const FEE_HIGH: u32 = 10000;    // 1%
}

// 引入 lazy_static
use lazy_static;
