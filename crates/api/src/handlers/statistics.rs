use axum::{
    extract::{Path, State},
    Json,
};
use serde::Serialize;

use crate::state::AppState;
use super::strategy::ApiResponse;

#[derive(Serialize)]
pub struct StatisticsResponse {
    pub total_trades: i64,
    pub successful_trades: i64,
    pub failed_trades: i64,
    pub total_profit_usd: f64,
    pub total_gas_cost_usd: f64,
    pub net_profit_usd: f64,
    pub win_rate: f64,
    pub avg_profit_per_trade: f64,
}

#[derive(Serialize)]
pub struct OverallStatistics {
    pub total_strategies: i64,
    pub running_strategies: i64,
    pub total_trades: i64,
    pub total_profit_usd: f64,
    pub today_trades: i64,
    pub today_profit_usd: f64,
}

/// 获取总体统计
pub async fn get_statistics(
    State(state): State<AppState>,
) -> Json<ApiResponse<OverallStatistics>> {
    // 获取策略数量
    let total_strategies: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM arbitrage_strategies"
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let running_strategies: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM arbitrage_strategies WHERE status = 'running'"
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    // 获取交易统计
    let total_trades: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM trade_records"
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let total_profit_usd: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(net_profit_usd), 0) FROM trade_records WHERE status = 'confirmed'"
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0.0);

    // 今日统计
    let today_trades: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM trade_records WHERE DATE(created_at) = CURDATE()"
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let today_profit_usd: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(net_profit_usd), 0) FROM trade_records WHERE DATE(created_at) = CURDATE() AND status = 'confirmed'"
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0.0);

    Json(ApiResponse::success(OverallStatistics {
        total_strategies,
        running_strategies,
        total_trades,
        total_profit_usd,
        today_trades,
        today_profit_usd,
    }))
}

/// 获取策略统计
pub async fn get_strategy_statistics(
    State(state): State<AppState>,
    Path(strategy_id): Path<i64>,
) -> Json<ApiResponse<StatisticsResponse>> {
    match sqlx::query_as::<_, (i64, i64, i64, f64, f64, f64, f64, f64)>(
        r#"
        SELECT
            total_trades,
            successful_trades,
            failed_trades,
            total_profit_usd,
            total_gas_cost_usd,
            net_profit_usd,
            win_rate,
            avg_profit_per_trade
        FROM strategy_statistics
        WHERE strategy_id = ?
        "#
    )
    .bind(strategy_id)
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some((total_trades, successful_trades, failed_trades, total_profit_usd, total_gas_cost_usd, net_profit_usd, win_rate, avg_profit_per_trade))) => {
            Json(ApiResponse::success(StatisticsResponse {
                total_trades,
                successful_trades,
                failed_trades,
                total_profit_usd,
                total_gas_cost_usd,
                net_profit_usd,
                win_rate,
                avg_profit_per_trade,
            }))
        }
        Ok(None) => {
            // 返回空统计
            Json(ApiResponse::success(StatisticsResponse {
                total_trades: 0,
                successful_trades: 0,
                failed_trades: 0,
                total_profit_usd: 0.0,
                total_gas_cost_usd: 0.0,
                net_profit_usd: 0.0,
                win_rate: 0.0,
                avg_profit_per_trade: 0.0,
            }))
        }
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}
