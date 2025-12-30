use anyhow::Result;
use async_trait::async_trait;
use ethers::prelude::*;
use ethers::types::{Address, U256};
use models::{DexType, Pool, PoolState, UniswapV3PoolState};
use std::sync::Arc;
use tracing::info;

use crate::common::DexProtocol;
use super::contracts::v3_addresses;

// Uniswap V3 Pool ABI
abigen!(
    UniswapV3Pool,
    r#"[
        function token0() external view returns (address)
        function token1() external view returns (address)
        function fee() external view returns (uint24)
        function tickSpacing() external view returns (int24)
        function liquidity() external view returns (uint128)
        function slot0() external view returns (uint160 sqrtPriceX96, int24 tick, uint16 observationIndex, uint16 observationCardinality, uint16 observationCardinalityNext, uint8 feeProtocol, bool unlocked)
        function feeGrowthGlobal0X128() external view returns (uint256)
        function feeGrowthGlobal1X128() external view returns (uint256)
        function flash(address recipient, uint256 amount0, uint256 amount1, bytes calldata data) external
        event Swap(address indexed sender, address indexed recipient, int256 amount0, int256 amount1, uint160 sqrtPriceX96, uint128 liquidity, int24 tick)
    ]"#
);

// Uniswap V3 Factory ABI
abigen!(
    UniswapV3Factory,
    r#"[
        function getPool(address tokenA, address tokenB, uint24 fee) external view returns (address pool)
        function owner() external view returns (address)
    ]"#
);

// Uniswap V3 Quoter ABI
abigen!(
    UniswapV3Quoter,
    r#"[
        function quoteExactInputSingle(address tokenIn, address tokenOut, uint24 fee, uint256 amountIn, uint160 sqrtPriceLimitX96) external returns (uint256 amountOut)
        function quoteExactOutputSingle(address tokenIn, address tokenOut, uint24 fee, uint256 amountOut, uint160 sqrtPriceLimitX96) external returns (uint256 amountIn)
    ]"#
);

/// Uniswap V3 协议实现
pub struct UniswapV3Protocol<M: Middleware> {
    provider: Arc<M>,
    factory_address: Address,
    quoter_address: Address,
    chain_id: u64,
}

impl<M: Middleware + 'static> UniswapV3Protocol<M> {
    pub fn new(provider: Arc<M>, chain_id: u64) -> Self {
        Self {
            provider,
            factory_address: *v3_addresses::FACTORY,
            quoter_address: *v3_addresses::QUOTER,
            chain_id,
        }
    }

    pub fn with_addresses(
        provider: Arc<M>,
        factory_address: Address,
        quoter_address: Address,
        chain_id: u64,
    ) -> Self {
        Self {
            provider,
            factory_address,
            quoter_address,
            chain_id,
        }
    }

    /// 获取池子合约
    fn get_pool_contract(&self, pool_address: Address) -> UniswapV3Pool<M> {
        UniswapV3Pool::new(pool_address, self.provider.clone())
    }

    /// 获取工厂合约
    fn get_factory_contract(&self) -> UniswapV3Factory<M> {
        UniswapV3Factory::new(self.factory_address, self.provider.clone())
    }

    /// 获取 Quoter 合约
    fn get_quoter_contract(&self) -> UniswapV3Quoter<M> {
        UniswapV3Quoter::new(self.quoter_address, self.provider.clone())
    }

    /// 查找池子地址
    pub async fn get_pool_address(
        &self,
        token_a: Address,
        token_b: Address,
        fee: u32,
    ) -> Result<Option<Address>> {
        let factory = self.get_factory_contract();
        let pool = factory.get_pool(token_a, token_b, fee.try_into()?).call().await?;

        if pool == Address::zero() {
            Ok(None)
        } else {
            Ok(Some(pool))
        }
    }

    /// 获取 slot0 数据
    pub async fn get_slot0(&self, pool_address: Address) -> Result<(U256, i32, bool)> {
        let pool = self.get_pool_contract(pool_address);
        let slot0 = pool.slot_0().call().await?;

        Ok((
            U256::from(slot0.0), // sqrtPriceX96
            slot0.1,             // tick
            slot0.6,             // unlocked
        ))
    }

    /// 从 sqrtPriceX96 计算价格
    pub fn sqrt_price_x96_to_price(sqrt_price_x96: U256, decimals0: u8, decimals1: u8) -> f64 {
        // price = (sqrtPriceX96 / 2^96)^2 * 10^(decimals0 - decimals1)
        let sqrt_price = sqrt_price_x96.as_u128() as f64 / (2_u128.pow(96) as f64);
        let price = sqrt_price * sqrt_price;

        let decimal_adjustment = 10_f64.powi(decimals0 as i32 - decimals1 as i32);
        price * decimal_adjustment
    }

    /// 使用 Quoter 获取精确报价
    pub async fn quote_exact_input(
        &self,
        token_in: Address,
        token_out: Address,
        fee: u32,
        amount_in: U256,
    ) -> Result<U256> {
        let quoter = self.get_quoter_contract();

        // 使用 call 模拟交易获取报价
        let amount_out = quoter
            .quote_exact_input_single(
                token_in,
                token_out,
                fee.try_into()?,
                amount_in,
                U256::zero(), // sqrtPriceLimitX96 = 0 表示无限制
            )
            .call()
            .await?;

        Ok(amount_out)
    }

    /// 使用 Quoter 获取精确输出报价
    pub async fn quote_exact_output(
        &self,
        token_in: Address,
        token_out: Address,
        fee: u32,
        amount_out: U256,
    ) -> Result<U256> {
        let quoter = self.get_quoter_contract();

        let amount_in = quoter
            .quote_exact_output_single(
                token_in,
                token_out,
                fee.try_into()?,
                amount_out,
                U256::zero(),
            )
            .call()
            .await?;

        Ok(amount_in)
    }

    /// 执行闪电贷
    pub async fn flash_loan(
        &self,
        pool_address: Address,
        recipient: Address,
        amount0: U256,
        amount1: U256,
        data: Vec<u8>,
    ) -> Result<()> {
        let pool = self.get_pool_contract(pool_address);

        // 构建闪电贷交易
        let _tx = pool.flash(recipient, amount0, amount1, data.into());

        info!(
            "执行 Uniswap V3 闪电贷: pool={:?}, amount0={}, amount1={}",
            pool_address, amount0, amount1
        );

        Ok(())
    }
}

