//! 历史 Swap 数据下载器

use anyhow::{Context, Result};
use ethers::{
    providers::{Http, Middleware, Provider},
    types::{BlockNumber, Filter, Log, H256, U256},
};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

use crate::config::BacktestConfig;
use crate::database::BacktestDatabase;
use crate::models::{PoolConfig, SwapEventData};

/// Uniswap V3 Swap 事件签名
const SWAP_EVENT_SIGNATURE: &str =
    "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67";

/// Swap 数据下载器
pub struct SwapDataDownloader {
    provider: Arc<Provider<Http>>,
    db: Arc<BacktestDatabase>,
    config: BacktestConfig,
    pools: Vec<PoolConfig>,
    pool_map: HashMap<String, PoolConfig>,
}

impl SwapDataDownloader {
    /// 创建下载器
    pub async fn new(
        config: BacktestConfig,
        db: Arc<BacktestDatabase>,
        pools: Vec<PoolConfig>,
    ) -> Result<Self> {
        let provider = Provider::<Http>::try_from(&config.eth_rpc_url)?;

        let pool_map: HashMap<String, PoolConfig> = pools
            .iter()
            .map(|p| (p.address.to_lowercase(), p.clone()))
            .collect();

        Ok(Self {
            provider: Arc::new(provider),
            db,
            config,
            pools,
            pool_map,
        })
    }

    /// 下载历史数据
    pub async fn download(&self) -> Result<u64> {
        info!("开始下载历史 Swap 数据...");

        // 获取当前区块
        let latest_block = self.provider.get_block_number().await?.as_u64();

        // 计算起始区块（3 个月前）
        // 以太坊每 12 秒一个区块，3 个月约 657,000 个区块
        let blocks_per_day = 24 * 60 * 60 / 12;
        let total_blocks = self.config.days * blocks_per_day;
        let start_block = latest_block.saturating_sub(total_blocks);

        // 检查是否有已下载的数据
        let resume_block = self
            .db
            .get_latest_downloaded_block(self.config.chain_id as i64)
            .await?;

        let actual_start = match resume_block {
            Some(b) if b > start_block => {
                info!("从区块 {} 继续下载（上次已下载到 {}）", b + 1, b);
                b + 1
            }
            _ => {
                info!("从区块 {} 开始下载", start_block);
                start_block
            }
        };

        info!(
            "下载范围: {} - {} (共 {} 个区块，采样间隔 {})",
            actual_start,
            latest_block,
            latest_block - actual_start,
            self.config.sample_interval
        );

        // 池子地址列表
        let pool_addresses: Vec<_> = self.pools.iter().map(|p| p.address_h160()).collect();

        if pool_addresses.is_empty() {
            warn!("没有配置任何池子");
            return Ok(0);
        }

        info!("监控 {} 个池子", pool_addresses.len());
        for pool in &self.pools {
            info!("  - {} ({}/{})", pool.address, pool.token0_symbol, pool.token1_symbol);
        }

        // 分批下载
        let batch_size = 2000u64; // 每批查询 2000 个区块
        let mut total_swaps = 0u64;
        let mut current_block = actual_start;

        // 创建进度条
        let total_iterations = (latest_block - actual_start) / batch_size + 1;
        let pb = ProgressBar::new(total_iterations);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
                .unwrap()
                .progress_chars("#>-"),
        );

        while current_block < latest_block {
            let end_block = std::cmp::min(current_block + batch_size, latest_block);

            // 获取 Swap 事件
            let filter = Filter::new()
                .address(pool_addresses.clone())
                .topic0(SWAP_EVENT_SIGNATURE.parse::<H256>()?)
                .from_block(BlockNumber::Number(current_block.into()))
                .to_block(BlockNumber::Number(end_block.into()));

            match self.provider.get_logs(&filter).await {
                Ok(logs) => {
                    if !logs.is_empty() {
                        let swaps = self.parse_logs(&logs).await?;
                        if !swaps.is_empty() {
                            let saved = self.db.save_swaps(&swaps).await?;
                            total_swaps += saved;
                        }
                    }
                }
                Err(e) => {
                    warn!("获取日志失败 (区块 {} - {}): {}", current_block, end_block, e);
                    // 等待一段时间后重试
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    continue;
                }
            }

            current_block = end_block + 1;
            pb.inc(1);

            // 避免 rate limit
            if pb.position() % 10 == 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }

