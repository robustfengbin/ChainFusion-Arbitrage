use anyhow::Result;
use ethers::prelude::*;
use ethers::types::{Address, U256};
use sqlx::{MySql, Pool};
use std::sync::Arc;
use tracing::{info, warn};

use models::DexType;

/// 常用代币地址 (Ethereum Mainnet)
pub mod eth_tokens {
    use ethers::types::Address;
    use lazy_static::lazy_static;

    lazy_static! {
        pub static ref WETH: Address = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".parse().unwrap();
        pub static ref USDT: Address = "0xdAC17F958D2ee523a2206206994597C13D831ec7".parse().unwrap();
        pub static ref USDC: Address = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".parse().unwrap();
        pub static ref DAI: Address = "0x6B175474E89094C44Da98b954EedeAC495271d0F".parse().unwrap();
        pub static ref WBTC: Address = "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599".parse().unwrap();
    }
}

/// 常用代币地址 (BSC)
pub mod bsc_tokens {
    use ethers::types::Address;
    use lazy_static::lazy_static;

    lazy_static! {
        pub static ref WBNB: Address = "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c".parse().unwrap();
        pub static ref BUSD: Address = "0xe9e7CEA3DedcA5984780Bafc599bD69ADd087D56".parse().unwrap();
        pub static ref USDT: Address = "0x55d398326f99059fF775485246999027B3197955".parse().unwrap();
        pub static ref USDC: Address = "0x8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d".parse().unwrap();
    }
}

// Uniswap V2 Factory ABI
abigen!(
    IUniswapV2Factory,
    r#"[
        function getPair(address tokenA, address tokenB) external view returns (address pair)
        function allPairs(uint) external view returns (address pair)
        function allPairsLength() external view returns (uint)
    ]"#
);

// Uniswap V2 Pair ABI
abigen!(
    IUniswapV2Pair,
    r#"[
        function token0() external view returns (address)
        function token1() external view returns (address)
        function getReserves() external view returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast)
    ]"#
);

/// DEX 工厂地址
pub struct DexFactory {
    pub name: &'static str,
    pub address: Address,
    pub dex_type: DexType,
    pub chain_id: u64,
}

/// 获取 Ethereum 主网的 DEX 工厂列表
pub fn get_eth_factories() -> Vec<DexFactory> {
    vec![
        DexFactory {
            name: "UniswapV2",
            address: "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f".parse().unwrap(),
            dex_type: DexType::UniswapV2,
            chain_id: 1,
        },
        DexFactory {
            name: "SushiSwap",
            address: "0xC0AEe478e3658e2610c5F7A4A2E1777cE9e4f2Ac".parse().unwrap(),
            dex_type: DexType::SushiSwap,
            chain_id: 1,
        },
    ]
}

/// 获取 BSC 的 DEX 工厂列表
pub fn get_bsc_factories() -> Vec<DexFactory> {
    vec![
        DexFactory {
            name: "PancakeSwapV2",
            address: "0xcA143Ce32Fe78f1f7019d7d551a6402fC5350c73".parse().unwrap(),
            dex_type: DexType::PancakeSwapV2,
            chain_id: 56,
        },
    ]
}

/// 池子同步器
pub struct PoolSyncer<M: Middleware> {
    provider: Arc<M>,
    db: Pool<MySql>,
    chain_id: u64,
}

