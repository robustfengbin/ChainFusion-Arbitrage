use ethers::types::{Address, U256};
use models::{ArbitragePath, DexType, SwapHop};
use std::collections::{HashMap, HashSet};

/// 路径查找器配置
#[derive(Debug, Clone)]
pub struct PathFinderConfig {
    pub max_hops: usize,
    pub min_liquidity: U256,
    pub allowed_dexes: Vec<DexType>,
}

impl Default for PathFinderConfig {
    fn default() -> Self {
        Self {
            max_hops: 3,
            min_liquidity: U256::from(10000) * U256::exp10(18), // $10000 最小流动性
            allowed_dexes: vec![
                DexType::UniswapV2,
                DexType::UniswapV3,
                DexType::Curve,
                DexType::PancakeSwapV2,
            ],
        }
    }
}

/// 池子信息
#[derive(Debug, Clone)]
pub struct PoolInfo {
    pub address: Address,
    pub token0: Address,
    pub token1: Address,
    pub dex_type: DexType,
    pub fee: u32,
    pub liquidity: U256,
}

/// 路径查找器
pub struct PathFinder {
    config: PathFinderConfig,
    /// token -> 相关的池子列表
    token_pools: HashMap<Address, Vec<PoolInfo>>,
    /// 所有池子
    all_pools: Vec<PoolInfo>,
}

impl PathFinder {
    pub fn new(config: PathFinderConfig) -> Self {
        Self {
            config,
            token_pools: HashMap::new(),
            all_pools: Vec::new(),
        }
    }

    /// 添加池子
    pub fn add_pool(&mut self, pool: PoolInfo) {
        // 添加到 token -> pools 映射
        self.token_pools
            .entry(pool.token0)
            .or_insert_with(Vec::new)
            .push(pool.clone());
        self.token_pools
            .entry(pool.token1)
            .or_insert_with(Vec::new)
            .push(pool.clone());

        self.all_pools.push(pool);
    }

    /// 查找从 start_token 开始的三角套利路径
    pub fn find_triangular_paths(&self, start_token: Address) -> Vec<ArbitragePath> {
        let mut paths = Vec::new();
        let mut visited = HashSet::new();
        let mut current_path = ArbitragePath::new(start_token, 1);

        self.dfs_find_paths(
            start_token,
            start_token,
            &mut visited,
            &mut current_path,
            &mut paths,
        );

        paths
    }

    fn dfs_find_paths(
        &self,
        start: Address,
        current: Address,
        visited: &mut HashSet<Address>,
        current_path: &mut ArbitragePath,
        found_paths: &mut Vec<ArbitragePath>,
    ) {
        if current_path.len() > self.config.max_hops {
            return;
        }

        // 获取当前 token 相关的池子
        let pools = match self.token_pools.get(&current) {
            Some(p) => p,
            None => return,
        };

        for pool in pools {
            // 检查流动性
            if pool.liquidity < self.config.min_liquidity {
                continue;
            }

            // 检查是否在允许的 DEX 列表中
            if !self.config.allowed_dexes.contains(&pool.dex_type) {
                continue;
            }

            // 确定下一个 token
            let next_token = if pool.token0 == current {
                pool.token1
            } else {
                pool.token0
            };

            // 检查是否回到起点
            if next_token == start && current_path.len() >= 2 {
                // 找到一条有效路径
                let mut path = current_path.clone();
                path.add_hop(SwapHop {
                    pool_address: pool.address,
                    dex_type: pool.dex_type,
                    token_in: current,
                    token_out: next_token,
                    fee: pool.fee,
                });
                found_paths.push(path);
                continue;
            }

            // 避免重复访问
            if visited.contains(&next_token) {
                continue;
            }

            // 继续搜索
            visited.insert(next_token);
            current_path.add_hop(SwapHop {
                pool_address: pool.address,
                dex_type: pool.dex_type,
                token_in: current,
                token_out: next_token,
                fee: pool.fee,
            });

            self.dfs_find_paths(start, next_token, visited, current_path, found_paths);

            // 回溯
            current_path.hops.pop();
            visited.remove(&next_token);
        }
    }

    /// 查找跨 DEX 套利路径
    ///
    /// 在 DEX A 买入，在 DEX B 卖出
    pub fn find_cross_dex_paths(
        &self,
        token_a: Address,
        token_b: Address,
    ) -> Vec<(PoolInfo, PoolInfo)> {
        let mut paths = Vec::new();

        // 获取 token_a -> token_b 的所有池子
        let a_pools: Vec<_> = self
            .all_pools
            .iter()
            .filter(|p| {
                (p.token0 == token_a && p.token1 == token_b)
                    || (p.token0 == token_b && p.token1 == token_a)
            })
            .collect();

        // 找出不同 DEX 的池子对
        for i in 0..a_pools.len() {
            for j in (i + 1)..a_pools.len() {
                if a_pools[i].dex_type != a_pools[j].dex_type {
                    paths.push((a_pools[i].clone(), a_pools[j].clone()));
                }
            }
        }

        paths
    }

    /// 获取所有已添加的池子数量
    pub fn pool_count(&self) -> usize {
        self.all_pools.len()
    }

    /// 清空所有池子
    pub fn clear(&mut self) {
        self.token_pools.clear();
        self.all_pools.clear();
    }
}
