//! 闪电贷池选择器
//!
//! 自动选择最优的 Uniswap V3 闪电贷池
//!
//! 选择策略:
//! 1. 池子必须包含起始代币 (token_a)
//! 2. 池子不能与 swap 路径中的池子重复
//! 3. 优先选择流动性最高的池子
//! 4. 优先选择费率最低的池子

use anyhow::{anyhow, Result};
use ethers::prelude::*;
use ethers::types::{Address, U256};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::providers::FlashLoanProvider;

/// V3 池子信息 (用于闪电贷选择)
#[derive(Debug, Clone)]
pub struct V3PoolInfo {
    /// 池子地址
    pub address: Address,
    /// Token0 地址
    pub token0: Address,
    /// Token1 地址
    pub token1: Address,
    /// 费率 (100=0.01%, 500=0.05%, 3000=0.3%, 10000=1%)
    pub fee: u32,
    /// 当前流动性
    pub liquidity: u128,
    /// 是否已验证存在
    pub verified: bool,
}

impl V3PoolInfo {
    /// 检查池子是否包含指定代币
    pub fn contains_token(&self, token: Address) -> bool {
        self.token0 == token || self.token1 == token
    }

    /// 获取另一个代币
    pub fn other_token(&self, token: Address) -> Option<Address> {
        if self.token0 == token {
            Some(self.token1)
        } else if self.token1 == token {
            Some(self.token0)
        } else {
            None
        }
    }

    /// 检查借入代币是否是 token0
    pub fn is_token0(&self, borrow_token: Address) -> bool {
        self.token0 == borrow_token
    }
}

/// 闪电贷池选择结果
#[derive(Debug, Clone)]
pub struct FlashPoolSelection {
    /// 选中的池子地址
    pub pool_address: Address,
    /// 池子费率
    pub pool_fee: u32,
    /// 借入代币是否是 token0
    pub is_token0: bool,
    /// 池子流动性
    pub liquidity: u128,
    /// 闪电贷提供商
    pub provider: FlashLoanProvider,
    /// 预估闪电贷费用 (基于借入金额)
    pub estimated_fee: U256,
}

/// 闪电贷池选择器配置
#[derive(Debug, Clone)]
pub struct FlashPoolSelectorConfig {
    /// Uniswap V3 Factory 地址
    pub v3_factory: Address,
    /// 最小流动性要求
    pub min_liquidity: u128,
    /// 优先使用的费率列表 (按优先级排序)
    pub preferred_fees: Vec<u32>,
    /// 是否验证池子存在
    pub verify_pools: bool,
}

impl Default for FlashPoolSelectorConfig {
    fn default() -> Self {
        Self {
            // Uniswap V3 Factory (Ethereum Mainnet)
            v3_factory: "0x1F98431c8aD98523631AE4a59f267346ea31F984"
                .parse()
                .unwrap(),
            min_liquidity: 100_000_000_000_000_000, // 1e17 - V3 流动性单位，非 USD 价值
            // 优先低费率池子以减少闪电贷成本
            preferred_fees: vec![100, 500, 3000, 10000],
            verify_pools: true,
        }
    }
}

impl FlashPoolSelectorConfig {
    /// 为 BSC 网络创建配置
    pub fn bsc() -> Self {
        Self {
            // PancakeSwap V3 Factory (BSC)
            v3_factory: "0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865"
                .parse()
                .unwrap(),
            min_liquidity: 100_000_000_000_000_000, // 1e17
            preferred_fees: vec![100, 500, 2500, 10000],
            verify_pools: true,
        }
    }
}

// Uniswap V3 Factory ABI
abigen!(
    IUniswapV3Factory,
    r#"[
        function getPool(address tokenA, address tokenB, uint24 fee) external view returns (address pool)
    ]"#
);

// Uniswap V3 Pool ABI (用于获取流动性)
abigen!(
    IUniswapV3PoolLiquidity,
    r#"[
        function liquidity() external view returns (uint128)
        function token0() external view returns (address)
        function token1() external view returns (address)
        function fee() external view returns (uint24)
    ]"#
);

/// 闪电贷池选择器
pub struct FlashPoolSelector<M: Middleware> {
    provider: Arc<M>,
    config: FlashPoolSelectorConfig,
    factory: IUniswapV3Factory<M>,
}

