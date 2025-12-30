//! Solana WebSocket 订阅模块
//!
//! 监控 DEX swap 事件，触发套利扫描

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, error, debug};

use crate::types::raydium;

/// WebSocket 订阅器
pub struct SolanaWsSubscriber {
    /// WebSocket URL
    ws_url: String,
    /// 监控的代币 mint 地址
    target_tokens: RwLock<Vec<Pubkey>>,
    /// 事件发送器
    event_tx: broadcast::Sender<SwapEvent>,
    /// 是否运行中
    running: RwLock<bool>,
}

/// Swap 事件
#[derive(Debug, Clone)]
pub struct SwapEvent {
    /// 签名
    pub signature: String,
    /// Slot
    pub slot: u64,
    /// 涉及的代币 (如果能解析)
    pub tokens: Vec<String>,
    /// 原始日志
    pub logs: Vec<String>,
}

/// RPC 响应
#[derive(Debug, Deserialize)]
struct RpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<RpcParams>,
    #[serde(default)]
    id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RpcParams {
    result: RpcResult,
    #[allow(dead_code)]
    subscription: u64,
}

#[derive(Debug, Deserialize)]
struct RpcResult {
    context: RpcContext,
    value: RpcValue,
}

#[derive(Debug, Deserialize)]
struct RpcContext {
    slot: u64,
}

#[derive(Debug, Deserialize)]
struct RpcValue {
    signature: String,
    #[serde(default)]
    err: Option<Value>,
    logs: Option<Vec<String>>,
}

impl SolanaWsSubscriber {
    /// 创建新的订阅器
    pub fn new(ws_url: &str) -> Self {
        let (event_tx, _) = broadcast::channel(1000);

        Self {
            ws_url: ws_url.to_string(),
            target_tokens: RwLock::new(Vec::new()),
            event_tx,
            running: RwLock::new(false),
        }
    }

    /// 添加监控的代币
    pub async fn add_target_token(&self, mint: &str) -> Result<()> {
        let pubkey = Pubkey::from_str(mint)?;
        self.target_tokens.write().await.push(pubkey);
        info!("[Solana WS] 添加监控代币: {}", mint);
        Ok(())
    }

    /// 订阅 swap 事件
    pub fn subscribe_swaps(&self) -> broadcast::Receiver<SwapEvent> {
        self.event_tx.subscribe()
    }

    /// 启动 WebSocket 订阅
    pub async fn start(&self) -> Result<()> {
        *self.running.write().await = true;

        info!("[Solana WS] 连接到 {}", self.ws_url);

        loop {
            if !*self.running.read().await {
                break;
            }

            match self.run_connection().await {
                Ok(_) => {
                    info!("[Solana WS] 连接正常关闭");
                }
                Err(e) => {
                    error!("[Solana WS] 连接错误: {}", e);
                }
            }

            if *self.running.read().await {
                info!("[Solana WS] 5秒后重连...");
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }

        Ok(())
    }

    /// 运行单次连接
    async fn run_connection(&self) -> Result<()> {
        let (ws_stream, _) = connect_async(&self.ws_url).await?;
        let (mut write, mut read) = ws_stream.split();

        info!("[Solana WS] 连接成功");

        // 订阅 Raydium CLMM 程序的日志
        let subscribe_raydium_clmm = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "logsSubscribe",
            "params": [
                {
                    "mentions": [raydium::CLMM_PROGRAM]
                },
                {
                    "commitment": "confirmed"
                }
            ]
        });

