use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::state::AppState;

#[derive(Serialize)]
pub struct StrategyResponse {
    pub id: i64,
    pub name: String,
    pub chain_id: u64,
    pub status: String,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct StrategyDetailResponse {
    pub id: i64,
    pub name: String,
    pub chain_id: u64,
    pub min_profit_threshold_usd: f64,
    pub max_slippage: f64,
    pub target_tokens: Vec<String>,
    pub target_dexes: Vec<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
pub struct CreateStrategyRequest {
    pub name: String,
    pub chain_id: u64,
    pub min_profit_threshold_usd: f64,
    pub max_slippage: f64,
    pub target_tokens: Vec<String>,
    pub target_dexes: Vec<String>,
}

#[derive(Deserialize)]
pub struct UpdateStrategyRequest {
    pub name: String,
    pub chain_id: u64,
    pub min_profit_threshold_usd: f64,
    pub max_slippage: f64,
    pub target_tokens: Vec<String>,
    pub target_dexes: Vec<String>,
}

#[derive(Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(message: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message),
        }
    }
}

#[derive(Serialize)]
pub struct MessageResponse {
    pub message: String,
}

/// 获取策略列表
pub async fn list_strategies(
    State(state): State<AppState>,
) -> Json<ApiResponse<Vec<StrategyResponse>>> {
    match sqlx::query_as::<_, (i64, String, i64, String, chrono::DateTime<chrono::Utc>)>(
        "SELECT id, name, chain_id, status, created_at FROM arbitrage_strategies ORDER BY id DESC"
    )
    .fetch_all(&state.db)
    .await
    {
        Ok(rows) => {
            let strategies: Vec<StrategyResponse> = rows
                .into_iter()
                .map(|(id, name, chain_id, status, created_at)| StrategyResponse {
                    id,
                    name,
                    chain_id: chain_id as u64,
                    status,
                    created_at: created_at.to_rfc3339(),
                })
                .collect();
            Json(ApiResponse::success(strategies))
        }
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

/// 创建策略
pub async fn create_strategy(
    State(state): State<AppState>,
    Json(req): Json<CreateStrategyRequest>,
) -> Json<ApiResponse<StrategyResponse>> {
    let target_tokens = serde_json::to_value(&req.target_tokens).unwrap();
    let target_dexes = serde_json::to_value(&req.target_dexes).unwrap();

    match sqlx::query(
        r#"
        INSERT INTO arbitrage_strategies
        (name, chain_id, min_profit_threshold_usd, max_slippage, target_tokens, target_dexes)
        VALUES (?, ?, ?, ?, ?, ?)
        "#
    )
    .bind(&req.name)
    .bind(req.chain_id as i64)
    .bind(req.min_profit_threshold_usd)
    .bind(req.max_slippage)
    .bind(&target_tokens)
    .bind(&target_dexes)
    .execute(&state.db)
    .await
    {
        Ok(result) => {
            let id = result.last_insert_id() as i64;
            info!("创建策略成功: id={}", id);
            Json(ApiResponse::success(StrategyResponse {
                id,
                name: req.name,
                chain_id: req.chain_id,
                status: "stopped".to_string(),
                created_at: chrono::Utc::now().to_rfc3339(),
            }))
        }
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

/// 获取策略详情
pub async fn get_strategy(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<StrategyDetailResponse>> {
    match sqlx::query_as::<_, (i64, String, i64, Decimal, Decimal, serde_json::Value, serde_json::Value, String, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
        "SELECT id, name, chain_id, min_profit_threshold_usd, max_slippage, target_tokens, target_dexes, status, created_at, updated_at FROM arbitrage_strategies WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some((id, name, chain_id, min_profit_threshold_usd, max_slippage, target_tokens, target_dexes, status, created_at, updated_at))) => {
            let tokens: Vec<String> = serde_json::from_value(target_tokens).unwrap_or_default();
            let dexes: Vec<String> = serde_json::from_value(target_dexes).unwrap_or_default();
            use rust_decimal::prelude::ToPrimitive;
            Json(ApiResponse::success(StrategyDetailResponse {
                id,
                name,
                chain_id: chain_id as u64,
                min_profit_threshold_usd: min_profit_threshold_usd.to_f64().unwrap_or(0.0),
                max_slippage: max_slippage.to_f64().unwrap_or(0.0),
                target_tokens: tokens,
                target_dexes: dexes,
                status,
                created_at: created_at.to_rfc3339(),
                updated_at: updated_at.to_rfc3339(),
            }))
        }
        Ok(None) => Json(ApiResponse::error("策略不存在".to_string())),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

/// 启动策略
pub async fn start_strategy(
    State(state): State<AppState>,
    Path(strategy_id): Path<i64>,
) -> Result<Json<MessageResponse>, (StatusCode, String)> {
    // 直接调用 strategy_manager 启动策略
    state
        .strategy_manager
        .start_strategy(strategy_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(MessageResponse {
        message: format!("策略 {} 已启动", strategy_id),
    }))
}

/// 停止策略
pub async fn stop_strategy(
    State(state): State<AppState>,
    Path(strategy_id): Path<i64>,
) -> Result<Json<MessageResponse>, (StatusCode, String)> {
    // 直接调用 strategy_manager 停止策略
    state
        .strategy_manager
        .stop_strategy(strategy_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(MessageResponse {
        message: format!("策略 {} 已停止", strategy_id),
    }))
}

/// 获取运行中的策略列表
pub async fn get_running_strategies(
    State(state): State<AppState>,
) -> Result<Json<Vec<i64>>, (StatusCode, String)> {
    let strategy_ids = state.strategy_manager.get_running_strategy_ids().await;
    Ok(Json(strategy_ids))
}

/// 更新策略
pub async fn update_strategy(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateStrategyRequest>,
) -> Json<ApiResponse<StrategyDetailResponse>> {
    let target_tokens = serde_json::to_value(&req.target_tokens).unwrap();
    let target_dexes = serde_json::to_value(&req.target_dexes).unwrap();

    match sqlx::query(
        r#"
        UPDATE arbitrage_strategies SET
            name = ?,
            chain_id = ?,
            min_profit_threshold_usd = ?,
            max_slippage = ?,
            target_tokens = ?,
            target_dexes = ?,
            updated_at = NOW()
        WHERE id = ?
        "#
    )
    .bind(&req.name)
    .bind(req.chain_id as i64)
    .bind(req.min_profit_threshold_usd)
    .bind(req.max_slippage)
    .bind(&target_tokens)
    .bind(&target_dexes)
    .bind(id)
    .execute(&state.db)
    .await
    {
        Ok(result) => {
            if result.rows_affected() == 0 {
                return Json(ApiResponse::error("策略不存在".to_string()));
            }
            info!("更新策略成功: id={}", id);
            Json(ApiResponse::success(StrategyDetailResponse {
                id,
                name: req.name,
                chain_id: req.chain_id,
                min_profit_threshold_usd: req.min_profit_threshold_usd,
                max_slippage: req.max_slippage,
                target_tokens: req.target_tokens,
                target_dexes: req.target_dexes,
                status: "stopped".to_string(),
                created_at: chrono::Utc::now().to_rfc3339(),
                updated_at: chrono::Utc::now().to_rfc3339(),
            }))
        }
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

/// 删除策略
pub async fn delete_strategy(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<String>> {
    // 先检查策略是否正在运行
    let running_ids = state.strategy_manager.get_running_strategy_ids().await;
    if running_ids.contains(&id) {
        return Json(ApiResponse::error("请先停止策略后再删除".to_string()));
    }

    // 删除策略
    match sqlx::query("DELETE FROM arbitrage_strategies WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await
    {
        Ok(result) => {
            if result.rows_affected() == 0 {
                return Json(ApiResponse::error("策略不存在".to_string()));
            }
            info!("删除策略: id={}", id);
            Json(ApiResponse::success("策略已删除".to_string()))
        }
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}
