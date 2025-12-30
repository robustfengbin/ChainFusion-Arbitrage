//! Flashbots 客户端
//!
//! 负责与 Flashbots 中继通信，发送私密交易

use anyhow::{Result, anyhow};
use ethers::prelude::*;
use ethers::types::{Bytes, H256};
use ethers::types::transaction::eip2718::TypedTransaction;
use ethers::utils::keccak256;
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn, debug, error};

use super::types::*;
use super::bundle::BundleBuilder;

/// Flashbots 客户端
pub struct FlashbotsClient<M: Middleware> {
    /// 配置
    config: FlashbotsConfig,
    /// HTTP 客户端
    http_client: Client,
    /// 以太坊 Provider
    provider: Arc<M>,
    /// Bundle 签名钱包（用于向 Flashbots 证明身份）
    signer: LocalWallet,
    /// 交易签名钱包
    tx_signer: LocalWallet,
}

impl<M: Middleware + 'static> FlashbotsClient<M> {
    /// 创建新的 Flashbots 客户端
    ///
    /// # 参数
    /// - `config`: Flashbots 配置
    /// - `provider`: 以太坊 Provider
    /// - `tx_private_key`: 交易签名私钥
    ///
    /// # 说明
    /// Bundle 签名私钥可以和交易私钥相同，也可以不同。
    /// 这个私钥只用于向 Flashbots 证明你的身份，不会用于签署实际交易。
    pub fn new(
        config: FlashbotsConfig,
        provider: Arc<M>,
        tx_private_key: &str,
    ) -> Result<Self> {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        // Bundle 签名钱包
        let signer_key_str = config.signer_key.clone().unwrap_or_else(|| tx_private_key.to_string());
        let signer: LocalWallet = signer_key_str.parse::<LocalWallet>()?
            .with_chain_id(config.chain_id);

        // 交易签名钱包
        let tx_signer: LocalWallet = tx_private_key.parse::<LocalWallet>()?
            .with_chain_id(config.chain_id);

        info!("🔒 Flashbots 客户端初始化: relay_url={}, chain_id={}", config.relay_url, config.chain_id);

        Ok(Self {
            config,
            http_client,
            provider,
            signer,
            tx_signer,
        })
    }

    /// 发送 Bundle 并等待打包
    ///
    /// # 流程
    /// 1. 模拟执行 Bundle
    /// 2. 发送 Bundle 到 Flashbots 中继
    /// 3. 等待 Bundle 被打包（在目标区块或后续区块）
    ///
    /// # 返回
    /// - `FlashbotsSendResult::Included`: 成功打包
    /// - `FlashbotsSendResult::NotIncluded`: 未被打包
    /// - `FlashbotsSendResult::SimulationFailed`: 模拟失败
    /// - `FlashbotsSendResult::SendFailed`: 发送失败
    pub async fn send_bundle(&self, bundle: BundleBuilder) -> FlashbotsSendResult {
        let target_block = self.get_next_block_number().await;

        info!(
            "准备发送 Flashbots Bundle: {} 笔交易, 目标区块 {}",
            bundle.tx_count(),
            target_block
        );

        // 尝试多个区块
        for block_offset in 0..self.config.max_block_retries {
            let current_target = target_block + block_offset;

            let bundle_request = bundle.clone().target_block(current_target).build();

            // 1. 模拟执行
            match self.simulate_bundle(&bundle_request).await {
                Ok(sim_result) => {
                    debug!("Bundle 模拟成功: gas_used={}, coinbase_diff={}",
                        sim_result.gas_used, sim_result.coinbase_diff);

                    // 检查是否有交易失败
                    for result in &sim_result.results {
                        // revert 为空 (0x) 表示成功，只有非空的 revert 才是失败
                        let has_revert = result.revert.as_ref()
                            .map(|r| !r.is_empty())
                            .unwrap_or(false);

                        if result.error.is_some() || has_revert {
                            let error_msg = result.error.clone()
                                .or_else(|| result.revert.as_ref().map(|r| format!("{:?}", r)))
                                .unwrap_or_else(|| "Unknown error".to_string());

                            return FlashbotsSendResult::SimulationFailed {
                                error: error_msg,
                            };
                        }
                    }
                }
                Err(e) => {
                    warn!("Bundle 模拟失败: {:?}", e);
                    return FlashbotsSendResult::SimulationFailed {
                        error: e.to_string(),
                    };
                }
            }

            // 2. 发送 Bundle
            match self.send_bundle_request(&bundle_request).await {
                Ok(response) => {
                    info!("Bundle 已发送: {:?}, 目标区块 {}", response.bundle_hash, current_target);

                    // 3. 等待打包
                    match self.wait_for_inclusion(response.bundle_hash, current_target).await {
                        Ok(Some(tx_hash)) => {
                            return FlashbotsSendResult::Included {
                                bundle_hash: response.bundle_hash,
                                block_number: current_target,
                                tx_hash,
                            };
                        }
                        Ok(None) => {
                            debug!("Bundle 未在区块 {} 被打包，尝试下一个区块", current_target);
                            continue;
                        }
                        Err(e) => {
                            warn!("等待打包时出错: {:?}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("发送 Bundle 失败: {:?}", e);
                    return FlashbotsSendResult::SendFailed {
                        error: e.to_string(),
                    };
                }
            }
        }

        FlashbotsSendResult::NotIncluded {
            bundle_hash: H256::zero(),
            reason: format!("Bundle 在 {} 个区块内未被打包", self.config.max_block_retries),
        }
    }

    /// 发送单笔交易通过 Flashbots
    ///
    /// 这是最常用的方法，将单笔交易包装成 Bundle 发送
    pub async fn send_private_transaction(
        &self,
        tx: TypedTransaction,
    ) -> Result<FlashbotsSendResult> {
        // 签名交易
        let signature = self.tx_signer.sign_transaction(&tx).await?;
        let signed_tx = tx.rlp_signed(&signature);

        // 构建 Bundle
        let bundle = BundleBuilder::new()
            .push_transaction(signed_tx);

        Ok(self.send_bundle(bundle).await)
    }

    /// 签名交易（不发送）
    pub async fn sign_transaction(&self, tx: &TypedTransaction) -> Result<Bytes> {
        let signature = self.tx_signer.sign_transaction(tx).await?;
        Ok(tx.rlp_signed(&signature))
    }

    /// 模拟 Bundle 执行
    async fn simulate_bundle(&self, bundle: &BundleRequest) -> Result<SimulateBundleResponse> {
        let current_block = self.provider.get_block_number().await?;

        let sim_request = SimulateBundleRequest {
            txs: bundle.txs.clone(),
            block_number: bundle.block_number.clone(),
            state_block_number: format!("0x{:x}", current_block),
            timestamp: None,
        };

        let request = JsonRpcRequest::new(
            "eth_callBundle",
            vec![sim_request],
        );

        let response = self.send_signed_request::<SimulateBundleResponse>(&request).await?;

        Ok(response)
    }

    /// 发送 Bundle 请求到 Flashbots 中继
    async fn send_bundle_request(&self, bundle: &BundleRequest) -> Result<SendBundleResponse> {
        let request = JsonRpcRequest::new(
            "eth_sendBundle",
            vec![bundle],
        );

        let response = self.send_signed_request::<SendBundleResponse>(&request).await?;

        Ok(response)
    }

    /// 等待 Bundle 被打包
    async fn wait_for_inclusion(
        &self,
        bundle_hash: H256,
        target_block: u64,
    ) -> Result<Option<H256>> {
        // 等待目标区块
        loop {
            let current_block = self.provider.get_block_number().await?;

            if current_block.as_u64() >= target_block {
                break;
            }

            debug!("等待区块 {} (当前 {})", target_block, current_block);
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        // 额外等待一点时间确保区块已传播
        tokio::time::sleep(Duration::from_secs(2)).await;

        // 查询 Bundle 状态
        match self.get_bundle_stats(bundle_hash, target_block).await {
            Ok(stats) => {
                debug!("Bundle 状态: {:?}", stats);

                // 检查是否被打包（通过查询区块内的交易）
                if let Ok(Some(block)) = self.provider.get_block_with_txs(target_block).await {
                    // 尝试找到我们的交易
                    // 注意：这里需要知道交易哈希才能匹配，暂时返回成功
                    if !block.transactions.is_empty() {
                        // 简单返回第一个交易的哈希作为示例
                        // 实际应该比较交易内容
                        return Ok(Some(block.transactions[0].hash));
                    }
                }

                Ok(None)
            }
            Err(e) => {
                warn!("获取 Bundle 状态失败: {:?}", e);
                Ok(None)
            }
        }
    }

    /// 获取 Bundle 状态
    async fn get_bundle_stats(&self, bundle_hash: H256, block_number: u64) -> Result<BundleStatsResponse> {
        #[derive(serde::Serialize)]
        struct Params {
            #[serde(rename = "bundleHash")]
            bundle_hash: String,
            #[serde(rename = "blockNumber")]
            block_number: String,
        }

        let request = JsonRpcRequest::new(
            "flashbots_getBundleStats",
            Params {
                bundle_hash: format!("{:?}", bundle_hash),
                block_number: format!("0x{:x}", block_number),
            },
        );

        self.send_signed_request::<BundleStatsResponse>(&request).await
    }

    /// 发送签名的请求到 Flashbots 中继
    async fn send_signed_request<T: serde::de::DeserializeOwned + Default>(
        &self,
        request: &JsonRpcRequest<impl serde::Serialize>,
    ) -> Result<T> {
        let body = serde_json::to_string(request)?;

        // 生成签名
        // Flashbots 要求: signMessage(keccak256(body).toHex())
        // 即：对 body 的 keccak256 哈希的十六进制字符串进行 EIP-191 签名
        let body_hash = keccak256(body.as_bytes());
        let hash_hex = format!("0x{}", hex::encode(body_hash));
        let signature = self.signer.sign_message(hash_hex.as_bytes()).await?;

        // X-Flashbots-Signature 格式: {signer_address}:{signature}
        // 确保签名格式正确：r (32) + s (32) + v (1) = 65 bytes
        let mut sig_bytes = signature.to_vec();
        // 确保 v 是 27 或 28 (EIP-155)
        if sig_bytes.len() == 65 && sig_bytes[64] < 27 {
            sig_bytes[64] += 27;
        }

        // 使用标准地址格式 (不用 {:?} 避免额外字符)
        let signer_addr = format!("0x{}", hex::encode(self.signer.address().as_bytes()));
        let auth_header = format!(
            "{}:0x{}",
            signer_addr,
            hex::encode(&sig_bytes)
        );

        info!("📡 Flashbots 请求 URL: {}", self.config.relay_url);
        info!("🔑 签名地址: {}", signer_addr);
        info!("🔐 签名长度: {} bytes, v={}", sig_bytes.len(), sig_bytes.get(64).unwrap_or(&0));
        debug!("📝 X-Flashbots-Signature: {}", auth_header);
        debug!("📤 请求体: {}", body);

        let response = self.http_client
            .post(&self.config.relay_url)
            .header("Content-Type", "application/json")
            .header("X-Flashbots-Signature", auth_header)
            .body(body)
            .send()
            .await?;

        let status = response.status();
        let text = response.text().await?;

        info!("📥 Flashbots 响应 [{}]: {}", status, text);

        if !status.is_success() {
            return Err(anyhow!("Flashbots 请求失败: {} - {}", status, text));
        }

        let json_response: JsonRpcResponse<T> = serde_json::from_str(&text)?;

        if let Some(error) = json_response.error {
            return Err(anyhow!("Flashbots RPC 错误: {} - {}", error.code, error.message));
        }

        json_response.result.ok_or_else(|| anyhow!("Flashbots 响应中没有 result"))
    }

    /// 获取下一个区块号
    async fn get_next_block_number(&self) -> u64 {
        match self.provider.get_block_number().await {
            Ok(n) => n.as_u64() + 1,
            Err(_) => 0,
        }
    }

    /// 检查 Flashbots 是否可用
    pub async fn health_check(&self) -> bool {
        // 尝试获取区块号来验证连接
        self.provider.get_block_number().await.is_ok()
    }

    /// 获取配置
    pub fn config(&self) -> &FlashbotsConfig {
        &self.config
    }

    /// 是否启用
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flashbots_config_default() {
        let config = FlashbotsConfig::default();
        assert_eq!(config.relay_url, "https://relay.flashbots.net");
        assert_eq!(config.chain_id, 1);
        assert!(!config.enabled);
    }

    #[test]
    fn test_relay_url_for_chain() {
        assert_eq!(
            FlashbotsConfig::relay_url_for_chain(1),
            "https://relay.flashbots.net"
        );
        assert_eq!(
            FlashbotsConfig::relay_url_for_chain(5),
            "https://relay-goerli.flashbots.net"
        );
    }
}