        pb.finish_with_message("下载完成");
        info!("共下载 {} 条 Swap 记录", total_swaps);

        Ok(total_swaps)
    }

    /// 解析日志为 Swap 事件
    async fn parse_logs(&self, logs: &[Log]) -> Result<Vec<(i64, SwapEventData, f64)>> {
        let mut results = Vec::new();

        // 获取区块时间戳缓存
        let mut block_timestamps: HashMap<u64, u64> = HashMap::new();

        for log in logs {
            let pool_address = format!("{:?}", log.address).to_lowercase();

            // 检查是否是我们关注的池子
            let pool = match self.pool_map.get(&pool_address) {
                Some(p) => p,
                None => continue,
            };

            // 解析 Swap 事件数据
            // data: amount0 (int256) + amount1 (int256) + sqrtPriceX96 (uint160) + liquidity (uint128) + tick (int24)
            let data = &log.data.0;
            if data.len() < 160 {
                continue;
            }

            // 解析 amount0 (int256)
            let amount0 = parse_int256(&data[0..32]);
            // 解析 amount1 (int256)
            let amount1 = parse_int256(&data[32..64]);
            // 解析 sqrtPriceX96 (uint256)
            let sqrt_price_x96 = U256::from_big_endian(&data[64..96]);
            // 解析 liquidity (uint128)
            let liquidity = u128::from_be_bytes(data[112..128].try_into().unwrap_or([0u8; 16]));
            // 解析 tick (int24)
            let tick_bytes = &data[128..160];
            let tick = parse_int24(&tick_bytes[29..32]);

            // 获取区块时间戳
            let block_number = log.block_number.unwrap_or_default().as_u64();
            let block_timestamp = if let Some(&ts) = block_timestamps.get(&block_number) {
                ts
            } else {
                let ts = self
                    .get_block_timestamp(block_number)
                    .await
                    .unwrap_or(0);
                block_timestamps.insert(block_number, ts);
                ts
            };

            let swap = SwapEventData {
                block_number,
                block_timestamp,
                tx_hash: log.transaction_hash.unwrap_or_default(),
                log_index: log.log_index.unwrap_or_default().as_u32(),
                pool_address: log.address,
                amount0,
                amount1,
                sqrt_price_x96,
                tick,
                liquidity,
            };

            let usd_volume = swap.usd_volume(pool);

            results.push((self.config.chain_id as i64, swap, usd_volume));
        }

        Ok(results)
    }

    /// 获取区块时间戳
    async fn get_block_timestamp(&self, block_number: u64) -> Result<u64> {
        let block = self
            .provider
            .get_block(block_number)
            .await?
            .context("Block not found")?;

        Ok(block.timestamp.as_u64())
    }
}

/// 解析 int256
fn parse_int256(data: &[u8]) -> i128 {
    if data.len() != 32 {
        return 0;
    }

    // 检查符号位
    let is_negative = data[0] & 0x80 != 0;

    // 取低 128 位
    let value = u128::from_be_bytes(data[16..32].try_into().unwrap_or([0u8; 16]));

    if is_negative {
        // 负数：使用二补码
        -((!value).wrapping_add(1) as i128)
    } else {
        value as i128
    }
}

/// 解析 int24
fn parse_int24(data: &[u8]) -> i32 {
    if data.len() != 3 {
        return 0;
    }

    let value = ((data[0] as i32) << 16) | ((data[1] as i32) << 8) | (data[2] as i32);

    // 符号扩展
    if value & 0x800000 != 0 {
        value | !0xFFFFFF
    } else {
        value
    }
}
