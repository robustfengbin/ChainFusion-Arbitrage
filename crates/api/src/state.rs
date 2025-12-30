use ethers::providers::{Provider, Middleware};
use services::Database;
use sqlx::{MySql, Pool};
use std::sync::Arc;
use strategies::ArbitrageStrategyManager;
use utils::StatsHttp;

/// API 应用状态 (使用带统计的 Provider)
pub type AppState = AppStateGeneric<Provider<StatsHttp>>;

/// 泛型 API 应用状态
#[derive(Clone)]
pub struct AppStateGeneric<M: Middleware + 'static> {
    pub db: Pool<MySql>,
    pub database: Arc<Database>,
    /// 套利策略管理器
    pub strategy_manager: Arc<ArbitrageStrategyManager<M>>,
}

impl<M: Middleware + 'static> AppStateGeneric<M> {
    pub fn new(
        database: Database,
        strategy_manager: Arc<ArbitrageStrategyManager<M>>,
    ) -> Self {
        Self {
            db: database.pool().clone(),
            database: Arc::new(database),
            strategy_manager,
        }
    }
}
