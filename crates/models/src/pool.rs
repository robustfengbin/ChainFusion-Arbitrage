use ethers::types::{Address, U256};
use rust_decimal::Decimal;
use rust_decimal::MathematicalOps;
use serde::{Deserialize, Serialize};

/// DEX 类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DexType {
    UniswapV2,
    UniswapV3,
    UniswapV4,
    Curve,
    PancakeSwapV2,
    PancakeSwapV3,
    SushiSwap,
    SushiSwapV2,
    SushiSwapV3,
}

impl DexType {
    pub fn name(&self) -> &'static str {
        match self {
            DexType::UniswapV2 => "Uniswap V2",
            DexType::UniswapV3 => "Uniswap V3",
            DexType::UniswapV4 => "Uniswap V4",
            DexType::Curve => "Curve",
            DexType::PancakeSwapV2 => "PancakeSwap V2",
            DexType::PancakeSwapV3 => "PancakeSwap V3",
            DexType::SushiSwap => "SushiSwap",
            DexType::SushiSwapV2 => "SushiSwap V2",
            DexType::SushiSwapV3 => "SushiSwap V3",
        }
    }

    /// 是否是 V3 类型的 DEX (集中流动性)
    pub fn is_v3_style(&self) -> bool {
        matches!(self, DexType::UniswapV3 | DexType::UniswapV4 | DexType::PancakeSwapV3 | DexType::SushiSwapV3)
    }

    /// 是否是 V2 类型的 DEX (恒定乘积)
    pub fn is_v2_style(&self) -> bool {
        matches!(self, DexType::UniswapV2 | DexType::PancakeSwapV2 | DexType::SushiSwap | DexType::SushiSwapV2)
    }
}

/// 流动性池基础信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pool {
    pub address: Address,
    pub dex_type: DexType,
    pub token0: Address,
    pub token1: Address,
    pub fee: u32,           // 费率 (以 1e6 为基数, 如 3000 = 0.3%)
    pub chain_id: u64,
}

/// Uniswap V2 风格的池状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniswapV2PoolState {
    pub pool: Pool,
    pub reserve0: U256,
    pub reserve1: U256,
    pub block_timestamp_last: u32,
}

impl UniswapV2PoolState {
    /// 计算给定输入量的输出量 (包含手续费)
    pub fn get_amount_out(&self, amount_in: U256, zero_for_one: bool) -> U256 {
        let (reserve_in, reserve_out) = if zero_for_one {
            (self.reserve0, self.reserve1)
        } else {
            (self.reserve1, self.reserve0)
        };

        if reserve_in.is_zero() || reserve_out.is_zero() {
            return U256::zero();
        }

        // Uniswap V2 公式: amountOut = (amountIn * 997 * reserveOut) / (reserveIn * 1000 + amountIn * 997)
        let amount_in_with_fee = amount_in * U256::from(997);
        let numerator = amount_in_with_fee * reserve_out;
        let denominator = reserve_in * U256::from(1000) + amount_in_with_fee;

        numerator / denominator
    }

    /// 计算价格 (token1/token0)
    pub fn get_price(&self) -> Decimal {
        if self.reserve0.is_zero() {
            return Decimal::ZERO;
        }

        let r0 = Decimal::from_str_exact(&self.reserve0.to_string()).unwrap_or(Decimal::ZERO);
        let r1 = Decimal::from_str_exact(&self.reserve1.to_string()).unwrap_or(Decimal::ZERO);

        r1 / r0
    }
}

/// Uniswap V3 风格的池状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniswapV3PoolState {
    pub pool: Pool,
    pub sqrt_price_x96: U256,
    pub tick: i32,
    pub liquidity: u128,
    pub fee_growth_global0_x128: U256,
    pub fee_growth_global1_x128: U256,
}

