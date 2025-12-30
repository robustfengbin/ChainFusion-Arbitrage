//! Raydium DEX 交互模块
//!
//! 支持:
//! - Raydium CLMM (集中流动性)
//! - Raydium AMM V4

use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

use crate::types::*;

/// Raydium CLMM Pool 结构 (简化版)
#[derive(Debug, Clone)]
pub struct RaydiumClmmPool {
    /// 池子地址
    pub address: Pubkey,
    /// Token 0 Mint
    pub token_0_mint: Pubkey,
    /// Token 1 Mint
    pub token_1_mint: Pubkey,
    /// Token 0 Vault
    pub token_0_vault: Pubkey,
    /// Token 1 Vault
    pub token_1_vault: Pubkey,
    /// 手续费率 (万分之几)
    pub fee_rate: u32,
    /// 当前 tick
    pub tick_current: i32,
    /// 当前价格 (sqrt_price_x64)
    pub sqrt_price_x64: u128,
    /// 流动性
    pub liquidity: u128,
}

impl RaydiumClmmPool {
    /// 从账户数据解析 CLMM Pool
    pub fn from_account_data(address: Pubkey, data: &[u8]) -> Option<Self> {
        // CLMM Pool 账户布局 (简化)
        // offset 8: discriminator
        // offset 8+32: token_0_mint
        // offset 8+64: token_1_mint
        // ... 更多字段

        if data.len() < 200 {
            return None;
        }

        // 跳过 8 字节 discriminator
        let offset = 8;

        // 解析 token mints (需要根据实际账户布局调整)
        let token_0_mint = Pubkey::try_from(&data[offset..offset + 32]).ok()?;
        let token_1_mint = Pubkey::try_from(&data[offset + 32..offset + 64]).ok()?;
        let token_0_vault = Pubkey::try_from(&data[offset + 64..offset + 96]).ok()?;
        let token_1_vault = Pubkey::try_from(&data[offset + 96..offset + 128]).ok()?;

        Some(Self {
            address,
            token_0_mint,
            token_1_mint,
            token_0_vault,
            token_1_vault,
            fee_rate: 2500, // 默认 0.25%
            tick_current: 0,
            sqrt_price_x64: 0,
            liquidity: 0,
        })
    }

    /// 转换为通用 SolanaPool 格式
    pub fn to_solana_pool(&self) -> SolanaPool {
        SolanaPool {
            address: self.address,
            dex_type: SolanaDexType::RaydiumClmm,
            token_a: SplTokenInfo {
                mint: self.token_0_mint,
                symbol: format!("T0_{}", &self.token_0_mint.to_string()[0..6]),
                decimals: 9,
                name: None,
                is_stable: false,
            },
            token_b: SplTokenInfo {
                mint: self.token_1_mint,
                symbol: format!("T1_{}", &self.token_1_mint.to_string()[0..6]),
                decimals: 9,
                name: None,
                is_stable: false,
            },
            fee_bps: (self.fee_rate / 100) as u16,
            liquidity_usd: None,
            enabled: true,
        }
    }
}

/// Raydium AMM V4 Pool 结构
#[derive(Debug, Clone)]
pub struct RaydiumAmmPool {
    /// 池子地址
    pub address: Pubkey,
    /// Token A Mint
    pub token_a_mint: Pubkey,
    /// Token B Mint
    pub token_b_mint: Pubkey,
    /// LP Mint
    pub lp_mint: Pubkey,
    /// Token A 储备
    pub token_a_reserve: u64,
    /// Token B 储备
    pub token_b_reserve: u64,
}

impl RaydiumAmmPool {
    /// 从账户数据解析 AMM Pool
    pub fn from_account_data(address: Pubkey, data: &[u8]) -> Option<Self> {
        if data.len() < 200 {
            return None;
        }

        // AMM V4 账户布局 (简化)
        let offset = 8;

        let token_a_mint = Pubkey::try_from(&data[offset + 72..offset + 104]).ok()?;
        let token_b_mint = Pubkey::try_from(&data[offset + 104..offset + 136]).ok()?;
        let lp_mint = Pubkey::try_from(&data[offset + 136..offset + 168]).ok()?;

        Some(Self {
            address,
            token_a_mint,
            token_b_mint,
            lp_mint,
            token_a_reserve: 0,
            token_b_reserve: 0,
        })
    }

    /// 转换为通用 SolanaPool 格式
    pub fn to_solana_pool(&self) -> SolanaPool {
        SolanaPool {
            address: self.address,
            dex_type: SolanaDexType::RaydiumAmmV4,
            token_a: SplTokenInfo {
                mint: self.token_a_mint,
                symbol: format!("TA_{}", &self.token_a_mint.to_string()[0..6]),
                decimals: 9,
                name: None,
                is_stable: false,
            },
            token_b: SplTokenInfo {
                mint: self.token_b_mint,
                symbol: format!("TB_{}", &self.token_b_mint.to_string()[0..6]),
                decimals: 9,
                name: None,
                is_stable: false,
            },
            fee_bps: 25, // 0.25% 默认
            liquidity_usd: None,
            enabled: true,
        }
    }

    /// 计算 swap 输出金额 (恒定乘积)
    pub fn calculate_swap_output(&self, amount_in: u64, is_a_to_b: bool) -> u64 {
        let (reserve_in, reserve_out) = if is_a_to_b {
            (self.token_a_reserve, self.token_b_reserve)
        } else {
            (self.token_b_reserve, self.token_a_reserve)
        };

        if reserve_in == 0 || reserve_out == 0 {
            return 0;
        }

        // 扣除 0.25% 手续费
        let amount_in_with_fee = (amount_in as u128) * 9975;
        let numerator = amount_in_with_fee * (reserve_out as u128);
        let denominator = (reserve_in as u128) * 10000 + amount_in_with_fee;

        (numerator / denominator) as u64
    }
}

/// Raydium 常用池子地址
pub mod known_pools {
    use super::*;

    /// SOL-USDC CLMM Pool
    pub const SOL_USDC_CLMM: &str = "2QdhepnKRTLjjSqPL1PtKNwqrUkoLee5Gqs8bvZhRdMv";

    /// SOL-USDT CLMM Pool
    pub const SOL_USDT_CLMM: &str = "CRGfGWvhWZTj8LbqNXZBCZhqQM6L3dVc8LdWJKjQqmm3";

    pub fn sol_usdc_clmm() -> Pubkey {
        Pubkey::from_str(SOL_USDC_CLMM).unwrap()
    }

    pub fn sol_usdt_clmm() -> Pubkey {
        Pubkey::from_str(SOL_USDT_CLMM).unwrap()
    }
}
