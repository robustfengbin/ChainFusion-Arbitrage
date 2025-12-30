use ethers::types::Address;
use serde::{Deserialize, Serialize};

/// Token 信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub address: Address,
    pub symbol: String,
    pub name: String,
    pub decimals: u8,
    pub chain_id: u64,
}

impl Token {
    pub fn new(address: Address, symbol: String, name: String, decimals: u8, chain_id: u64) -> Self {
        Self {
            address,
            symbol,
            name,
            decimals,
            chain_id,
        }
    }
}

/// Token 对
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPair {
    pub token0: Token,
    pub token1: Token,
}

impl TokenPair {
    pub fn new(token0: Token, token1: Token) -> Self {
        Self { token0, token1 }
    }

    /// 获取排序后的 token 对 (按地址排序)
    pub fn sorted(&self) -> (&Token, &Token) {
        if self.token0.address < self.token1.address {
            (&self.token0, &self.token1)
        } else {
            (&self.token1, &self.token0)
        }
    }
}

/// 常用 Token 地址 (Ethereum Mainnet)
pub mod eth_tokens {
    use ethers::types::Address;
    use std::str::FromStr;

    lazy_static::lazy_static! {
        pub static ref WETH: Address = Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap();
        pub static ref USDT: Address = Address::from_str("0xdAC17F958D2ee523a2206206994597C13D831ec7").unwrap();
        pub static ref USDC: Address = Address::from_str("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48").unwrap();
        pub static ref DAI: Address = Address::from_str("0x6B175474E89094C44Da98b954EedeAC495271d0F").unwrap();
        pub static ref WBTC: Address = Address::from_str("0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599").unwrap();
    }
}

/// 常用 Token 地址 (BSC Mainnet)
pub mod bsc_tokens {
    use ethers::types::Address;
    use std::str::FromStr;

    lazy_static::lazy_static! {
        pub static ref WBNB: Address = Address::from_str("0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c").unwrap();
        pub static ref USDT: Address = Address::from_str("0x55d398326f99059fF775485246999027B3197955").unwrap();
        pub static ref USDC: Address = Address::from_str("0x8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d").unwrap();
        pub static ref BUSD: Address = Address::from_str("0xe9e7CEA3DedcA5984780Bafc599bD69ADd087D56").unwrap();
        pub static ref CAKE: Address = Address::from_str("0x0E09FaBB73Bd3Ade0a17ECC321fD13a19e81cE82").unwrap();
    }
}