impl UniswapV3PoolState {
    /// 从 sqrtPriceX96 计算价格
    pub fn get_price(&self) -> Decimal {
        // price = (sqrtPriceX96 / 2^96)^2 = sqrtPriceX96^2 / 2^192
        let sqrt_price = Decimal::from_str_exact(&self.sqrt_price_x96.to_string())
            .unwrap_or(Decimal::ZERO);
        let q96 = Decimal::from(2u128.pow(96));

        let price = (sqrt_price / q96).powi(2);
        price
    }
}

/// Curve 池状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurvePoolState {
    pub pool: Pool,
    pub balances: Vec<U256>,
    pub a: U256,              // 放大系数
    pub fee: U256,            // 费率
    pub admin_fee: U256,      // 管理费
    pub virtual_price: U256,  // 虚拟价格
}

/// Uniswap V4 PoolKey - 唯一标识一个池子
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniswapV4PoolKey {
    pub currency0: Address,
    pub currency1: Address,
    pub fee: u32,
    pub tick_spacing: i32,
    pub hooks: Address,
}

/// Uniswap V4 池状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniswapV4PoolState {
    pub pool: Pool,
    pub pool_key: UniswapV4PoolKey,
    pub sqrt_price_x96: U256,
    pub tick: i32,
    pub liquidity: u128,
    pub fee_growth_global0_x128: U256,
    pub fee_growth_global1_x128: U256,
    /// Protocol fee (dynamic fees in V4)
    pub protocol_fee: u32,
}

impl UniswapV4PoolState {
    /// 从 sqrtPriceX96 计算价格
    pub fn get_price(&self) -> Decimal {
        let sqrt_price = Decimal::from_str_exact(&self.sqrt_price_x96.to_string())
            .unwrap_or(Decimal::ZERO);
        let q96 = Decimal::from(2u128.pow(96));

        let price = (sqrt_price / q96).powi(2);
        price
    }

    /// 计算池子 ID (PoolKey hash)
    pub fn pool_id(&self) -> [u8; 32] {
        use ethers::utils::keccak256;
        use ethers::abi::{encode, Token};

        let encoded = encode(&[
            Token::Address(self.pool_key.currency0),
            Token::Address(self.pool_key.currency1),
            Token::Uint(self.pool_key.fee.into()),
            Token::Int(self.pool_key.tick_spacing.into()),
            Token::Address(self.pool_key.hooks),
        ]);

        keccak256(&encoded)
    }
}

/// 通用池状态枚举
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PoolState {
    UniswapV2(UniswapV2PoolState),
    UniswapV3(UniswapV3PoolState),
    UniswapV4(UniswapV4PoolState),
    Curve(CurvePoolState),
}

impl PoolState {
    pub fn pool(&self) -> &Pool {
        match self {
            PoolState::UniswapV2(s) => &s.pool,
            PoolState::UniswapV3(s) => &s.pool,
            PoolState::UniswapV4(s) => &s.pool,
            PoolState::Curve(s) => &s.pool,
        }
    }

    pub fn dex_type(&self) -> DexType {
        self.pool().dex_type
    }

    /// 获取池子的 sqrtPriceX96 (V3/V4)
    pub fn sqrt_price_x96(&self) -> Option<U256> {
        match self {
            PoolState::UniswapV3(s) => Some(s.sqrt_price_x96),
            PoolState::UniswapV4(s) => Some(s.sqrt_price_x96),
            _ => None,
        }
    }

    /// 获取池子的流动性
    pub fn liquidity(&self) -> Option<u128> {
        match self {
            PoolState::UniswapV3(s) => Some(s.liquidity),
            PoolState::UniswapV4(s) => Some(s.liquidity),
            _ => None,
        }
    }

    /// 获取池子的 tick
    pub fn tick(&self) -> Option<i32> {
        match self {
            PoolState::UniswapV3(s) => Some(s.tick),
            PoolState::UniswapV4(s) => Some(s.tick),
            _ => None,
        }
    }
}
