//! Jupiter 聚合器交互模块
//!
//! Jupiter 是 Solana 上最大的 DEX 聚合器，
//! 可以自动找到最优路径进行交易

use anyhow::Result;
use solana_sdk::pubkey::Pubkey;
use serde::{Deserialize, Serialize};
use tracing::{info, debug};

/// Jupiter API 基础 URL
pub const JUPITER_API_BASE: &str = "https://quote-api.jup.ag/v6";

/// Jupiter 报价请求
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteRequest {
    pub input_mint: String,
    pub output_mint: String,
    pub amount: String,
    pub slippage_bps: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub only_direct_routes: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_legacy_transaction: Option<bool>,
}

/// Jupiter 报价响应
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteResponse {
    pub input_mint: String,
    pub in_amount: String,
    pub output_mint: String,
    pub out_amount: String,
    pub other_amount_threshold: String,
    pub swap_mode: String,
    pub slippage_bps: u16,
    pub price_impact_pct: String,
    pub route_plan: Vec<RoutePlanStep>,
    #[serde(default)]
    pub context_slot: u64,
    #[serde(default)]
    pub time_taken: f64,
}

/// 路由计划步骤
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutePlanStep {
    pub swap_info: SwapInfo,
    pub percent: u8,
}

/// Swap 信息
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapInfo {
    pub amm_key: String,
    pub label: Option<String>,
    pub input_mint: String,
    pub output_mint: String,
    pub in_amount: String,
    pub out_amount: String,
    pub fee_amount: String,
    pub fee_mint: String,
}

/// Jupiter Swap 请求
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapRequest {
    pub quote_response: QuoteResponse,
    pub user_public_key: String,
    pub wrap_and_unwrap_sol: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_shared_accounts: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_unit_price_micro_lamports: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_legacy_transaction: Option<bool>,
}

/// Jupiter Swap 响应
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapResponse {
    pub swap_transaction: String,
    pub last_valid_block_height: u64,
    pub prioritization_fee_lamports: u64,
}

/// Jupiter 客户端
pub struct JupiterApi {
    client: reqwest::Client,
    base_url: String,
}

impl JupiterApi {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: JUPITER_API_BASE.to_string(),
        }
    }

    pub fn with_url(base_url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.to_string(),
        }
    }

    /// 获取报价
    pub async fn get_quote(
        &self,
        input_mint: &Pubkey,
        output_mint: &Pubkey,
        amount: u64,
        slippage_bps: u16,
    ) -> Result<QuoteResponse> {
        let url = format!(
            "{}/quote?inputMint={}&outputMint={}&amount={}&slippageBps={}",
            self.base_url,
            input_mint,
            output_mint,
            amount,
            slippage_bps
        );

        debug!("[Jupiter] 获取报价: {} -> {}, amount={}", input_mint, output_mint, amount);

        let response = self.client
            .get(&url)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Jupiter API error: {} - {}", status, text);
        }

        let quote = response.json::<QuoteResponse>().await?;

        debug!("[Jupiter] 报价结果: {} -> {}, out_amount={}",
            input_mint, output_mint, quote.out_amount);

        Ok(quote)
    }

    /// 获取 Swap 交易
    pub async fn get_swap_transaction(
        &self,
        quote: QuoteResponse,
        user_pubkey: &Pubkey,
        priority_fee: Option<u64>,
    ) -> Result<SwapResponse> {
        let url = format!("{}/swap", self.base_url);

        let request = SwapRequest {
            quote_response: quote,
            user_public_key: user_pubkey.to_string(),
            wrap_and_unwrap_sol: true,
            use_shared_accounts: Some(true),
            fee_account: None,
            compute_unit_price_micro_lamports: priority_fee,
            as_legacy_transaction: None,
        };

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Jupiter Swap API error: {} - {}", status, text);
        }

        let swap = response.json::<SwapResponse>().await?;
        Ok(swap)
    }

    /// 检查三角套利机会 (A -> B -> C -> A)
    pub async fn check_triangle_arbitrage(
        &self,
        token_a: &Pubkey,
        token_b: &Pubkey,
        token_c: &Pubkey,
        input_amount: u64,
        slippage_bps: u16,
    ) -> Result<Option<TriangleArbitrageResult>> {
        // 第一跳: A -> B
        let quote_ab = match self.get_quote(token_a, token_b, input_amount, slippage_bps).await {
            Ok(q) => q,
            Err(e) => {
                debug!("[Jupiter] A->B 报价失败: {}", e);
                return Ok(None);
            }
        };

        let amount_b = quote_ab.out_amount.parse::<u64>().unwrap_or(0);
        if amount_b == 0 {
            return Ok(None);
        }

        // 第二跳: B -> C
        let quote_bc = match self.get_quote(token_b, token_c, amount_b, slippage_bps).await {
            Ok(q) => q,
            Err(e) => {
                debug!("[Jupiter] B->C 报价失败: {}", e);
                return Ok(None);
            }
        };

        let amount_c = quote_bc.out_amount.parse::<u64>().unwrap_or(0);
        if amount_c == 0 {
            return Ok(None);
        }

        // 第三跳: C -> A
        let quote_ca = match self.get_quote(token_c, token_a, amount_c, slippage_bps).await {
            Ok(q) => q,
            Err(e) => {
                debug!("[Jupiter] C->A 报价失败: {}", e);
                return Ok(None);
            }
        };

        let final_amount = quote_ca.out_amount.parse::<u64>().unwrap_or(0);

        // 计算利润
        if final_amount > input_amount {
            let profit = final_amount - input_amount;
            let profit_percent = (profit as f64 / input_amount as f64) * 100.0;

            // 计算总价格影响
            let price_impact_ab: f64 = quote_ab.price_impact_pct.parse().unwrap_or(0.0);
            let price_impact_bc: f64 = quote_bc.price_impact_pct.parse().unwrap_or(0.0);
            let price_impact_ca: f64 = quote_ca.price_impact_pct.parse().unwrap_or(0.0);
            let total_price_impact = price_impact_ab + price_impact_bc + price_impact_ca;

            info!("[Jupiter] 发现三角套利机会!");
            info!("  路径: {} -> {} -> {} -> {}",
                &token_a.to_string()[0..8],
                &token_b.to_string()[0..8],
                &token_c.to_string()[0..8],
                &token_a.to_string()[0..8]
            );
            info!("  输入: {}, 输出: {}, 利润: {} ({:.4}%)",
                input_amount, final_amount, profit, profit_percent);

            return Ok(Some(TriangleArbitrageResult {
                token_a: *token_a,
                token_b: *token_b,
                token_c: *token_c,
                input_amount,
                amount_after_first: amount_b,
                amount_after_second: amount_c,
                final_amount,
                profit,
                profit_percent,
                total_price_impact,
                quotes: vec![quote_ab, quote_bc, quote_ca],
            }));
        }

        Ok(None)
    }
}

impl Default for JupiterApi {
    fn default() -> Self {
        Self::new()
    }
}

/// 三角套利结果
#[derive(Debug, Clone)]
pub struct TriangleArbitrageResult {
    pub token_a: Pubkey,
    pub token_b: Pubkey,
    pub token_c: Pubkey,
    pub input_amount: u64,
    pub amount_after_first: u64,
    pub amount_after_second: u64,
    pub final_amount: u64,
    pub profit: u64,
    pub profit_percent: f64,
    pub total_price_impact: f64,
    pub quotes: Vec<QuoteResponse>,
}