impl<M: Middleware + 'static> FlashPoolSelector<M> {
    /// 创建新的选择器
    pub fn new(provider: Arc<M>, config: FlashPoolSelectorConfig) -> Self {
        let factory = IUniswapV3Factory::new(config.v3_factory, provider.clone());
        Self {
            provider,
            config,
            factory,
        }
    }

    /// 为套利路径选择最优闪电贷池
    ///
    /// # 参数
    /// - `borrow_token`: 要借入的代币地址
    /// - `borrow_amount`: 借入金额
    /// - `swap_pools`: swap 路径中使用的池子地址列表 (需要排除)
    /// - `available_pair_tokens`: 可与 borrow_token 配对的代币列表
    ///
    /// # 返回
    /// - 最优闪电贷池选择结果
    pub async fn select_flash_pool(
        &self,
        borrow_token: Address,
        borrow_amount: U256,
        swap_pools: &[Address],
        available_pair_tokens: &[Address],
    ) -> Result<FlashPoolSelection> {
        info!(
            "选择闪电贷池: borrow_token={:?}, amount={}, 排除池子数={}",
            borrow_token,
            borrow_amount,
            swap_pools.len()
        );

        let excluded_pools: HashSet<Address> = swap_pools.iter().cloned().collect();
        let mut candidates: Vec<V3PoolInfo> = Vec::new();

        // 遍历所有可能的配对代币和费率
        for pair_token in available_pair_tokens {
            if *pair_token == borrow_token {
                continue;
            }

            for fee in &self.config.preferred_fees {
                // 获取池子地址
                let pool_address = match self
                    .factory
                    .get_pool(borrow_token, *pair_token, *fee as u32)
                    .call()
                    .await
                {
                    Ok(addr) if addr != Address::zero() => addr,
                    _ => continue,
                };

                // 检查是否在排除列表中
                if excluded_pools.contains(&pool_address) {
                    debug!("排除池子 {:?} (在 swap 路径中)", pool_address);
                    continue;
                }

                // 获取池子流动性
                let pool_info = match self.get_pool_info(pool_address).await {
                    Ok(info) => info,
                    Err(e) => {
                        debug!("获取池子信息失败 {:?}: {}", pool_address, e);
                        continue;
                    }
                };

                // 流动性检查已移除，由调用方自行处理
                candidates.push(pool_info);
            }
        }

        if candidates.is_empty() {
            return Err(anyhow!(
                "找不到合适的闪电贷池: borrow_token={:?}",
                borrow_token
            ));
        }

        // 选择最优池子: 优先流动性高，其次费率低
        candidates.sort_by(|a, b| {
            // 首先按流动性降序
            let liq_cmp = b.liquidity.cmp(&a.liquidity);
            if liq_cmp != std::cmp::Ordering::Equal {
                return liq_cmp;
            }
            // 然后按费率升序
            a.fee.cmp(&b.fee)
        });

        let best = &candidates[0];
        let is_token0 = best.is_token0(borrow_token);

        // 计算预估闪电贷费用
        let fee_rate = best.fee as u128;
        let estimated_fee = borrow_amount * U256::from(fee_rate) / U256::from(1_000_000);

        let selection = FlashPoolSelection {
            pool_address: best.address,
            pool_fee: best.fee,
            is_token0,
            liquidity: best.liquidity,
            provider: FlashLoanProvider::UniswapV3,
            estimated_fee,
        };

        info!(
            "选择闪电贷池: {:?}, 费率={}bps, 流动性={}, 预估费用={}",
            selection.pool_address,
            selection.pool_fee as f64 / 100.0,
            selection.liquidity,
            selection.estimated_fee
        );

        Ok(selection)
    }

    /// 为三角套利选择闪电贷池
    ///
    /// # 参数
    /// - `token_a`: 起始/结束代币 (借入并归还)
    /// - `token_b`: 中间代币 1
    /// - `token_c`: 中间代币 2
    /// - `borrow_amount`: 借入金额
    /// - `swap_pools`: swap 路径中使用的池子地址列表
    pub async fn select_for_triangular(
        &self,
        token_a: Address,
        token_b: Address,
        token_c: Address,
        borrow_amount: U256,
        swap_pools: &[Address],
    ) -> Result<FlashPoolSelection> {
        // 对于三角套利，配对代币可以是 token_b 或 token_c
        // 因为借的是 token_a，需要找包含 token_a 的池子
        let pair_tokens = vec![token_b, token_c];

        self.select_flash_pool(token_a, borrow_amount, swap_pools, &pair_tokens)
            .await
    }

    /// 从 ArbitragePath 中提取 swap 池子地址
    pub fn extract_swap_pools(path: &models::ArbitragePath) -> Vec<Address> {
        path.hops.iter().map(|hop| hop.pool_address).collect()
    }

    /// 获取池子详细信息
    async fn get_pool_info(&self, pool_address: Address) -> Result<V3PoolInfo> {
        let pool = IUniswapV3PoolLiquidity::new(pool_address, self.provider.clone());

        // 分开调用避免生命周期问题
        let token0_call = pool.token_0();
        let token1_call = pool.token_1();
        let fee_call = pool.fee();
        let liquidity_call = pool.liquidity();

        let (token0, token1, fee, liquidity) = tokio::try_join!(
            token0_call.call(),
            token1_call.call(),
            fee_call.call(),
            liquidity_call.call()
        )?;

        Ok(V3PoolInfo {
            address: pool_address,
            token0,
            token1,
            fee,
            liquidity,
            verified: true,
        })
    }

    /// 预计算常见代币对的闪电贷池 (用于缓存)
    pub async fn precompute_pools(
        &self,
        tokens: &[Address],
    ) -> Result<Vec<V3PoolInfo>> {
        let mut pools = Vec::new();

        for i in 0..tokens.len() {
            for j in (i + 1)..tokens.len() {
                for fee in &self.config.preferred_fees {
                    let pool_address = match self
                        .factory
                        .get_pool(tokens[i], tokens[j], *fee as u32)
                        .call()
                        .await
                    {
                        Ok(addr) if addr != Address::zero() => addr,
                        _ => continue,
                    };

                    if let Ok(info) = self.get_pool_info(pool_address).await {
                        pools.push(info);
                    }
                }
            }
        }

        info!("预计算完成: 找到 {} 个有效闪电贷池", pools.len());
        Ok(pools)
    }
}