        // 订阅 Raydium AMM V4 程序的日志
        let subscribe_raydium_amm = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "logsSubscribe",
            "params": [
                {
                    "mentions": [raydium::AMM_V4_PROGRAM]
                },
                {
                    "commitment": "confirmed"
                }
            ]
        });

        // 发送订阅请求
        write.send(Message::Text(subscribe_raydium_clmm.to_string())).await?;
        write.send(Message::Text(subscribe_raydium_amm.to_string())).await?;

        info!("[Solana WS] 已订阅 Raydium CLMM 和 AMM V4 日志");

        // 处理消息
        while let Some(msg) = read.next().await {
            if !*self.running.read().await {
                break;
            }

            match msg {
                Ok(Message::Text(text)) => {
                    self.handle_message(&text).await;
                }
                Ok(Message::Ping(data)) => {
                    let _ = write.send(Message::Pong(data)).await;
                }
                Ok(Message::Close(_)) => {
                    info!("[Solana WS] 收到关闭帧");
                    break;
                }
                Err(e) => {
                    error!("[Solana WS] 接收消息错误: {}", e);
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// 处理 WebSocket 消息
    async fn handle_message(&self, text: &str) {
        let response: RpcResponse = match serde_json::from_str(text) {
            Ok(r) => r,
            Err(e) => {
                debug!("[Solana WS] 解析消息失败: {}", e);
                return;
            }
        };

        // 处理订阅确认
        if let Some(result) = &response.result {
            if let Some(id) = response.id {
                info!("[Solana WS] 订阅 #{} 确认: {:?}", id, result);
            }
            return;
        }

        // 处理日志通知
        if response.method.as_deref() == Some("logsNotification") {
            if let Some(params) = response.params {
                let slot = params.result.context.slot;
                let signature = params.result.value.signature;
                let logs = params.result.value.logs.unwrap_or_default();

                // 检查是否是失败的交易
                if params.result.value.err.is_some() {
                    return;
                }

                // 检查是否是 swap 相关的日志
                let is_swap = logs.iter().any(|log| {
                    log.contains("Swap") ||
                    log.contains("swap") ||
                    log.contains("SwapBaseIn") ||
                    log.contains("SwapBaseOut")
                });

                if is_swap {
                    debug!("[Solana WS] 检测到 Swap 事件: slot={}, sig={}", slot, &signature[..16]);

                    // 提取涉及的代币地址（从日志中解析）
                    let tokens: Vec<String> = logs.iter()
                        .filter(|log| log.len() >= 44 && !log.contains("Program"))
                        .filter_map(|log| {
                            // 尝试提取 base58 地址
                            log.split_whitespace()
                                .find(|s| s.len() >= 32 && s.len() <= 44)
                                .map(|s| s.to_string())
                        })
                        .collect();

                    let event = SwapEvent {
                        signature: signature.clone(),
                        slot,
                        tokens,
                        logs: logs.clone(),
                    };

                    // 发送事件
                    if let Err(e) = self.event_tx.send(event) {
                        debug!("[Solana WS] 发送事件失败 (无接收者): {}", e);
                    }
                }
            }
        }
    }

    /// 停止订阅
    pub async fn stop(&self) {
        *self.running.write().await = false;
        info!("[Solana WS] 停止订阅");
    }
}

/// 简化的事件驱动扫描器
pub struct EventDrivenSolanaScanner {
    ws_subscriber: Arc<SolanaWsSubscriber>,
    target_token: String,
}

impl EventDrivenSolanaScanner {
    pub fn new(ws_url: &str, target_token: &str) -> Self {
        Self {
            ws_subscriber: Arc::new(SolanaWsSubscriber::new(ws_url)),
            target_token: target_token.to_string(),
        }
    }

    /// 启动事件驱动扫描
    pub async fn start(&self) -> Result<()> {
        // 添加目标代币
        self.ws_subscriber.add_target_token(&self.target_token).await?;

        // 订阅 swap 事件
        let mut swap_rx = self.ws_subscriber.subscribe_swaps();

        // 启动 WebSocket 订阅器
        let ws = self.ws_subscriber.clone();
        let ws_handle = tokio::spawn(async move {
            if let Err(e) = ws.start().await {
                error!("[Solana] WebSocket 订阅错误: {}", e);
            }
        });

        info!("[Solana] 事件驱动扫描器启动，监控代币: {}", self.target_token);

        // 处理 swap 事件
        while let Ok(event) = swap_rx.recv().await {
            self.handle_swap_event(event).await;
        }

        ws_handle.await?;
        Ok(())
    }

    /// 处理 swap 事件
    async fn handle_swap_event(&self, event: SwapEvent) {
        // 检查是否涉及目标代币
        let involves_target = event.tokens.iter()
            .any(|t| t == &self.target_token);

        if involves_target {
            info!("[Solana] 🎯 检测到目标代币 swap!");
            info!("  Slot: {}", event.slot);
            info!("  签名: {}", &event.signature[..32]);

            // TODO: 触发 Jupiter 套利检查
            // 这里可以调用 JupiterApi 检查三角套利机会
        }
    }
}
