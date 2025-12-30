//! Flashbots 类型定义

use ethers::types::{H256, U256, Bytes, Address};
use serde::{Deserialize, Serialize};

/// Flashbots 配置
#[derive(Debug, Clone)]
pub struct FlashbotsConfig {
    /// Flashbots 中继 URL
    pub relay_url: String,
    /// 链 ID
    pub chain_id: u64,
    /// 是否启用 Flashbots
    pub enabled: bool,
    /// 最大重试区块数（如果当前区块没打包，尝试下几个区块）
    pub max_block_retries: u64,
    /// Bundle 签名私钥（用于向 Flashbots 证明身份，可以和交易私钥不同）
    pub signer_key: Option<String>,
}

impl Default for FlashbotsConfig {
    fn default() -> Self {
        Self {
            // 以太坊主网 Flashbots 中继
            relay_url: "https://relay.flashbots.net".to_string(),
            chain_id: 1,
            enabled: false,
            max_block_retries: 3,
            signer_key: None,
        }
    }
}

impl FlashbotsConfig {
    /// 获取对应链的 Flashbots 中继 URL
    pub fn relay_url_for_chain(chain_id: u64) -> &'static str {
        match chain_id {
            1 => "https://relay.flashbots.net",           // 以太坊主网
            5 => "https://relay-goerli.flashbots.net",    // Goerli 测试网
            11155111 => "https://relay-sepolia.flashbots.net", // Sepolia 测试网
            _ => "https://relay.flashbots.net",           // 默认主网
        }
    }
}

/// Bundle 请求参数
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleRequest {
    /// 签名后的交易列表（十六进制字符串）
    pub txs: Vec<String>,
    /// 目标区块号（十六进制）
    pub block_number: String,
    /// 最小时间戳（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_timestamp: Option<u64>,
    /// 最大时间戳（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_timestamp: Option<u64>,
    /// 回滚交易哈希列表（如果这些交易失败，整个 bundle 回滚）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reverting_tx_hashes: Vec<String>,
}

/// Bundle 模拟请求
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulateBundleRequest {
    /// 签名后的交易列表
    pub txs: Vec<String>,
    /// 目标区块号
    pub block_number: String,
    /// 用于模拟的状态区块号
    pub state_block_number: String,
    /// 模拟时间戳（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
}

/// Bundle 发送响应
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendBundleResponse {
    /// Bundle 哈希
    #[serde(default)]
    pub bundle_hash: H256,
}

/// Bundle 模拟响应
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulateBundleResponse {
    /// 模拟结果列表
    #[serde(default)]
    pub results: Vec<SimulationResult>,
    /// coinbase 收益差（验证者收益）
    #[serde(default)]
    pub coinbase_diff: U256,
    /// gas 价格
    #[serde(default)]
    pub gas_price: U256,
    /// 总 gas 使用
    #[serde(default)]
    pub gas_used: u64,
    /// 状态区块号
    #[serde(default)]
    pub state_block_number: u64,
    /// 总 gas 费
    #[serde(default)]
    pub total_gas_fees: U256,
}

/// 单笔交易模拟结果
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulationResult {
    /// 交易哈希
    #[serde(default)]
    pub tx_hash: H256,
    /// gas 使用量
    #[serde(default)]
    pub gas_used: u64,
    /// gas 价格
    #[serde(default)]
    pub gas_price: U256,
    /// gas 费
    #[serde(default)]
    pub gas_fees: U256,
    /// 发送者
    #[serde(default)]
    pub from_address: Address,
    /// 接收者
    #[serde(default)]
    pub to_address: Option<Address>,
    /// coinbase 差值
    #[serde(default)]
    pub coinbase_diff: U256,
    /// ETH 发送金额
    #[serde(default)]
    pub eth_sent_to_coinbase: U256,
    /// 调用数据
    #[serde(default)]
    pub coinbase_transfer: Option<U256>,
    /// 错误信息（如果有）
    #[serde(default)]
    pub error: Option<String>,
    /// 回滚原因（如果有）
    #[serde(default)]
    pub revert: Option<Bytes>,
    /// 返回值
    #[serde(default)]
    pub value: Option<Bytes>,
}

/// Bundle 状态查询响应
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleStatsResponse {
    /// 是否已经被模拟
    #[serde(default)]
    pub is_simulated: bool,
    /// 是否已经被提交给验证者
    #[serde(default)]
    pub is_sent_to_miners: bool,
    /// 是否高优先级
    #[serde(default)]
    pub is_high_priority: Option<bool>,
    /// 第一次模拟时间
    #[serde(default)]
    pub simulated_at: Option<String>,
    /// 提交给验证者的时间
    #[serde(default)]
    pub submitted_at: Option<String>,
    /// 考虑的区块号
    #[serde(default)]
    pub considered_by_builders_at: Option<Vec<ConsideredBlock>>,
}

/// 被考虑的区块信息
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsideredBlock {
    pub pubkey: String,
    pub timestamp: String,
}

/// Flashbots 发送结果
#[derive(Debug)]
pub enum FlashbotsSendResult {
    /// 成功打包
    Included {
        bundle_hash: H256,
        block_number: u64,
        tx_hash: H256,
    },
    /// 未被打包（但没有错误，可以重试）
    NotIncluded {
        bundle_hash: H256,
        reason: String,
    },
    /// 模拟失败
    SimulationFailed {
        error: String,
    },
    /// 发送失败
    SendFailed {
        error: String,
    },
}

/// JSON-RPC 请求
#[derive(Debug, Serialize)]
pub struct JsonRpcRequest<T: Serialize> {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: &'static str,
    pub params: T,
}

impl<T: Serialize> JsonRpcRequest<T> {
    pub fn new(method: &'static str, params: T) -> Self {
        Self {
            jsonrpc: "2.0",
            id: 1,
            method,
            params,
        }
    }
}

/// JSON-RPC 响应
#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse<T> {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(default)]
    pub result: Option<T>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 错误
#[derive(Debug, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}
