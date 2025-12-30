use ethers::types::{Address, Bytes, H256, U256};
use serde::{Deserialize, Serialize};

/// 交易构建参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionParams {
    pub to: Address,
    pub value: U256,
    pub data: Bytes,
    pub gas_limit: U256,
    pub gas_price: Option<U256>,
    pub max_fee_per_gas: Option<U256>,
    pub max_priority_fee_per_gas: Option<U256>,
    pub nonce: Option<U256>,
    pub chain_id: u64,
}

/// 闪电贷请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashLoanRequest {
    pub token: Address,
    pub amount: U256,
    pub provider: FlashLoanProvider,
    pub callback_data: Bytes,
}

/// 闪电贷提供商
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlashLoanProvider {
    UniswapV3,
    UniswapV4,
    Aave,
    Balancer,
}

impl FlashLoanProvider {
    pub fn fee_bps(&self) -> u32 {
        match self {
            FlashLoanProvider::UniswapV3 => 0,    // Uniswap V3 闪电贷无费用
            FlashLoanProvider::UniswapV4 => 0,    // Uniswap V4 Flash Accounting 无费用
            FlashLoanProvider::Aave => 9,         // Aave 0.09% fee
            FlashLoanProvider::Balancer => 0,     // Balancer 无费用
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            FlashLoanProvider::UniswapV3 => "Uniswap V3",
            FlashLoanProvider::UniswapV4 => "Uniswap V4",
            FlashLoanProvider::Aave => "Aave",
            FlashLoanProvider::Balancer => "Balancer",
        }
    }
}

/// Swap 指令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapInstruction {
    pub pool_address: Address,
    pub token_in: Address,
    pub token_out: Address,
    pub amount_in: U256,
    pub min_amount_out: U256,
    pub deadline: U256,
}

/// 批量 Swap 指令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchSwapInstruction {
    pub swaps: Vec<SwapInstruction>,
    pub flash_loan: Option<FlashLoanRequest>,
}

/// 交易收据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionReceipt {
    pub tx_hash: H256,
    pub block_number: u64,
    pub block_hash: H256,
    pub gas_used: U256,
    pub effective_gas_price: U256,
    pub status: bool,
    pub logs: Vec<TransactionLog>,
}

/// 交易日志
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionLog {
    pub address: Address,
    pub topics: Vec<H256>,
    pub data: Bytes,
    pub log_index: u64,
}
