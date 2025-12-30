//! FlashArbitrage 合约 ABI 绑定

use ethers::prelude::*;

// 生成合约绑定 - 使用 JSON ABI 格式
abigen!(
    FlashArbitrageContract,
    r#"[
        {
            "inputs": [
                {
                    "components": [
                        {"name": "flashPool", "type": "address"},
                        {"name": "tokenA", "type": "address"},
                        {"name": "tokenB", "type": "address"},
                        {"name": "tokenC", "type": "address"},
                        {"name": "fee1", "type": "uint24"},
                        {"name": "fee2", "type": "uint24"},
                        {"name": "fee3", "type": "uint24"},
                        {"name": "amountIn", "type": "uint256"},
                        {"name": "minProfit", "type": "uint256"},
                        {"name": "profitToken", "type": "address"},
                        {"name": "profitConvertFee", "type": "uint24"}
                    ],
                    "name": "params",
                    "type": "tuple"
                }
            ],
            "name": "executeArbitrage",
            "outputs": [{"name": "profit", "type": "uint256"}],
            "stateMutability": "nonpayable",
            "type": "function"
        },
        {
            "inputs": [
                {"name": "token", "type": "address"},
                {"name": "to", "type": "address"},
                {"name": "amount", "type": "uint256"}
            ],
            "name": "withdrawProfit",
            "outputs": [],
            "stateMutability": "nonpayable",
            "type": "function"
        },
        {
            "inputs": [
                {"name": "token", "type": "address"},
                {"name": "to", "type": "address"}
            ],
            "name": "withdrawAllProfit",
            "outputs": [],
            "stateMutability": "nonpayable",
            "type": "function"
        },
        {
            "inputs": [{"name": "threshold", "type": "uint256"}],
            "name": "setMinProfitThreshold",
            "outputs": [],
            "stateMutability": "nonpayable",
            "type": "function"
        },
        {
            "inputs": [{"name": "token", "type": "address"}],
            "name": "emergencyWithdraw",
            "outputs": [],
            "stateMutability": "nonpayable",
            "type": "function"
        },
        {
            "inputs": [],
            "name": "emergencyWithdrawEth",
            "outputs": [],
            "stateMutability": "nonpayable",
            "type": "function"
        },
        {
            "inputs": [],
            "name": "owner",
            "outputs": [{"name": "", "type": "address"}],
            "stateMutability": "view",
            "type": "function"
        },
        {
            "inputs": [],
            "name": "minProfitThreshold",
            "outputs": [{"name": "", "type": "uint256"}],
            "stateMutability": "view",
            "type": "function"
        },
        {
            "inputs": [],
            "name": "SWAP_ROUTER",
            "outputs": [{"name": "", "type": "address"}],
            "stateMutability": "view",
            "type": "function"
        },
        {
            "anonymous": false,
            "inputs": [
                {"indexed": true, "name": "tokenA", "type": "address"},
                {"indexed": true, "name": "tokenB", "type": "address"},
                {"indexed": true, "name": "tokenC", "type": "address"},
                {"indexed": false, "name": "amountIn", "type": "uint256"},
                {"indexed": false, "name": "amountOut", "type": "uint256"},
                {"indexed": false, "name": "profit", "type": "uint256"}
            ],
            "name": "ArbitrageExecuted",
            "type": "event"
        },
        {
            "anonymous": false,
            "inputs": [
                {"indexed": true, "name": "fromToken", "type": "address"},
                {"indexed": true, "name": "toToken", "type": "address"},
                {"indexed": false, "name": "amountIn", "type": "uint256"},
                {"indexed": false, "name": "amountOut", "type": "uint256"}
            ],
            "name": "ProfitConverted",
            "type": "event"
        },
        {
            "anonymous": false,
            "inputs": [
                {"indexed": true, "name": "tokenA", "type": "address"},
                {"indexed": false, "name": "amountIn", "type": "uint256"},
                {"indexed": false, "name": "reason", "type": "string"}
            ],
            "name": "ArbitrageFailed",
            "type": "event"
        },
        {
            "anonymous": false,
            "inputs": [
                {"indexed": true, "name": "token", "type": "address"},
                {"indexed": true, "name": "to", "type": "address"},
                {"indexed": false, "name": "amount", "type": "uint256"}
            ],
            "name": "ProfitWithdrawn",
            "type": "event"
        },
        {
            "anonymous": false,
            "inputs": [
                {"indexed": true, "name": "step", "type": "uint8"},
                {"indexed": false, "name": "tokenIn", "type": "address"},
                {"indexed": false, "name": "tokenOut", "type": "address"},
                {"indexed": false, "name": "amountIn", "type": "uint256"},
                {"indexed": false, "name": "amountOut", "type": "uint256"}
            ],
            "name": "SwapStepExecuted",
            "type": "event"
        },
        {
            "anonymous": false,
            "inputs": [
                {"indexed": false, "name": "inputAmount", "type": "uint256"},
                {"indexed": false, "name": "step1Out", "type": "uint256"},
                {"indexed": false, "name": "step2Out", "type": "uint256"},
                {"indexed": false, "name": "step3Out", "type": "uint256"},
                {"indexed": false, "name": "flashFee", "type": "uint256"},
                {"indexed": false, "name": "profitOrLoss", "type": "int256"}
            ],
            "name": "ArbitrageResult",
            "type": "event"
        },
        {
            "inputs": [
                {"name": "reason", "type": "string"},
                {"name": "tokenA", "type": "address"},
                {"name": "tokenB", "type": "address"},
                {"name": "tokenC", "type": "address"},
                {"name": "inputAmount", "type": "uint256"},
                {"name": "step1Out", "type": "uint256"},
                {"name": "step2Out", "type": "uint256"},
                {"name": "step3Out", "type": "uint256"},
                {"name": "amountOwed", "type": "uint256"},
                {"name": "profitOrLoss", "type": "int256"}
            ],
            "name": "ArbitrageFailed_Detailed",
            "type": "error"
        },
        {
            "inputs": [
                {"name": "actualProfit", "type": "uint256"},
                {"name": "minRequired", "type": "uint256"},
                {"name": "inputAmount", "type": "uint256"},
                {"name": "outputAmount", "type": "uint256"}
            ],
            "name": "ProfitBelowMinimum",
            "type": "error"
        }
    ]"#
);

/// 套利参数 - 用于调用 executeArbitrage 函数
/// 这个结构体与合约中的 ArbitrageParams 结构体一一对应
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArbitrageContractParams {
    pub flash_pool: Address,
    pub token_a: Address,
    pub token_b: Address,
    pub token_c: Address,
    pub fee1: u32,
    pub fee2: u32,
    pub fee3: u32,
    pub amount_in: U256,
    pub min_profit: U256,
    /// 利润结算代币 (Address::zero() 表示不转换)
    pub profit_token: Address,
    /// 利润转换池费率 (tokenA -> profitToken)
    pub profit_convert_fee: u32,
}

/// abigen 生成的 executeArbitrage 函数期望的参数类型
pub type ExecuteArbitrageParams = (Address, Address, Address, Address, u32, u32, u32, U256, U256, Address, u32);

impl ArbitrageContractParams {
    /// 转换为 abigen 生成的元组格式
    pub fn into_tuple(self) -> ExecuteArbitrageParams {
        (
            self.flash_pool,
            self.token_a,
            self.token_b,
            self.token_c,
            self.fee1,
            self.fee2,
            self.fee3,
            self.amount_in,
            self.min_profit,
            self.profit_token,
            self.profit_convert_fee,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contract_binding() {
        // 验证合约绑定正确生成
        let _: Address = Address::zero();
    }
}
