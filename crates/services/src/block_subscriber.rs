use anyhow::Result;
use ethers::prelude::*;
use ethers::types::{Address, H256};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{info, error};
use ::utils::{record_ws_block, record_ws_swap};

/// 区块订阅配置
#[derive(Debug, Clone)]
pub struct BlockSubscriberConfig {
    pub ws_url: String,
    pub chain_id: u64,
    pub reconnect_delay_secs: u64,
    /// 监控的池子地址列表（只订阅这些池子的事件）
    pub monitored_pools: Vec<Address>,
}

/// 新区块事件
#[derive(Debug, Clone)]
pub struct NewBlockEvent {
    pub block_number: u64,
    pub block_hash: H256,
    pub timestamp: u64,
    pub base_fee: Option<U256>,
    pub gas_used: U256,
    pub gas_limit: U256,
}

/// Swap 事件 (Uniswap V3)
#[derive(Debug, Clone)]
pub struct SwapEvent {
    pub pool_address: Address,
    pub sender: Address,
    pub amount0_in: U256,
    pub amount1_in: U256,
    pub amount0_out: U256,
    pub amount1_out: U256,
    pub block_number: u64,
    pub tx_hash: H256,
    /// V3 价格状态: sqrtPriceX96 (交易后的价格)
    pub sqrt_price_x96: Option<U256>,
    /// V3 流动性
    pub liquidity: Option<u128>,
    /// V3 tick
    pub tick: Option<i32>,
}

/// 区块订阅器
pub struct BlockSubscriber {
    config: BlockSubscriberConfig,
    /// 新区块事件广播器
    block_tx: broadcast::Sender<NewBlockEvent>,
    /// Swap 事件广播器
    swap_tx: broadcast::Sender<SwapEvent>,
    /// 是否正在运行
    running: RwLock<bool>,
    /// 当前区块号
    current_block: RwLock<u64>,
}

impl BlockSubscriber {
    pub fn new(config: BlockSubscriberConfig) -> Self {
        let (block_tx, _) = broadcast::channel(100);
        let (swap_tx, _) = broadcast::channel(1000);

        Self {
            config,
            block_tx,
            swap_tx,
            running: RwLock::new(false),
            current_block: RwLock::new(0),
        }
    }

    /// 订阅新区块事件
    pub fn subscribe_blocks(&self) -> broadcast::Receiver<NewBlockEvent> {
        self.block_tx.subscribe()
    }

    /// 订阅 Swap 事件
    pub fn subscribe_swaps(&self) -> broadcast::Receiver<SwapEvent> {
        self.swap_tx.subscribe()
    }

    /// 获取当前区块号
    pub async fn current_block(&self) -> u64 {
        *self.current_block.read().await
    }

