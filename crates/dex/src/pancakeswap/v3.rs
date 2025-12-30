use anyhow::Result;
use async_trait::async_trait;
use ethers::prelude::*;
use ethers::types::{Address, U256};
use models::{DexType, Pool, PoolState, UniswapV3PoolState};
use std::sync::Arc;
use tracing::warn;

use crate::common::DexProtocol;
use super::contracts::pancake_v3_addresses;

// PancakeSwap V3 使用与 Uniswap V3 类似的 ABI
abigen!(
    PancakeV3Pool,
    r#"[
        function token0() external view returns (address)
        function token1() external view returns (address)
        function fee() external view returns (uint24)
        function liquidity() external view returns (uint128)
        function slot0() external view returns (uint160 sqrtPriceX96, int24 tick, uint16 observationIndex, uint16 observationCardinality, uint16 observationCardinalityNext, uint32 feeProtocol, bool unlocked)
        function feeGrowthGlobal0X128() external view returns (uint256)
        function feeGrowthGlobal1X128() external view returns (uint256)
    ]"#
);

abigen!(
    PancakeV3Factory,
    r#"[
        function getPool(address tokenA, address tokenB, uint24 fee) external view returns (address pool)
    ]"#
);

// PancakeSwap V3 Quoter 暂不使用，因为 ABI 复杂
// 后续可通过直接调用合约实现

/// PancakeSwap V3 协议实现
///
/// 注意: 这是一个框架实现，完整功能需要后续开发
#[allow(dead_code)]
pub struct PancakeSwapV3Protocol<M: Middleware> {
    provider: Arc<M>,
    factory_address: Address,
    quoter_address: Address,
    chain_id: u64,
}

impl<M: Middleware + 'static> PancakeSwapV3Protocol<M> {
    pub fn new(provider: Arc<M>, chain_id: u64) -> Self {
        Self {
            provider,
            factory_address: *pancake_v3_addresses::FACTORY,
            quoter_address: *pancake_v3_addresses::QUOTER,
            chain_id,
        }
    }

    fn get_pool_contract(&self, pool_address: Address) -> PancakeV3Pool<M> {
        PancakeV3Pool::new(pool_address, self.provider.clone())
    }

    fn get_factory_contract(&self) -> PancakeV3Factory<M> {
        PancakeV3Factory::new(self.factory_address, self.provider.clone())
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
}

#[async_trait]
impl<M: Middleware + 'static> DexProtocol for PancakeSwapV3Protocol<M> {
    fn dex_type(&self) -> DexType {
        DexType::PancakeSwapV3
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
            dex_type: DexType::PancakeSwapV3,
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
        _pool_address: Address,
        _token_in: Address,
        _token_out: Address,
        _amount_in: U256,
    ) -> Result<U256> {
        // TODO: 实现 PancakeSwap V3 Quoter 调用
        // 目前返回 0，需要后续完善
        warn!("PancakeSwap V3 get_amount_out 尚未完全实现");
        Ok(U256::zero())
    }

    async fn get_amount_in(
        &self,
        _pool_address: Address,
        _token_in: Address,
        _token_out: Address,
        _amount_out: U256,
    ) -> Result<U256> {
        // TODO: 实现 PancakeSwap V3 Quoter 调用
        warn!("PancakeSwap V3 get_amount_in 尚未完全实现");
        Ok(U256::zero())
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
