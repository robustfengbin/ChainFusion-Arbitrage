//! Solana 类型定义

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use rust_decimal::Decimal;
use std::str::FromStr;

/// Solana 链 ID (非标准，用于内部标识)
pub const SOLANA_CHAIN_ID: u64 = 900;

/// Solana DEX 类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SolanaDexType {
    /// Raydium CLMM (集中流动性)
    RaydiumClmm,
    /// Raydium AMM V4
    RaydiumAmmV4,
    /// Orca Whirlpools
    OrcaWhirlpool,
    /// Jupiter 聚合器
    Jupiter,
}

impl std::fmt::Display for SolanaDexType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolanaDexType::RaydiumClmm => write!(f, "raydium_clmm"),
            SolanaDexType::RaydiumAmmV4 => write!(f, "raydium_amm_v4"),
            SolanaDexType::OrcaWhirlpool => write!(f, "orca_whirlpool"),
            SolanaDexType::Jupiter => write!(f, "jupiter"),
        }
    }
}

/// SPL Token 信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplTokenInfo {
    /// Token Mint 地址
    pub mint: Pubkey,
    /// Token 符号
    pub symbol: String,
    /// 小数位数
    pub decimals: u8,
    /// Token 名称
    pub name: Option<String>,
    /// 是否是稳定币
    pub is_stable: bool,
}

impl SplTokenInfo {
    pub fn new(mint: &str, symbol: &str, decimals: u8, is_stable: bool) -> Option<Self> {
        Some(Self {
            mint: Pubkey::from_str(mint).ok()?,
            symbol: symbol.to_string(),
            decimals,
            name: None,
            is_stable,
        })
    }
}

/// Solana 流动性池信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaPool {
    /// 池子地址
    pub address: Pubkey,
    /// DEX 类型
    pub dex_type: SolanaDexType,
    /// Token A
    pub token_a: SplTokenInfo,
    /// Token B
    pub token_b: SplTokenInfo,
    /// 手续费 (基点，如 25 = 0.25%)
    pub fee_bps: u16,
    /// 池子流动性 (TVL)
    pub liquidity_usd: Option<Decimal>,
    /// 是否启用
    pub enabled: bool,
}

/// Solana 套利路径
#[derive(Debug, Clone)]
pub struct SolanaArbitragePath {
    /// 路径名称
    pub name: String,
    /// 涉及的池子
    pub pools: Vec<SolanaPool>,
    /// 代币路径 (mint 地址)
    pub token_path: Vec<Pubkey>,
    /// 预估利润 (USD)
    pub estimated_profit_usd: Decimal,
    /// 预估 gas 费用 (SOL)
    pub estimated_fee_sol: Decimal,
}

/// Solana 套利机会
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaArbitrageOpportunity {
    /// 机会 ID
    pub id: String,
    /// 路径名称
    pub path_name: String,
    /// 输入代币
    pub input_token: Pubkey,
    /// 输入金额
    pub input_amount: u64,
    /// 输出金额
    pub output_amount: u64,
    /// 净利润 (USD)
    pub net_profit_usd: Decimal,
    /// DEX 路径
    pub dex_path: Vec<SolanaDexType>,
    /// 发现时间
    pub discovered_at: u64,
    /// 区块 slot
    pub slot: u64,
}

/// Swap 事件 (从链上解析)
#[derive(Debug, Clone)]
pub struct SolanaSwapEvent {
    /// 交易签名
    pub signature: String,
    /// Slot
    pub slot: u64,
    /// 池子地址
    pub pool: Pubkey,
    /// DEX 类型
    pub dex_type: SolanaDexType,
    /// 输入代币
    pub token_in: Pubkey,
    /// 输出代币
    pub token_out: Pubkey,
    /// 输入金额
    pub amount_in: u64,
    /// 输出金额
    pub amount_out: u64,
    /// 交易者
    pub user: Pubkey,
}

/// 常用 Solana 代币地址
pub mod known_tokens {
    use super::*;

    /// SOL (Wrapped SOL)
    pub const WSOL: &str = "So11111111111111111111111111111111111111112";
    /// USDC
    pub const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    /// USDT
    pub const USDT: &str = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";
    /// RAY (Raydium)
    pub const RAY: &str = "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R";
    /// BONK
    pub const BONK: &str = "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263";
    /// JUP (Jupiter)
    pub const JUP: &str = "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN";

    pub fn wsol() -> Pubkey {
        Pubkey::from_str(WSOL).unwrap()
    }

    pub fn usdc() -> Pubkey {
        Pubkey::from_str(USDC).unwrap()
    }

    pub fn usdt() -> Pubkey {
        Pubkey::from_str(USDT).unwrap()
    }
}

/// Raydium 程序地址
pub mod raydium {
    use super::*;

    /// Raydium CLMM Program
    pub const CLMM_PROGRAM: &str = "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK";
    /// Raydium AMM V4 Program
    pub const AMM_V4_PROGRAM: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";

    pub fn clmm_program() -> Pubkey {
        Pubkey::from_str(CLMM_PROGRAM).unwrap()
    }

    pub fn amm_v4_program() -> Pubkey {
        Pubkey::from_str(AMM_V4_PROGRAM).unwrap()
    }
}

/// Orca 程序地址
pub mod orca {
    use super::*;

    /// Orca Whirlpool Program
    pub const WHIRLPOOL_PROGRAM: &str = "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc";

    pub fn whirlpool_program() -> Pubkey {
        Pubkey::from_str(WHIRLPOOL_PROGRAM).unwrap()
    }
}
