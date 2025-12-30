use anyhow::Result;
use async_trait::async_trait;
use ethers::prelude::*;
use ethers::types::{Address, U256};
use models::{DexType, Pool, PoolState, UniswapV2PoolState};
use std::sync::Arc;

use crate::common::DexProtocol;
use super::contracts::pancake_v2_addresses;

// PancakeSwap V2 使用与 Uniswap V2 相同的 ABI
abigen!(
    PancakeV2Pair,
    r#"[
        function token0() external view returns (address)
        function token1() external view returns (address)
        function getReserves() external view returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast)
        function totalSupply() external view returns (uint256)
    ]"#
);

abigen!(
    PancakeV2Factory,
    r#"[
        function getPair(address tokenA, address tokenB) external view returns (address pair)
        function allPairs(uint) external view returns (address pair)
        function allPairsLength() external view returns (uint)
    ]"#
);

/// PancakeSwap V2 协议实现
///
/// 注意: PancakeSwap V2 与 Uniswap V2 协议相似，但费率为 0.25% (而非 0.3%)
pub struct PancakeSwapV2Protocol<M: Middleware> {
    provider: Arc<M>,
    factory_address: Address,
    chain_id: u64,
}

impl<M: Middleware + 'static> PancakeSwapV2Protocol<M> {
    pub fn new(provider: Arc<M>, chain_id: u64) -> Self {
        Self {
            provider,
            factory_address: *pancake_v2_addresses::FACTORY,
            chain_id,
        }
    }

    fn get_pair_contract(&self, pair_address: Address) -> PancakeV2Pair<M> {
        PancakeV2Pair::new(pair_address, self.provider.clone())
    }

    fn get_factory_contract(&self) -> PancakeV2Factory<M> {
        PancakeV2Factory::new(self.factory_address, self.provider.clone())
    }

    /// 查找池子地址
    pub async fn get_pair_address(&self, token_a: Address, token_b: Address) -> Result<Option<Address>> {
        let factory = self.get_factory_contract();
        let pair = factory.get_pair(token_a, token_b).call().await?;

        if pair == Address::zero() {
            Ok(None)
        } else {
            Ok(Some(pair))
        }
    }

    /// 计算输出数量 (PancakeSwap V2 使用 0.25% fee = 9975/10000)
    pub fn calculate_amount_out(
        amount_in: U256,
        reserve_in: U256,
        reserve_out: U256,
    ) -> U256 {
        if amount_in.is_zero() || reserve_in.is_zero() || reserve_out.is_zero() {
            return U256::zero();
        }

        // PancakeSwap V2 公式 (0.25% fee):
        // amountOut = (amountIn * 9975 * reserveOut) / (reserveIn * 10000 + amountIn * 9975)
        let amount_in_with_fee = amount_in * U256::from(9975);
        let numerator = amount_in_with_fee * reserve_out;
        let denominator = reserve_in * U256::from(10000) + amount_in_with_fee;

        numerator / denominator
    }
}

#[async_trait]
impl<M: Middleware + 'static> DexProtocol for PancakeSwapV2Protocol<M> {
    fn dex_type(&self) -> DexType {
        DexType::PancakeSwapV2
    }

    async fn get_pool_state(&self, pool_address: Address) -> Result<PoolState> {
        let pair = self.get_pair_contract(pool_address);

        let token0 = pair.token_0().call().await?;
        let token1 = pair.token_1().call().await?;
        let (reserve0, reserve1, timestamp) = pair.get_reserves().call().await?;

        let pool = Pool {
            address: pool_address,
            dex_type: DexType::PancakeSwapV2,
            token0,
            token1,
            fee: 2500, // 0.25% fee
            chain_id: self.chain_id,
        };

        Ok(PoolState::UniswapV2(UniswapV2PoolState {
            pool,
            reserve0: U256::from(reserve0),
            reserve1: U256::from(reserve1),
            block_timestamp_last: timestamp,
        }))
    }

    async fn get_amount_out(
        &self,
        pool_address: Address,
        token_in: Address,
        _token_out: Address,
        amount_in: U256,
    ) -> Result<U256> {
        let pair = self.get_pair_contract(pool_address);

        let token0 = pair.token_0().call().await?;
        let (reserve0, reserve1, _) = pair.get_reserves().call().await?;

        let (reserve_in, reserve_out) = if token_in == token0 {
            (U256::from(reserve0), U256::from(reserve1))
        } else {
            (U256::from(reserve1), U256::from(reserve0))
        };

        Ok(Self::calculate_amount_out(amount_in, reserve_in, reserve_out))
    }

    async fn get_amount_in(
        &self,
        pool_address: Address,
        token_in: Address,
        _token_out: Address,
        amount_out: U256,
    ) -> Result<U256> {
        let pair = self.get_pair_contract(pool_address);

        let token0 = pair.token_0().call().await?;
        let (reserve0, reserve1, _) = pair.get_reserves().call().await?;

        let (reserve_in, reserve_out) = if token_in == token0 {
            (U256::from(reserve0), U256::from(reserve1))
        } else {
            (U256::from(reserve1), U256::from(reserve0))
        };

        if amount_out >= reserve_out {
            return Ok(U256::MAX);
        }

        // amountIn = (reserveIn * amountOut * 10000) / ((reserveOut - amountOut) * 9975) + 1
        let numerator = reserve_in * amount_out * U256::from(10000);
        let denominator = (reserve_out - amount_out) * U256::from(9975);

        Ok((numerator / denominator) + U256::from(1))
    }

    async fn get_pool_tokens(&self, pool_address: Address) -> Result<(Address, Address)> {
        let pair = self.get_pair_contract(pool_address);
        let token0 = pair.token_0().call().await?;
        let token1 = pair.token_1().call().await?;
        Ok((token0, token1))
    }

    async fn get_pool_fee(&self, _pool_address: Address) -> Result<u32> {
        // PancakeSwap V2 固定 0.25% 费率
        Ok(2500)
    }
}
