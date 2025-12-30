use anyhow::Result;
use async_trait::async_trait;
use ethers::prelude::*;
use ethers::types::{Address, U256};
use models::{DexType, Pool, PoolState, CurvePoolState};
use std::sync::Arc;

use crate::common::DexProtocol;

// Curve StableSwap Pool ABI (简化版)
abigen!(
    CurvePool,
    r#"[
        function coins(uint256 i) external view returns (address)
        function balances(uint256 i) external view returns (uint256)
        function A() external view returns (uint256)
        function fee() external view returns (uint256)
        function admin_fee() external view returns (uint256)
        function get_virtual_price() external view returns (uint256)
        function get_dy(int128 i, int128 j, uint256 dx) external view returns (uint256)
        function get_dx(int128 i, int128 j, uint256 dy) external view returns (uint256)
        function exchange(int128 i, int128 j, uint256 dx, uint256 min_dy) external returns (uint256)
    ]"#
);

/// Curve 协议实现
///
/// 注意: 这是一个框架实现，完整功能需要后续开发
pub struct CurveProtocol<M: Middleware> {
    provider: Arc<M>,
    chain_id: u64,
}

impl<M: Middleware + 'static> CurveProtocol<M> {
    pub fn new(provider: Arc<M>, chain_id: u64) -> Self {
        Self { provider, chain_id }
    }

    /// 获取池子合约
    fn get_pool_contract(&self, pool_address: Address) -> CurvePool<M> {
        CurvePool::new(pool_address, self.provider.clone())
    }

    /// 获取池子中的代币数量
    pub async fn get_balances(&self, pool_address: Address, num_coins: usize) -> Result<Vec<U256>> {
        let pool = self.get_pool_contract(pool_address);
        let mut balances = Vec::new();

        for i in 0..num_coins {
            let balance = pool.balances(U256::from(i)).call().await?;
            balances.push(balance);
        }

        Ok(balances)
    }

    /// 获取交换输出量
    pub async fn get_dy(
        &self,
        pool_address: Address,
        i: i128,
        j: i128,
        dx: U256,
    ) -> Result<U256> {
        let pool = self.get_pool_contract(pool_address);
        let dy = pool.get_dy(i, j, dx).call().await?;
        Ok(dy)
    }

    /// 获取虚拟价格
    pub async fn get_virtual_price(&self, pool_address: Address) -> Result<U256> {
        let pool = self.get_pool_contract(pool_address);
        let price = pool.get_virtual_price().call().await?;
        Ok(price)
    }
}

#[async_trait]
impl<M: Middleware + 'static> DexProtocol for CurveProtocol<M> {
    fn dex_type(&self) -> DexType {
        DexType::Curve
    }

    async fn get_pool_state(&self, pool_address: Address) -> Result<PoolState> {
        let pool = self.get_pool_contract(pool_address);

        // 获取池子基本信息
        let token0 = pool.coins(U256::zero()).call().await?;
        let token1 = pool.coins(U256::from(1)).call().await?;

        let a = pool.a().call().await?;
        let fee = pool.fee().call().await?;
        let admin_fee = pool.admin_fee().call().await?;
        let virtual_price = pool.get_virtual_price().call().await?;

        // 获取余额
        let balance0 = pool.balances(U256::zero()).call().await?;
        let balance1 = pool.balances(U256::from(1)).call().await?;

        let pool_info = Pool {
            address: pool_address,
            dex_type: DexType::Curve,
            token0,
            token1,
            fee: (fee.as_u64() / 1_000_000) as u32, // Curve fee is in 1e10
            chain_id: self.chain_id,
        };

        Ok(PoolState::Curve(CurvePoolState {
            pool: pool_info,
            balances: vec![balance0, balance1],
            a,
            fee,
            admin_fee,
            virtual_price,
        }))
    }

    async fn get_amount_out(
        &self,
        pool_address: Address,
        token_in: Address,
        _token_out: Address,
        amount_in: U256,
    ) -> Result<U256> {
        // 获取 token 索引
        let pool = self.get_pool_contract(pool_address);
        let token0 = pool.coins(U256::zero()).call().await?;

        let (i, j) = if token_in == token0 {
            (0i128, 1i128)
        } else {
            (1i128, 0i128)
        };

        self.get_dy(pool_address, i, j, amount_in).await
    }

    async fn get_amount_in(
        &self,
        pool_address: Address,
        token_in: Address,
        _token_out: Address,
        amount_out: U256,
    ) -> Result<U256> {
        let pool = self.get_pool_contract(pool_address);
        let token0 = pool.coins(U256::zero()).call().await?;

        let (i, j) = if token_in == token0 {
            (0i128, 1i128)
        } else {
            (1i128, 0i128)
        };

        // Curve 的 get_dx 可能不是所有池子都支持
        let dx = pool.get_dx(i, j, amount_out).call().await?;
        Ok(dx)
    }

    async fn get_pool_tokens(&self, pool_address: Address) -> Result<(Address, Address)> {
        let pool = self.get_pool_contract(pool_address);
        let token0 = pool.coins(U256::zero()).call().await?;
        let token1 = pool.coins(U256::from(1)).call().await?;
        Ok((token0, token1))
    }

    async fn get_pool_fee(&self, pool_address: Address) -> Result<u32> {
        let pool = self.get_pool_contract(pool_address);
        let fee = pool.fee().call().await?;
        // Curve fee 是 1e10 基数，转换为 1e6 基数
        Ok((fee.as_u64() / 10000) as u32)
    }
}
