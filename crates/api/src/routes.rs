use axum::{
    routing::{get, post, put, delete},
    Router,
};
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use crate::handlers;
use crate::state::AppState;

/// 创建 API 服务器
pub async fn create_server(state: AppState, _host: &str, _port: u16) -> Router {
    // CORS 配置
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // 创建路由
    let app = Router::new()
        // 健康检查
        .route("/health", get(handlers::health_check))
        // 策略相关
        .route("/api/strategies", get(handlers::list_strategies))
        .route("/api/strategies", post(handlers::create_strategy))
        .route("/api/strategies/:id", get(handlers::get_strategy))
        .route("/api/strategies/:id", put(handlers::update_strategy))
        .route("/api/strategies/:id", delete(handlers::delete_strategy))
        .route("/api/strategies/:id/start", post(handlers::start_strategy))
        .route("/api/strategies/:id/stop", post(handlers::stop_strategy))
        .route("/api/strategies/running", get(handlers::get_running_strategies))
        // 交易记录
        .route("/api/trades", get(handlers::list_trades))
        .route("/api/trades/:id", get(handlers::get_trade))
        // 统计信息
        .route("/api/statistics", get(handlers::get_statistics))
        .route("/api/statistics/:strategy_id", get(handlers::get_strategy_statistics))
        // 套利机会
        .route("/api/opportunities", get(handlers::list_opportunities))
        // 系统状态
        .route("/api/system/status", get(handlers::get_system_status))
        .route("/api/system/pools", get(handlers::list_pools))
        .layer(cors)
        .with_state(state);

    app
}

/// 启动服务器
pub async fn start_server(app: Router, host: &str, port: u16) {
    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

    info!("API 服务器启动: http://{}", addr);

    axum::serve(listener, app).await.unwrap();
}