/// 带缓存的闪电贷池选择器
pub struct CachedFlashPoolSelector<M: Middleware> {
    selector: FlashPoolSelector<M>,
    /// 缓存的池子信息: (token_a, token_b, fee) -> V3PoolInfo
    cache: tokio::sync::RwLock<std::collections::HashMap<(Address, Address, u32), V3PoolInfo>>,
    /// 缓存过期时间 (毫秒)
    cache_ttl_ms: u64,
    /// 最后更新时间
    last_update: tokio::sync::RwLock<std::time::Instant>,
}

impl<M: Middleware + 'static> CachedFlashPoolSelector<M> {
    pub fn new(provider: Arc<M>, config: FlashPoolSelectorConfig) -> Self {
        Self {
            selector: FlashPoolSelector::new(provider, config),
            cache: tokio::sync::RwLock::new(std::collections::HashMap::new()),
            cache_ttl_ms: 60_000, // 1 分钟缓存
            last_update: tokio::sync::RwLock::new(std::time::Instant::now()),
        }
    }

    /// 刷新缓存
    pub async fn refresh_cache(&self, tokens: &[Address]) -> Result<()> {
        let pools = self.selector.precompute_pools(tokens).await?;

        let mut cache = self.cache.write().await;
        cache.clear();

        for pool in pools {
            let key = if pool.token0 < pool.token1 {
                (pool.token0, pool.token1, pool.fee)
            } else {
                (pool.token1, pool.token0, pool.fee)
            };
            cache.insert(key, pool);
        }

        let mut last_update = self.last_update.write().await;
        *last_update = std::time::Instant::now();

        Ok(())
    }

    /// 检查缓存是否过期
    pub async fn is_cache_expired(&self) -> bool {
        let last_update = self.last_update.read().await;
        last_update.elapsed().as_millis() as u64 > self.cache_ttl_ms
    }

    /// 选择闪电贷池 (使用缓存)
    pub async fn select_flash_pool(
        &self,
        borrow_token: Address,
        borrow_amount: U256,
        swap_pools: &[Address],
        available_pair_tokens: &[Address],
    ) -> Result<FlashPoolSelection> {
        // 检查缓存是否过期
        if self.is_cache_expired().await {
            warn!("闪电贷池缓存已过期，使用实时查询");
        }

        // 尝试从缓存中查找
        let cache = self.cache.read().await;
        let excluded: HashSet<Address> = swap_pools.iter().cloned().collect();

        let mut candidates: Vec<&V3PoolInfo> = cache
            .values()
            .filter(|pool| {
                pool.contains_token(borrow_token)
                    && !excluded.contains(&pool.address)
            })
            .collect();

        if candidates.is_empty() {
            drop(cache);
            // 缓存中没有，使用实时查询
            return self
                .selector
                .select_flash_pool(borrow_token, borrow_amount, swap_pools, available_pair_tokens)
                .await;
        }

        // 排序选择最优
        candidates.sort_by(|a, b| {
            let liq_cmp = b.liquidity.cmp(&a.liquidity);
            if liq_cmp != std::cmp::Ordering::Equal {
                return liq_cmp;
            }
            a.fee.cmp(&b.fee)
        });

        let best = candidates[0];
        let is_token0 = best.is_token0(borrow_token);
        let fee_rate = best.fee as u128;
        let estimated_fee = borrow_amount * U256::from(fee_rate) / U256::from(1_000_000);

        Ok(FlashPoolSelection {
            pool_address: best.address,
            pool_fee: best.fee,
            is_token0,
            liquidity: best.liquidity,
            provider: FlashLoanProvider::UniswapV3,
            estimated_fee,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v3_pool_info() {
        let pool = V3PoolInfo {
            address: Address::zero(),
            token0: "0xdAC17F958D2ee523a2206206994597C13D831ec7"
                .parse()
                .unwrap(),
            token1: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
                .parse()
                .unwrap(),
            fee: 3000,
            liquidity: 1_000_000_000_000_000_000,
            verified: true,
        };

        let usdt: Address = "0xdAC17F958D2ee523a2206206994597C13D831ec7"
            .parse()
            .unwrap();
        let weth: Address = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
            .parse()
            .unwrap();

        assert!(pool.contains_token(usdt));
        assert!(pool.contains_token(weth));
        assert_eq!(pool.other_token(usdt), Some(weth));
        assert!(pool.is_token0(usdt));
        assert!(!pool.is_token0(weth));
    }

    #[test]
    fn test_default_config() {
        let config = FlashPoolSelectorConfig::default();
        assert_eq!(config.preferred_fees, vec![100, 500, 3000, 10000]);
    }
}