impl<M: Middleware + 'static> PoolSyncer<M> {
    pub fn new(provider: Arc<M>, db: Pool<MySql>, chain_id: u64) -> Self {
        Self {
            provider,
            db,
            chain_id,
        }
    }

    /// 同步指定工厂的池子
    pub async fn sync_factory(&self, factory: &DexFactory, limit: usize) -> Result<usize> {
        info!("同步 {} 池子 (chain_id={})", factory.name, factory.chain_id);

        let factory_contract = IUniswapV2Factory::new(factory.address, self.provider.clone());

        // 获取池子总数
        let total_pairs = factory_contract.all_pairs_length().call().await?;
        let total = total_pairs.as_u64() as usize;
        info!("{} 共有 {} 个池子", factory.name, total);

        let sync_count = std::cmp::min(limit, total);
        let mut synced = 0;

        // 从最新的池子开始同步
        for i in (total.saturating_sub(sync_count)..total).rev() {
            match self.sync_pair(factory, i as u64).await {
                Ok(true) => synced += 1,
                Ok(false) => {}
                Err(e) => {
                    warn!("同步池子 {} 失败: {}", i, e);
                }
            }

            // 避免请求过快
            if synced % 10 == 0 && synced > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }

        info!("同步完成: {} 个池子", synced);
        Ok(synced)
    }

    /// 同步单个池子
    async fn sync_pair(&self, factory: &DexFactory, index: u64) -> Result<bool> {
        let factory_contract = IUniswapV2Factory::new(factory.address, self.provider.clone());

        // 获取池子地址
        let pair_address = factory_contract.all_pairs(U256::from(index)).call().await?;

        if pair_address == Address::zero() {
            return Ok(false);
        }

        // 获取池子信息
        let pair_contract = IUniswapV2Pair::new(pair_address, self.provider.clone());

        let token0 = pair_contract.token_0().call().await?;
        let token1 = pair_contract.token_1().call().await?;
        let (reserve0, reserve1, _) = pair_contract.get_reserves().call().await?;

        // 计算流动性 (简化：使用 reserve0 + reserve1)
        let liquidity = U256::from(reserve0) + U256::from(reserve1);

        // 保存到数据库
        self.save_pool(
            pair_address,
            token0,
            token1,
            factory.dex_type,
            3000, // 0.3% fee for V2
            liquidity,
        )
        .await?;

        Ok(true)
    }

    /// 保存池子到数据库
    async fn save_pool(
        &self,
        address: Address,
        token0: Address,
        token1: Address,
        dex_type: DexType,
        fee: u32,
        liquidity: U256,
    ) -> Result<()> {
        let address_str = format!("{:?}", address);
        let token0_str = format!("{:?}", token0);
        let token1_str = format!("{:?}", token1);
        let dex_type_str = dex_type_to_string(dex_type);
        let liquidity_str = liquidity.to_string();

        sqlx::query(
            r#"
            INSERT INTO pool_cache (address, chain_id, token0, token1, dex_type, fee, liquidity, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, NOW())
            ON DUPLICATE KEY UPDATE
                liquidity = VALUES(liquidity),
                updated_at = NOW()
            "#,
        )
        .bind(&address_str)
        .bind(self.chain_id as i64)
        .bind(&token0_str)
        .bind(&token1_str)
        .bind(&dex_type_str)
        .bind(fee as i32)
        .bind(&liquidity_str)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// 同步指定代币对的池子
    pub async fn sync_token_pairs(&self, tokens: &[Address]) -> Result<usize> {
        let factories = if self.chain_id == 1 {
            get_eth_factories()
        } else if self.chain_id == 56 {
            get_bsc_factories()
        } else {
            return Ok(0);
        };

        let mut synced = 0;

        for factory in &factories {
            let factory_contract = IUniswapV2Factory::new(factory.address, self.provider.clone());

            // 获取所有代币对的池子
            for i in 0..tokens.len() {
                for j in (i + 1)..tokens.len() {
                    match factory_contract.get_pair(tokens[i], tokens[j]).call().await {
                        Ok(pair_address) => {
                            if pair_address != Address::zero() {
                                if let Err(e) = self.sync_specific_pair(pair_address, factory.dex_type).await {
                                    warn!("同步池子失败: {:?} - {}", pair_address, e);
                                } else {
                                    synced += 1;
                                }
                            }
                        }
                        Err(e) => {
                            warn!("获取池子地址失败: {}", e);
                        }
                    }
                }
            }
        }

        info!("同步了 {} 个代币对池子", synced);
        Ok(synced)
    }

    /// 同步指定池子地址
    async fn sync_specific_pair(&self, pair_address: Address, dex_type: DexType) -> Result<()> {
        let pair_contract = IUniswapV2Pair::new(pair_address, self.provider.clone());

        let token0 = pair_contract.token_0().call().await?;
        let token1 = pair_contract.token_1().call().await?;
        let (reserve0, reserve1, _) = pair_contract.get_reserves().call().await?;

        let liquidity = U256::from(reserve0) + U256::from(reserve1);

        self.save_pool(pair_address, token0, token1, dex_type, 3000, liquidity)
            .await?;

        Ok(())
    }

    /// 更新所有池子的流动性
    pub async fn update_liquidity(&self) -> Result<usize> {
        let pools = sqlx::query_as::<_, (String, String)>(
            "SELECT address, dex_type FROM pool_cache WHERE chain_id = ?",
        )
        .bind(self.chain_id as i64)
        .fetch_all(&self.db)
        .await?;

        let mut updated = 0;

        for (address_str, dex_type_str) in pools {
            let address: Address = address_str.parse()?;
            let dex_type = parse_dex_type(&dex_type_str);

            match self.update_pool_liquidity(address, dex_type).await {
                Ok(_) => updated += 1,
                Err(e) => {
                    warn!("更新池子流动性失败: {:?} - {}", address, e);
                }
            }

            // 避免请求过快
            if updated % 20 == 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            }
        }

        info!("更新了 {} 个池子的流动性", updated);
        Ok(updated)
    }

    /// 更新单个池子的流动性
    async fn update_pool_liquidity(&self, address: Address, _dex_type: DexType) -> Result<()> {
        let pair_contract = IUniswapV2Pair::new(address, self.provider.clone());
        let (reserve0, reserve1, _) = pair_contract.get_reserves().call().await?;

        let liquidity = U256::from(reserve0) + U256::from(reserve1);
        let address_str = format!("{:?}", address);
        let liquidity_str = liquidity.to_string();

        sqlx::query("UPDATE pool_cache SET liquidity = ?, updated_at = NOW() WHERE address = ?")
            .bind(&liquidity_str)
            .bind(&address_str)
            .execute(&self.db)
            .await?;

        Ok(())
    }
}