#[async_trait]
impl<M: Middleware + 'static> DexProtocol for UniswapV3Protocol<M> {
    fn dex_type(&self) -> DexType {
        DexType::UniswapV3
    }

    async fn get_pool_state(&self, pool_address: Address) -> Result<PoolState> {
        let pool = self.get_pool_contract(pool_address);

        let token0 = pool.token_0().call().await?;
        let token1 = pool.token_1().call().await?;
        let fee: u32 = pool.fee().call().await?.try_into()?;
        let liquidity = pool.liquidity().call().await?;
        let slot0 = pool.slot_0().call().await?;
        let fee_growth0 = pool.fee_growth_global_0x128().call().await?;
        let fee_growth1 = pool.fee_growth_global_1x128().call().await?;

        let pool_info = Pool {
            address: pool_address,
            dex_type: DexType::UniswapV3,
            token0,
            token1,
            fee,
            chain_id: self.chain_id,
        };

        Ok(PoolState::UniswapV3(UniswapV3PoolState {
            pool: pool_info,
            sqrt_price_x96: U256::from(slot0.0),
            tick: slot0.1,
            liquidity,
            fee_growth_global0_x128: fee_growth0,
            fee_growth_global1_x128: fee_growth1,
        }))
    }

    async fn get_amount_out(
        &self,
        pool_address: Address,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> Result<U256> {
        let pool = self.get_pool_contract(pool_address);
        let fee: u32 = pool.fee().call().await?.try_into()?;

        self.quote_exact_input(token_in, token_out, fee, amount_in).await
    }

    async fn get_amount_in(
        &self,
        pool_address: Address,
        token_in: Address,
        token_out: Address,
        amount_out: U256,
    ) -> Result<U256> {
        let pool = self.get_pool_contract(pool_address);
        let fee: u32 = pool.fee().call().await?.try_into()?;

        self.quote_exact_output(token_in, token_out, fee, amount_out).await
    }

    async fn get_pool_tokens(&self, pool_address: Address) -> Result<(Address, Address)> {
        let pool = self.get_pool_contract(pool_address);
        let token0 = pool.token_0().call().await?;
        let token1 = pool.token_1().call().await?;
        Ok((token0, token1))
    }

    async fn get_pool_fee(&self, pool_address: Address) -> Result<u32> {
        let pool = self.get_pool_contract(pool_address);
        let fee: u32 = pool.fee().call().await?.try_into()?;
        Ok(fee)
    }
}
