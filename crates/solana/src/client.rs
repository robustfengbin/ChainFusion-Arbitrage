//! Solana RPC 客户端模块

use anyhow::{Context, Result};
use solana_client::{
    nonblocking::rpc_client::RpcClient,
    rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
    rpc_filter::RpcFilterType,
};
use solana_sdk::{
    account::Account,
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::Signature,
};
use solana_account_decoder::UiAccountEncoding;
use std::sync::Arc;
use std::str::FromStr;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::config::SolanaConfig;
use crate::types::*;

/// Solana RPC 客户端
pub struct SolanaClient {
    /// RPC 客户端
    rpc: Arc<RpcClient>,
    /// 配置
    #[allow(dead_code)]
    config: SolanaConfig,
    /// 当前 slot 缓存
    current_slot: RwLock<u64>,
}

impl SolanaClient {
    /// 创建新的 Solana 客户端
    pub fn new(config: SolanaConfig) -> Result<Self> {
        let rpc = RpcClient::new_with_commitment(
            config.rpc_url.clone(),
            CommitmentConfig::confirmed(),
        );

        info!("[Solana] 创建 RPC 客户端: {}", config.rpc_url);

        Ok(Self {
            rpc: Arc::new(rpc),
            config,
            current_slot: RwLock::new(0),
        })
    }

    /// 获取 RPC 客户端引用
    pub fn rpc(&self) -> &RpcClient {
        &self.rpc
    }

    /// 获取当前 slot
    pub async fn get_slot(&self) -> Result<u64> {
        let slot = self.rpc.get_slot().await?;
        *self.current_slot.write().await = slot;
        Ok(slot)
    }

    /// 获取缓存的 slot
    pub async fn cached_slot(&self) -> u64 {
        *self.current_slot.read().await
    }

    /// 获取账户信息
    pub async fn get_account(&self, pubkey: &Pubkey) -> Result<Option<Account>> {
        let account = self.rpc
            .get_account_with_commitment(pubkey, CommitmentConfig::confirmed())
            .await?
            .value;
        Ok(account)
    }

    /// 获取多个账户信息
    pub async fn get_multiple_accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<Option<Account>>> {
        let accounts = self.rpc
            .get_multiple_accounts(pubkeys)
            .await?;
        Ok(accounts)
    }

    /// 获取 Token 账户余额
    pub async fn get_token_balance(&self, token_account: &Pubkey) -> Result<u64> {
        let balance = self.rpc
            .get_token_account_balance(token_account)
            .await?;

        let amount = balance.amount.parse::<u64>()
            .context("Failed to parse token balance")?;

        Ok(amount)
    }

    /// 获取 SOL 余额
    pub async fn get_sol_balance(&self, pubkey: &Pubkey) -> Result<u64> {
        let balance = self.rpc.get_balance(pubkey).await?;
        Ok(balance)
    }

    /// 获取最近的区块哈希
    pub async fn get_recent_blockhash(&self) -> Result<solana_sdk::hash::Hash> {
        let blockhash = self.rpc.get_latest_blockhash().await?;
        Ok(blockhash)
    }

    /// 获取程序账户 (用于获取所有池子)
    pub async fn get_program_accounts(
        &self,
        program_id: &Pubkey,
        filters: Option<Vec<RpcFilterType>>,
    ) -> Result<Vec<(Pubkey, Account)>> {
        let config = RpcProgramAccountsConfig {
            filters,
            account_config: RpcAccountInfoConfig {
                encoding: Some(UiAccountEncoding::Base64),
                commitment: Some(CommitmentConfig::confirmed()),
                ..Default::default()
            },
            ..Default::default()
        };

        let accounts = self.rpc
            .get_program_accounts_with_config(program_id, config)
            .await?;

        Ok(accounts)
    }

    /// 获取 Raydium CLMM 池子列表
    pub async fn get_raydium_clmm_pools(&self) -> Result<Vec<(Pubkey, Account)>> {
        let program_id = raydium::clmm_program();

        // CLMM Pool 账户大小过滤
        let filters = Some(vec![
            RpcFilterType::DataSize(1544), // CLMM Pool 账户大小
        ]);

        self.get_program_accounts(&program_id, filters).await
    }

    /// 获取 Raydium AMM V4 池子列表
    pub async fn get_raydium_amm_pools(&self) -> Result<Vec<(Pubkey, Account)>> {
        let program_id = raydium::amm_v4_program();

        let filters = Some(vec![
            RpcFilterType::DataSize(752), // AMM V4 Pool 账户大小
        ]);

        self.get_program_accounts(&program_id, filters).await
    }

    /// 检查连接状态
    pub async fn health_check(&self) -> Result<bool> {
        match self.rpc.get_health().await {
            Ok(_) => Ok(true),
            Err(e) => {
                warn!("[Solana] RPC 健康检查失败: {}", e);
                Ok(false)
            }
        }
    }

