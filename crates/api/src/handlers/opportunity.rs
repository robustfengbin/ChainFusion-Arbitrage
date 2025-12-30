use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::state::AppState;
use super::strategy::ApiResponse;

#[derive(Serialize)]
pub struct OpportunityResponse {
    pub id: String,
    pub path: serde_json::Value,
    pub expected_profit_usd: f64,
    pub gas_cost_usd: f64,
    pub net_profit_usd: f64,
    pub profit_percentage: f64,
    pub block_number: i64,
    pub executed: bool,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct OpportunityListQuery {
    pub executed: Option<bool>,
    pub min_profit: Option<f64>,
    pub limit: Option<i64>,
}

/// 获取套利机会列表
pub async fn list_opportunities(
    State(state): State<AppState>,
    Query(query): Query<OpportunityListQuery>,
) -> Json<ApiResponse<Vec<OpportunityResponse>>> {
    let limit = query.limit.unwrap_or(50);
    let min_profit = query.min_profit.unwrap_or(0.0);

    let sql = match query.executed {
        Some(true) => format!(
            "SELECT id, path, expected_profit_usd, gas_cost_usd, net_profit_usd, profit_percentage, block_number, executed, created_at
             FROM arbitrage_opportunities WHERE executed = TRUE AND net_profit_usd >= {} ORDER BY created_at DESC LIMIT {}",
            min_profit, limit
        ),
        Some(false) => format!(
            "SELECT id, path, expected_profit_usd, gas_cost_usd, net_profit_usd, profit_percentage, block_number, executed, created_at
             FROM arbitrage_opportunities WHERE executed = FALSE AND net_profit_usd >= {} ORDER BY net_profit_usd DESC LIMIT {}",
            min_profit, limit
        ),
        None => format!(
            "SELECT id, path, expected_profit_usd, gas_cost_usd, net_profit_usd, profit_percentage, block_number, executed, created_at
             FROM arbitrage_opportunities WHERE net_profit_usd >= {} ORDER BY created_at DESC LIMIT {}",
            min_profit, limit
        ),
    };

    match sqlx::query_as::<_, (String, serde_json::Value, f64, f64, f64, f64, i64, bool, chrono::DateTime<chrono::Utc>)>(&sql)
        .fetch_all(&state.db)
        .await
    {
        Ok(rows) => {
            let opportunities: Vec<OpportunityResponse> = rows
                .into_iter()
                .map(|(id, path, expected_profit_usd, gas_cost_usd, net_profit_usd, profit_percentage, block_number, executed, created_at)| {
                    OpportunityResponse {
                        id,
                        path,
                        expected_profit_usd,
                        gas_cost_usd,
                        net_profit_usd,
                        profit_percentage,
                        block_number,
                        executed,
                        created_at: created_at.to_rfc3339(),
                    }
                })
                .collect();
            Json(ApiResponse::success(opportunities))
        }
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}