fn dex_type_to_string(dex_type: DexType) -> String {
    match dex_type {
        DexType::UniswapV2 => "uniswap_v2".to_string(),
        DexType::UniswapV3 => "uniswap_v3".to_string(),
        DexType::UniswapV4 => "uniswap_v4".to_string(),
        DexType::Curve => "curve".to_string(),
        DexType::PancakeSwapV2 => "pancakeswap_v2".to_string(),
        DexType::PancakeSwapV3 => "pancakeswap_v3".to_string(),
        DexType::SushiSwap => "sushiswap".to_string(),
        DexType::SushiSwapV2 => "sushiswap_v2".to_string(),
        DexType::SushiSwapV3 => "sushiswap_v3".to_string(),
    }
}

fn parse_dex_type(s: &str) -> DexType {
    match s.to_lowercase().as_str() {
        "uniswap_v2" | "uniswapv2" => DexType::UniswapV2,
        "uniswap_v3" | "uniswapv3" => DexType::UniswapV3,
        "uniswap_v4" | "uniswapv4" => DexType::UniswapV4,
        "curve" => DexType::Curve,
        "pancakeswap_v2" | "pancakeswapv2" => DexType::PancakeSwapV2,
        "pancakeswap_v3" | "pancakeswapv3" => DexType::PancakeSwapV3,
        "sushiswap" => DexType::SushiSwap,
        "sushiswap_v2" | "sushiswapv2" => DexType::SushiSwapV2,
        "sushiswap_v3" | "sushiswapv3" => DexType::SushiSwapV3,
        _ => DexType::UniswapV2,
    }
}
