use ethers::types::Address;
use std::str::FromStr;

/// Uniswap V2 合约地址 (Ethereum Mainnet)
pub mod v2_addresses {
    use super::*;

    lazy_static::lazy_static! {
        /// Uniswap V2 Factory
        pub static ref FACTORY: Address = Address::from_str("0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f").unwrap();
        /// Uniswap V2 Router02
        pub static ref ROUTER: Address = Address::from_str("0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D").unwrap();
    }

    /// 计算 Uniswap V2 池子地址
    pub fn compute_pair_address(factory: Address, token0: Address, token1: Address) -> Address {
        use ethers::utils::keccak256;

        let (t0, t1) = if token0 < token1 {
            (token0, token1)
        } else {
            (token1, token0)
        };

        // Uniswap V2 init code hash
        let init_code_hash: [u8; 32] = hex::decode(
            "96e8ac4277198ff8b6f785478aa9a39f403cb768dd02cbee326c3e7da348845f"
        ).unwrap().try_into().unwrap();

        let mut data = Vec::new();
        data.push(0xff);
        data.extend_from_slice(factory.as_bytes());

        // salt = keccak256(token0, token1)
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

/// Uniswap V3 合约地址 (Ethereum Mainnet)
pub mod v3_addresses {
    use super::*;

    lazy_static::lazy_static! {
        /// Uniswap V3 Factory
        pub static ref FACTORY: Address = Address::from_str("0x1F98431c8aD98523631AE4a59f267346ea31F984").unwrap();
        /// Uniswap V3 SwapRouter
        pub static ref SWAP_ROUTER: Address = Address::from_str("0xE592427A0AEce92De3Edee1F18E0157C05861564").unwrap();
        /// Uniswap V3 SwapRouter02
        pub static ref SWAP_ROUTER_02: Address = Address::from_str("0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45").unwrap();
        /// Uniswap V3 Quoter
        pub static ref QUOTER: Address = Address::from_str("0xb27308f9F90D607463bb33eA1BeBb41C27CE5AB6").unwrap();
        /// Uniswap V3 QuoterV2
        pub static ref QUOTER_V2: Address = Address::from_str("0x61fFE014bA17989E743c5F6cB21bF9697530B21e").unwrap();
    }

    /// 常用费率
    pub const FEE_LOWEST: u32 = 100;    // 0.01%
    pub const FEE_LOW: u32 = 500;       // 0.05%
    pub const FEE_MEDIUM: u32 = 3000;   // 0.3%
    pub const FEE_HIGH: u32 = 10000;    // 1%

    /// 计算 Uniswap V3 池子地址
    pub fn compute_pool_address(factory: Address, token0: Address, token1: Address, fee: u32) -> Address {
        use ethers::utils::keccak256;
        use ethers::abi::encode;
        use ethers::abi::Token;

        let (t0, t1) = if token0 < token1 {
            (token0, token1)
        } else {
            (token1, token0)
        };

        // V3 pool init code hash
        let init_code_hash: [u8; 32] = hex::decode(
            "e34f199b19b2b4f47f68442619d555527d244f78a3297ea89325f843f87b8b54"
        ).unwrap().try_into().unwrap();

        // salt = keccak256(abi.encode(token0, token1, fee))
        let salt = keccak256(&encode(&[
            Token::Address(t0),
            Token::Address(t1),
            Token::Uint(fee.into()),
        ]));

        let mut data = Vec::new();
        data.push(0xff);
        data.extend_from_slice(factory.as_bytes());
        data.extend_from_slice(&salt);
        data.extend_from_slice(&init_code_hash);

        let hash = keccak256(&data);
        Address::from_slice(&hash[12..])
    }
}

/// Uniswap V4 合约地址 (Ethereum Mainnet)
/// V4 采用单例模式，所有池子都在 PoolManager 合约中
pub mod v4_addresses {
    use super::*;

    lazy_static::lazy_static! {
        /// Uniswap V4 PoolManager - 核心单例合约，管理所有池子
        pub static ref POOL_MANAGER: Address = Address::from_str("0x000000000004444c5dc75cB358380D2e3dE08A90").unwrap();
        /// Uniswap V4 PositionManager - 流动性管理
        pub static ref POSITION_MANAGER: Address = Address::from_str("0xbD216513d74C8cf14cf4747E6AaA6420FF64ee9e").unwrap();
        /// Uniswap V4 QuoterV2
        pub static ref QUOTER: Address = Address::from_str("0x52f0E24D1c21C8a0cB1e5a5dD6198556BD86D8E9").unwrap();
        /// Uniswap V4 StateView - 读取池子状态
        pub static ref STATE_VIEW: Address = Address::from_str("0x7fFE42C4a5DEeA5b0feC41C94C136Cf115597227").unwrap();
        /// Universal Router V2 (支持V4)
        pub static ref UNIVERSAL_ROUTER: Address = Address::from_str("0x66a9893cC07D91D95644AEDD05D03f95e1dBA8Af").unwrap();
    }

    /// V4 支持的 tick spacing
    pub const TICK_SPACING_1: i32 = 1;      // 最小 tick spacing
    pub const TICK_SPACING_10: i32 = 10;    // 用于 0.05% fee
    pub const TICK_SPACING_60: i32 = 60;    // 用于 0.3% fee
    pub const TICK_SPACING_200: i32 = 200;  // 用于 1% fee

    /// V4 动态费用标志
    pub const DYNAMIC_FEE_FLAG: u32 = 0x800000;

    /// 计算 Pool ID (PoolKey 的 keccak256 哈希)
    pub fn compute_pool_id(
        currency0: Address,
        currency1: Address,
        fee: u32,
        tick_spacing: i32,
        hooks: Address,
    ) -> [u8; 32] {
        use ethers::utils::keccak256;
        use ethers::abi::{encode, Token};

        // 确保 currency0 < currency1
        let (c0, c1) = if currency0 < currency1 {
            (currency0, currency1)
        } else {
            (currency1, currency0)
        };

        let encoded = encode(&[
            Token::Address(c0),
            Token::Address(c1),
            Token::Uint(fee.into()),
            Token::Int(tick_spacing.into()),
            Token::Address(hooks),
        ]);

        keccak256(&encoded)
    }

    /// 判断是否使用动态费用
    pub fn is_dynamic_fee(fee: u32) -> bool {
        (fee & DYNAMIC_FEE_FLAG) != 0
    }

    /// V4 中的原生 ETH 表示地址
    pub fn native_currency() -> Address {
        Address::zero()
    }
}

/// 常用代币地址 (Ethereum Mainnet)
pub mod token_addresses {
    use super::*;

    lazy_static::lazy_static! {
        /// Wrapped ETH
        pub static ref WETH: Address = Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap();
        /// USDC
        pub static ref USDC: Address = Address::from_str("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48").unwrap();
        /// USDT
        pub static ref USDT: Address = Address::from_str("0xdAC17F958D2ee523a2206206994597C13D831ec7").unwrap();
        /// DAI
        pub static ref DAI: Address = Address::from_str("0x6B175474E89094C44Da98b954EedeAC495271d0F").unwrap();
        /// WBTC
        pub static ref WBTC: Address = Address::from_str("0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599").unwrap();
    }
}

// 需要引入 lazy_static
use lazy_static;