    /// 获取交易详情
    pub async fn get_transaction(&self, signature: &str) -> Result<Option<solana_transaction_status::EncodedConfirmedTransactionWithStatusMeta>> {
        let sig = Signature::from_str(signature)?;
        let tx = self.rpc
            .get_transaction(&sig, solana_transaction_status::UiTransactionEncoding::Json)
            .await
            .ok();
        Ok(tx)
    }

    /// 获取 Token Mint 信息
    pub async fn get_token_info(&self, mint: &Pubkey) -> Result<Option<SplTokenInfo>> {
        let account = self.get_account(mint).await?;

        if let Some(acc) = account {
            // 解析 SPL Token Mint 数据
            if acc.data.len() >= 82 {
                let decimals = acc.data[44];

                return Ok(Some(SplTokenInfo {
                    mint: *mint,
                    symbol: format!("TOKEN_{}", &mint.to_string()[0..6]),
                    decimals,
                    name: None,
                    is_stable: false,
                }));
            }
        }

        Ok(None)
    }
}

/// Jupiter API 客户端 (用于获取报价和路由)
pub struct JupiterClient {
    /// HTTP 客户端
    client: reqwest::Client,
    /// API URL
    api_url: String,
}

impl JupiterClient {
    pub fn new(api_url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_url: api_url.to_string(),
        }
    }

    /// 获取报价
    pub async fn get_quote(
        &self,
        input_mint: &Pubkey,
        output_mint: &Pubkey,
        amount: u64,
        slippage_bps: u16,
    ) -> Result<JupiterQuote> {
        let url = format!(
            "{}/quote?inputMint={}&outputMint={}&amount={}&slippageBps={}",
            self.api_url,
            input_mint,
            output_mint,
            amount,
            slippage_bps
        );

        let response = self.client
            .get(&url)
            .send()
            .await?
            .json::<JupiterQuote>()
            .await?;

        Ok(response)
    }

    /// 检查三角套利机会
    pub async fn check_triangle_opportunity(
        &self,
        token_a: &Pubkey,
        token_b: &Pubkey,
        token_c: &Pubkey,
        input_amount: u64,
        slippage_bps: u16,
    ) -> Result<Option<TriangleOpportunity>> {
        // A -> B
        let quote_ab = self.get_quote(token_a, token_b, input_amount, slippage_bps).await?;
        let amount_b = quote_ab.out_amount.parse::<u64>().unwrap_or(0);

        if amount_b == 0 {
            return Ok(None);
        }

        // B -> C
        let quote_bc = self.get_quote(token_b, token_c, amount_b, slippage_bps).await?;
        let amount_c = quote_bc.out_amount.parse::<u64>().unwrap_or(0);

        if amount_c == 0 {
            return Ok(None);
        }

        // C -> A
        let quote_ca = self.get_quote(token_c, token_a, amount_c, slippage_bps).await?;
        let final_amount = quote_ca.out_amount.parse::<u64>().unwrap_or(0);

        // 计算利润
        if final_amount > input_amount {
            let profit = final_amount - input_amount;
            let profit_percent = (profit as f64 / input_amount as f64) * 100.0;

            return Ok(Some(TriangleOpportunity {
                path: format!("{} -> {} -> {} -> {}",
                    &token_a.to_string()[0..8],
                    &token_b.to_string()[0..8],
                    &token_c.to_string()[0..8],
                    &token_a.to_string()[0..8]
                ),
                input_amount,
                output_amount: final_amount,
                profit,
                profit_percent,
                routes: vec![quote_ab, quote_bc, quote_ca],
            }));
        }

        Ok(None)
    }
}

/// Jupiter 报价响应
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JupiterQuote {
    pub input_mint: String,
    pub in_amount: String,
    pub output_mint: String,
    pub out_amount: String,
    pub other_amount_threshold: String,
    pub swap_mode: String,
    pub slippage_bps: u16,
    pub price_impact_pct: String,
    pub route_plan: Vec<JupiterRoutePlan>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JupiterRoutePlan {
    pub swap_info: JupiterSwapInfo,
    pub percent: u8,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JupiterSwapInfo {
    pub amm_key: String,
    pub label: Option<String>,
    pub input_mint: String,
    pub output_mint: String,
    pub in_amount: String,
    pub out_amount: String,
    pub fee_amount: String,
    pub fee_mint: String,
}

/// 三角套利机会
#[derive(Debug, Clone)]
pub struct TriangleOpportunity {
    pub path: String,
    pub input_amount: u64,
    pub output_amount: u64,
    pub profit: u64,
    pub profit_percent: f64,
    pub routes: Vec<JupiterQuote>,
}
