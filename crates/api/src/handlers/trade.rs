use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::state::AppState;
use super::strategy::ApiResponse;

#[derive(Serialize)]
pub struct TradeResponse {
    pub id: i64,
    pub strategy_id: i64,
    pub tx_hash: String,
    pub arbitrage_type: String,
    pub profit_usd: f64,
    pub gas_cost_usd: f64,
    pub net_profit_usd: f64,
    pub status: String,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct TradeListQuery {
    pub strategy_id: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// 获取交易列表
pub async fn list_trades(
    State(state): State<AppState>,
    Query(query): Query<TradeListQuery>,
) -> Json<ApiResponse<Vec<TradeResponse>>> {
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);

    let sql = if let Some(strategy_id) = query.strategy_id {
        format!(
            "SELECT id, strategy_id, tx_hash, arbitrage_type, profit_usd, gas_cost_usd, net_profit_usd, status, created_at
             FROM trade_records WHERE strategy_id = {} ORDER BY id DESC LIMIT {} OFFSET {}",
            strategy_id, limit, offset
        )
    } else {
        format!(
            "SELECT id, strategy_id, tx_hash, arbitrage_type, profit_usd, gas_cost_usd, net_profit_usd, status, created_at
             FROM trade_records ORDER BY id DESC LIMIT {} OFFSET {}",
            limit, offset
        )
    };

    match sqlx::query_as::<_, (i64, i64, String, String, f64, f64, f64, String, chrono::DateTime<chrono::Utc>)>(&sql)
        .fetch_all(&state.db)
        .await
    {
        Ok(rows) => {
            let trades: Vec<TradeResponse> = rows
                .into_iter()
                .map(|(id, strategy_id, tx_hash, arbitrage_type, profit_usd, gas_cost_usd, net_profit_usd, status, created_at)| {
                    TradeResponse {
                        id,
                        strategy_id,
                        tx_hash,
                        arbitrage_type,
                        profit_usd,
                        gas_cost_usd,
                        net_profit_usd,
                        status,
                        created_at: created_at.to_rfc3339(),
                    }
                })
                .collect();
            Json(ApiResponse::success(trades))
        }
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

/// 获取交易详情
pub async fn get_trade(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<TradeResponse>> {
    match sqlx::query_as::<_, (i64, i64, String, String, f64, f64, f64, String, chrono::DateTime<chrono::Utc>)>(
        "SELECT id, strategy_id, tx_hash, arbitrage_type, profit_usd, gas_cost_usd, net_profit_usd, status, created_at
         FROM trade_records WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some((id, strategy_id, tx_hash, arbitrage_type, profit_usd, gas_cost_usd, net_profit_usd, status, created_at))) => {
            Json(ApiResponse::success(TradeResponse {
                id,
                strategy_id,
                tx_hash,
                arbitrage_type,
                profit_usd,
                gas_cost_usd,
                net_profit_usd,
                status,
                created_at: created_at.to_rfc3339(),
            }))
        }
        Ok(None) => Json(ApiResponse::error("交易不存在".to_string())),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}
