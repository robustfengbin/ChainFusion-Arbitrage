use axum::{
    extract::State,
    Json,
};
use serde::Serialize;

use crate::state::AppState;
use super::strategy::ApiResponse;

#[derive(Serialize)]
pub struct SystemStatus {
    pub status: String,
    pub strategy_running: bool,
    pub database_connected: bool,
    pub uptime_seconds: u64,
    pub version: String,
}

#[derive(Serialize)]
pub struct PoolInfo {
    pub address: String,
    pub chain_id: u64,
    pub dex_type: String,
    pub token0: String,
    pub token1: String,
    pub fee: u32,
    pub last_updated_block: i64,
}

/// 获取系统状态
pub async fn get_system_status(
    State(state): State<AppState>,
) -> Json<ApiResponse<SystemStatus>> {
    // 检查是否有运行中的策略
    let running_strategies = state.strategy_manager.get_running_strategy_ids().await;
    let strategy_running = !running_strategies.is_empty();

    // 检查数据库连接
    let database_connected = sqlx::query("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();

    Json(ApiResponse::success(SystemStatus {
        status: "running".to_string(),
        strategy_running,
        database_connected,
        uptime_seconds: 0, // TODO: 实现 uptime 追踪
        version: env!("CARGO_PKG_VERSION").to_string(),
    }))
}

/// 获取池子列表
pub async fn list_pools(
    State(state): State<AppState>,
) -> Json<ApiResponse<Vec<PoolInfo>>> {
    match sqlx::query_as::<_, (String, i64, String, String, String, i32, i64)>(
        "SELECT address, chain_id, dex_type, token0, token1, fee, last_updated_block FROM pool_cache ORDER BY last_updated_block DESC LIMIT 100"
    )
    .fetch_all(&state.db)
    .await
    {
        Ok(rows) => {
            let pools: Vec<PoolInfo> = rows
                .into_iter()
                .map(|(address, chain_id, dex_type, token0, token1, fee, last_updated_block)| {
                    PoolInfo {
                        address,
                        chain_id: chain_id as u64,
                        dex_type,
                        token0,
                        token1,
                        fee: fee as u32,
                        last_updated_block,
                    }
                })
                .collect();
            Json(ApiResponse::success(pools))
        }
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}
