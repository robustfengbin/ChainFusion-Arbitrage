use anyhow::Result;
use async_trait::async_trait;
use ethers::types::{Address, U256};
use models::{DexType, PoolState};

/// DEX 交互的通用 trait
#[async_trait]
pub trait DexProtocol: Send + Sync {
    /// 获取 DEX 类型
    fn dex_type(&self) -> DexType;

    /// 获取池子状态
    async fn get_pool_state(&self, pool_address: Address) -> Result<PoolState>;

    /// 计算输出数量
    async fn get_amount_out(
        &self,
        pool_address: Address,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> Result<U256>;

    /// 计算输入数量
    async fn get_amount_in(
        &self,
        pool_address: Address,
        token_in: Address,
        token_out: Address,
        amount_out: U256,
    ) -> Result<U256>;

    /// 获取池子中的代币对
    async fn get_pool_tokens(&self, pool_address: Address) -> Result<(Address, Address)>;

    /// 获取池子费率 (以 1e6 为基数)
    async fn get_pool_fee(&self, pool_address: Address) -> Result<u32>;
}

/// 路径计算器
pub struct PathCalculator;

impl PathCalculator {
    /// 查找三角套利路径
    ///
    /// 例如: USDT -> ETH -> DAI -> USDT
    pub fn find_triangular_paths(
        start_token: Address,
        available_pools: &[(Address, Address, Address)], // (pool, token0, token1)
        max_depth: usize,
    ) -> Vec<Vec<Address>> {
        let mut paths = Vec::new();
        let mut current_path = vec![start_token];

        Self::dfs_find_paths(
            start_token,
            start_token,
            available_pools,
            &mut current_path,
            &mut paths,
            max_depth,
        );

        paths
    }

    fn dfs_find_paths(
        start: Address,
        current: Address,
        pools: &[(Address, Address, Address)],
        current_path: &mut Vec<Address>,
        found_paths: &mut Vec<Vec<Address>>,
        max_depth: usize,
    ) {
        if current_path.len() > max_depth + 1 {
            return;
        }

        for (_pool, token0, token1) in pools {
            let next_token = if *token0 == current && !current_path.contains(token1) {
                Some(*token1)
            } else if *token1 == current && !current_path.contains(token0) {
                Some(*token0)
            } else if *token0 == current && *token1 == start && current_path.len() > 1 {
                // 找到回到起点的路径
                let mut path = current_path.clone();
                path.push(start);
                found_paths.push(path);
                continue;
            } else if *token1 == current && *token0 == start && current_path.len() > 1 {
                let mut path = current_path.clone();
                path.push(start);
                found_paths.push(path);
                continue;
            } else {
                None
            };

            if let Some(next) = next_token {
                current_path.push(next);
                Self::dfs_find_paths(start, next, pools, current_path, found_paths, max_depth);
                current_path.pop();
            }
        }
    }
}

/// 流动性聚合器
pub struct LiquidityAggregator {
    protocols: Vec<Box<dyn DexProtocol>>,
}

impl LiquidityAggregator {
    pub fn new() -> Self {
        Self {
            protocols: Vec::new(),
        }
    }

    pub fn add_protocol(&mut self, protocol: Box<dyn DexProtocol>) {
        self.protocols.push(protocol);
    }

    /// 获取最佳报价
    pub async fn get_best_quote(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        pools: &[(Address, DexType)],
    ) -> Result<Option<(Address, U256, DexType)>> {
        let mut best: Option<(Address, U256, DexType)> = None;

        for (pool_address, dex_type) in pools {
            for protocol in &self.protocols {
                if protocol.dex_type() == *dex_type {
                    match protocol.get_amount_out(*pool_address, token_in, token_out, amount_in).await {
                        Ok(amount_out) => {
                            if let Some((_, best_amount, _)) = &best {
                                if amount_out > *best_amount {
                                    best = Some((*pool_address, amount_out, *dex_type));
                                }
                            } else {
                                best = Some((*pool_address, amount_out, *dex_type));
                            }
                        }
                        Err(_) => continue,
                    }
                }
            }
        }

        Ok(best)
    }
}

impl Default for LiquidityAggregator {
    fn default() -> Self {
        Self::new()
    }
}