    /// 启动订阅
    pub async fn start(&self) -> Result<()> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(());
        }
        *running = true;
        drop(running);

        info!(
            "区块订阅器启动: chain_id={}, ws_url={}",
            self.config.chain_id, self.config.ws_url
        );

        loop {
            let running = self.running.read().await;
            if !*running {
                break;
            }
            drop(running);

            match self.connect_and_subscribe().await {
                Ok(_) => {
                    info!("WebSocket 连接正常关闭");
                }
                Err(e) => {
                    error!("WebSocket 连接错误: {}", e);
                }
            }

            // 检查是否应该重连
            let running = self.running.read().await;
            if !*running {
                break;
            }
            drop(running);

            info!("{}秒后重新连接...", self.config.reconnect_delay_secs);
            tokio::time::sleep(tokio::time::Duration::from_secs(
                self.config.reconnect_delay_secs,
            ))
            .await;
        }

        info!("区块订阅器停止");
        Ok(())
    }

    /// 连接并订阅
    async fn connect_and_subscribe(&self) -> Result<()> {
        // 连接 WebSocket
        let ws = Ws::connect(&self.config.ws_url).await?;
        let provider = Provider::new(ws);

        info!("WebSocket 已连接");

        // Uniswap V3 Swap 事件签名 (只监控 V3)
        let swap_v3_signature = H256::from_slice(&ethers::utils::keccak256(
            "Swap(address,address,int256,int256,uint160,uint128,int24)"
        ));

        // 订阅新区块
        let mut block_stream = provider.subscribe_blocks().await?;

        // 检查是否配置了监控池子
        let has_pools = !self.config.monitored_pools.is_empty();

        info!("区块订阅器配置 has_pools: {:?}", has_pools);

        info!(
            "监控池子数量: {}, 池子地址: {:?}",
            self.config.monitored_pools.len(),
            self.config.monitored_pools
        );

        // 订阅 V3 Swap 事件 - 只针对监控的池子地址
        // 如果没有配置池子，则不订阅任何 Swap 事件（只订阅区块）
        let mut v3_log_stream = if has_pools {
            // 只订阅我们监控的池子的 V3 Swap 事件
            let v3_filter = Filter::new()
                .topic0(swap_v3_signature)
                .address(self.config.monitored_pools.clone());
            let v3_stream = provider.subscribe_logs(&v3_filter).await?;

            info!(
                "已订阅 {} 个池子的 V3 Swap 事件 (只监控指定池子)",
                self.config.monitored_pools.len()
            );

            Some(v3_stream)
        } else {
            info!("未配置监控池子 - 只订阅区块事件，不订阅 Swap 事件");
            None
        };

        info!("开始接收事件 (纯 WebSocket, 无额外 RPC 调用)...");

        loop {
            let running = self.running.read().await;
            if !*running {
                break;
            }
            drop(running);

            tokio::select! {
                // 处理新区块
                Some(block) = block_stream.next() => {
                    let block_number = block.number.unwrap_or_default().as_u64();

                    // 更新当前区块号
                    {
                        let mut current = self.current_block.write().await;
                        *current = block_number;
                    }

                    // 构建区块事件
                    let event = NewBlockEvent {
                        block_number,
                        block_hash: block.hash.unwrap_or_default(),
                        timestamp: block.timestamp.as_u64(),
                        base_fee: block.base_fee_per_gas,
                        gas_used: block.gas_used,
                        gas_limit: block.gas_limit,
                    };

                    // 记录 WebSocket 区块事件统计
                    record_ws_block();

                    info!(
                        "📦 新区块: #{}, base_fee={:?} gwei",
                        block_number,
                        event.base_fee.map(|f| f / ethers::types::U256::from(1_000_000_000))
                    );

                    // 广播区块事件
                    let _ = self.block_tx.send(event);
                }

                // 处理 V3 Swap 事件 (直接从 WebSocket 收到)
                Some(log) = async {
                    match &mut v3_log_stream {
                        Some(stream) => stream.next().await,
                        None => std::future::pending().await,
                    }
                } => {
                    // 记录 WebSocket Swap 事件统计
                    record_ws_swap();

                    let block_number = log.block_number.map(|n| n.as_u64()).unwrap_or(0);
                    if let Some(event) = self.parse_swap_v3_log(&log, block_number) {
                        let _ = self.swap_tx.send(event);
                    }
                }

                else => {
                    // 所有流都结束了
                    break;
                }
            }
        }

        Ok(())
    }

    /// 解析 V3 Swap 日志
    /// V3 Swap: Swap(address indexed sender, address indexed recipient, int256 amount0, int256 amount1, uint160 sqrtPriceX96, uint128 liquidity, int24 tick)
    fn parse_swap_v3_log(&self, log: &Log, block_number: u64) -> Option<SwapEvent> {
        // 需要至少 3 个 topic: signature, sender, recipient
        if log.topics.len() < 3 {
            return None;
        }

        // 数据布局: int256 amount0 (32) + int256 amount1 (32) + uint160 sqrtPriceX96 (32) + uint128 liquidity (32) + int24 tick (32)
        // 至少需要 amount0 和 amount1 (64 bytes)
        if log.data.len() < 64 {
            return None;
        }

        let pool_address = log.address;
        let sender = Address::from_slice(&log.topics[1].as_bytes()[12..]);

        // V3 的 amount0 和 amount1 是有符号的 int256
        // 正数表示进入池子，负数表示离开池子
        let amount0_bytes: [u8; 32] = log.data[0..32].try_into().ok()?;
        let amount1_bytes: [u8; 32] = log.data[32..64].try_into().ok()?;

        let amount0_signed = i256_from_bytes(&amount0_bytes);
        let amount1_signed = i256_from_bytes(&amount1_bytes);

        // 转换为 V2 风格的 in/out
        // 正数 = token 进入池子 = amountIn
        // 负数 = token 离开池子 = amountOut
        let (amount0_in, amount0_out) = if amount0_signed >= 0 {
            (U256::from(amount0_signed as u128), U256::zero())
        } else {
            // 使用 saturating_abs 避免溢出
            let abs_val = (amount0_signed as i128).saturating_abs() as u128;
            (U256::zero(), U256::from(abs_val))
        };

        let (amount1_in, amount1_out) = if amount1_signed >= 0 {
            (U256::from(amount1_signed as u128), U256::zero())
        } else {
            // 使用 saturating_abs 避免溢出
            let abs_val = (amount1_signed as i128).saturating_abs() as u128;
            (U256::zero(), U256::from(abs_val))
        };

        // 解析 sqrtPriceX96 (bytes 64-96, uint160 存储在 32 字节中，右对齐)
        let sqrt_price_x96 = if log.data.len() >= 96 {
            Some(U256::from_big_endian(&log.data[64..96]))
        } else {
            None
        };

        // 解析 liquidity (bytes 96-128, uint128 存储在 32 字节中，右对齐)
        let liquidity = if log.data.len() >= 128 {
            // 取最后 16 字节 (128 bits)
            let mut liq_bytes = [0u8; 16];
            liq_bytes.copy_from_slice(&log.data[112..128]);
            Some(u128::from_be_bytes(liq_bytes))
        } else {
            None
        };

        // 解析 tick (bytes 128-160, int24 存储在 32 字节中，右对齐，有符号)
        let tick = if log.data.len() >= 160 {
            // tick 是 int24，存储在最后 3 字节，但需要考虑符号扩展
            let tick_bytes: [u8; 32] = log.data[128..160].try_into().ok()?;
            // 检查符号位 (第一个非零字节的最高位，或者看最后4字节)
            let tick_i32 = i32::from_be_bytes([tick_bytes[28], tick_bytes[29], tick_bytes[30], tick_bytes[31]]);
            // int24 范围是 -8388608 到 8388607，需要符号扩展
            let tick_i24 = if tick_bytes[28] & 0x80 != 0 {
                // 负数，需要符号扩展
                tick_i32 | (0xFF << 24) as i32
            } else {
                tick_i32
            };
            Some(tick_i24)
        } else {
            None
        };

        Some(SwapEvent {
            pool_address,
            sender,
            amount0_in,
            amount1_in,
            amount0_out,
            amount1_out,
            block_number,
            tx_hash: log.transaction_hash.unwrap_or_default(),
            sqrt_price_x96,
            liquidity,
            tick,
        })
    }

    /// 停止订阅
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
    }
}

