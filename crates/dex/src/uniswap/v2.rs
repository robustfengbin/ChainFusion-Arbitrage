use anyhow::Result;
use async_trait::async_trait;
use ethers::prelude::*;
use ethers::types::{Address, U256};
use models::{DexType, Pool, PoolState, UniswapV2PoolState};
use std::sync::Arc;

use crate::common::DexProtocol;
use super::contracts::v2_addresses;

// Uniswap V2 Pair ABI
abigen!(
    UniswapV2Pair,
    r#"[
        function token0() external view returns (address)
        function token1() external view returns (address)
        function getReserves() external view returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast)
        function totalSupply() external view returns (uint256)
        event Swap(address indexed sender, uint amount0In, uint amount1In, uint amount0Out, uint amount1Out, address indexed to)
        event Sync(uint112 reserve0, uint112 reserve1)
    ]"#
);

// Uniswap V2 Factory ABI
abigen!(
    UniswapV2Factory,
    r#"[
        function getPair(address tokenA, address tokenB) external view returns (address pair)
        function allPairs(uint) external view returns (address pair)
        function allPairsLength() external view returns (uint)
    ]"#
);

/// Uniswap V2 协议实现
pub struct UniswapV2Protocol<M: Middleware> {
    provider: Arc<M>,
    factory_address: Address,
    chain_id: u64,
}

impl<M: Middleware + 'static> UniswapV2Protocol<M> {
    pub fn new(provider: Arc<M>, chain_id: u64) -> Self {
        Self {
            provider,
            factory_address: *v2_addresses::FACTORY,
            chain_id,
        }
    }

    pub fn with_factory(provider: Arc<M>, factory_address: Address, chain_id: u64) -> Self {
        Self {
            provider,
            factory_address,
            chain_id,
        }
    }

    /// 获取池子合约
    fn get_pair_contract(&self, pair_address: Address) -> UniswapV2Pair<M> {
        UniswapV2Pair::new(pair_address, self.provider.clone())
    }

    /// 获取工厂合约
    fn get_factory_contract(&self) -> UniswapV2Factory<M> {
        UniswapV2Factory::new(self.factory_address, self.provider.clone())
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

    /// 获取所有池子数量
    pub async fn get_all_pairs_length(&self) -> Result<u64> {
        let factory = self.get_factory_contract();
        let length = factory.all_pairs_length().call().await?;
        Ok(length.as_u64())
    }

    /// 获取储备量
    pub async fn get_reserves(&self, pair_address: Address) -> Result<(U256, U256, u32)> {
        let pair = self.get_pair_contract(pair_address);
        let (reserve0, reserve1, timestamp) = pair.get_reserves().call().await?;
        Ok((U256::from(reserve0), U256::from(reserve1), timestamp))
    }

    /// 计算输出数量 (纯计算，不调用合约)
    pub fn calculate_amount_out(
        amount_in: U256,
        reserve_in: U256,
        reserve_out: U256,
    ) -> U256 {
        if amount_in.is_zero() || reserve_in.is_zero() || reserve_out.is_zero() {
            return U256::zero();
        }

        // Uniswap V2 公式:
        // amountOut = (amountIn * 997 * reserveOut) / (reserveIn * 1000 + amountIn * 997)
        let amount_in_with_fee = amount_in * U256::from(997);
        let numerator = amount_in_with_fee * reserve_out;
        let denominator = reserve_in * U256::from(1000) + amount_in_with_fee;

        numerator / denominator
    }

    /// 计算输入数量 (纯计算)
    pub fn calculate_amount_in(
        amount_out: U256,
        reserve_in: U256,
        reserve_out: U256,
    ) -> U256 {
        if amount_out.is_zero() || reserve_in.is_zero() || reserve_out.is_zero() {
            return U256::zero();
        }

        if amount_out >= reserve_out {
            return U256::MAX; // 无法满足
        }

        // amountIn = (reserveIn * amountOut * 1000) / ((reserveOut - amountOut) * 997) + 1
        let numerator = reserve_in * amount_out * U256::from(1000);
        let denominator = (reserve_out - amount_out) * U256::from(997);

        (numerator / denominator) + U256::from(1)
    }
}

#[async_trait]
impl<M: Middleware + 'static> DexProtocol for UniswapV2Protocol<M> {
    fn dex_type(&self) -> DexType {
        DexType::UniswapV2
    }

    async fn get_pool_state(&self, pool_address: Address) -> Result<PoolState> {
        let pair = self.get_pair_contract(pool_address);

        let token0 = pair.token_0().call().await?;
        let token1 = pair.token_1().call().await?;
        let (reserve0, reserve1, timestamp) = pair.get_reserves().call().await?;

        let pool = Pool {
            address: pool_address,
            dex_type: DexType::UniswapV2,
            token0,
            token1,
            fee: 3000, // 0.3% fee
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

        Ok(Self::calculate_amount_in(amount_out, reserve_in, reserve_out))
    }

    async fn get_pool_tokens(&self, pool_address: Address) -> Result<(Address, Address)> {
        let pair = self.get_pair_contract(pool_address);
        let token0 = pair.token_0().call().await?;
        let token1 = pair.token_1().call().await?;
        Ok((token0, token1))
    }

    async fn get_pool_fee(&self, _pool_address: Address) -> Result<u32> {
        // Uniswap V2 固定 0.3% 费率
        Ok(3000)
    }
}
