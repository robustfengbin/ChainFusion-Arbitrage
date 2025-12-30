//! Bundle 构建器
//!
//! 用于构建 Flashbots Bundle（交易包）

use ethers::types::{Bytes, H256};
use super::types::BundleRequest;

/// Bundle 构建器
#[derive(Debug, Clone, Default)]
pub struct BundleBuilder {
    /// 签名后的交易列表
    txs: Vec<Bytes>,
    /// 目标区块号
    target_block: u64,
    /// 最小时间戳
    min_timestamp: Option<u64>,
    /// 最大时间戳
    max_timestamp: Option<u64>,
    /// 允许回滚的交易哈希
    reverting_tx_hashes: Vec<H256>,
}

impl BundleBuilder {
    /// 创建新的 Bundle 构建器
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置目标区块号
    pub fn target_block(mut self, block: u64) -> Self {
        self.target_block = block;
        self
    }

    /// 添加签名后的交易
    pub fn push_transaction(mut self, signed_tx: Bytes) -> Self {
        self.txs.push(signed_tx);
        self
    }

    /// 添加多笔签名后的交易
    pub fn push_transactions(mut self, signed_txs: Vec<Bytes>) -> Self {
        self.txs.extend(signed_txs);
        self
    }

    /// 设置最小时间戳（Bundle 只在此时间之后有效）
    pub fn min_timestamp(mut self, timestamp: u64) -> Self {
        self.min_timestamp = Some(timestamp);
        self
    }

    /// 设置最大时间戳（Bundle 只在此时间之前有效）
    pub fn max_timestamp(mut self, timestamp: u64) -> Self {
        self.max_timestamp = Some(timestamp);
        self
    }

    /// 添加允许回滚的交易哈希
    /// 如果这些交易失败，整个 Bundle 仍然有效
    pub fn allow_revert(mut self, tx_hash: H256) -> Self {
        self.reverting_tx_hashes.push(tx_hash);
        self
    }

    /// 构建 Bundle 请求
    pub fn build(self) -> BundleRequest {
        BundleRequest {
            txs: self.txs.iter().map(|tx| format!("0x{}", hex::encode(tx))).collect(),
            block_number: format!("0x{:x}", self.target_block),
            min_timestamp: self.min_timestamp,
            max_timestamp: self.max_timestamp,
            reverting_tx_hashes: self.reverting_tx_hashes
                .iter()
                .map(|h| format!("{:?}", h))
                .collect(),
        }
    }

    /// 获取交易数量
    pub fn tx_count(&self) -> usize {
        self.txs.len()
    }

    /// 检查 Bundle 是否为空
    pub fn is_empty(&self) -> bool {
        self.txs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bundle_builder() {
        let tx1 = Bytes::from(vec![0x01, 0x02, 0x03]);
        let tx2 = Bytes::from(vec![0x04, 0x05, 0x06]);

        let bundle = BundleBuilder::new()
            .target_block(12345678)
            .push_transaction(tx1)
            .push_transaction(tx2)
            .min_timestamp(1000)
            .max_timestamp(2000)
            .build();

        assert_eq!(bundle.txs.len(), 2);
        assert_eq!(bundle.block_number, "0xbc614e");
        assert_eq!(bundle.min_timestamp, Some(1000));
        assert_eq!(bundle.max_timestamp, Some(2000));
    }
}