/// 可共享的区块订阅器
pub type SharedBlockSubscriber = Arc<BlockSubscriber>;

/// 创建共享的区块订阅器
pub fn create_block_subscriber(config: BlockSubscriberConfig) -> SharedBlockSubscriber {
    Arc::new(BlockSubscriber::new(config))
}

/// 从 bytes 解析 int256（简化版，只取低 128 位）
fn i256_from_bytes(bytes: &[u8; 32]) -> i128 {
    // 检查符号位（第一个字节的最高位）
    let is_negative = bytes[0] & 0x80 != 0;

    if is_negative {
        // 负数：取反加一（二进制补码）
        // 简化处理：只取低 128 位
        let mut result_bytes = [0u8; 16];
        result_bytes.copy_from_slice(&bytes[16..32]);

        // 检查是否全部是 0xff（溢出到高位）
        let high_all_ff = bytes[0..16].iter().all(|&b| b == 0xff);

        if high_all_ff {
            // 安全地从低 128 位解析
            let abs_value = i128::from_be_bytes(result_bytes);
            abs_value
        } else {
            // 数值太大，返回最小值
            i128::MIN
        }
    } else {
        // 正数：直接解析低 128 位
        let mut result_bytes = [0u8; 16];
        result_bytes.copy_from_slice(&bytes[16..32]);
        i128::from_be_bytes(result_bytes)
    }
}
