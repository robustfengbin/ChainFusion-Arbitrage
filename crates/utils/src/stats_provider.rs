use ethers::providers::{Http, JsonRpcClient, Provider, ProviderError};
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;
use std::sync::Arc;
use tokio::task::JoinHandle;
use url::Url;

use crate::api_stats::{log_api_stats, record_rpc_request};

/// 带统计功能的 HTTP 传输层
#[derive(Debug, Clone)]
pub struct StatsHttp {
    inner: Http,
}

impl StatsHttp {
    pub fn new(url: &str) -> Result<Self, ProviderError> {
        let parsed_url: Url = url.parse().map_err(|e| {
            ProviderError::CustomError(format!("Invalid URL: {}", e))
        })?;
        let inner = Http::new(parsed_url);
        Ok(Self { inner })
    }
}

#[async_trait::async_trait]
impl JsonRpcClient for StatsHttp {
    type Error = <Http as JsonRpcClient>::Error;

    async fn request<T, R>(&self, method: &str, params: T) -> Result<R, Self::Error>
    where
        T: Debug + Serialize + Send + Sync,
        R: DeserializeOwned + Send,
    {
        record_rpc_request();
        JsonRpcClient::request(&self.inner, method, params).await
    }
}

/// RPC 统计 Provider 管理器
///
/// 封装了带统计功能的 Provider 和定时日志任务
pub struct RpcStatsProvider {
    provider: Arc<Provider<StatsHttp>>,
    log_task: Option<JoinHandle<()>>,
}

impl RpcStatsProvider {
    /// 创建新的统计 Provider
    ///
    /// - `url`: RPC 节点 URL
    /// - `log_interval_secs`: 统计日志输出间隔（秒），设为 0 则不启动定时任务
    pub fn new(url: &str, log_interval_secs: u64) -> Result<Self, ProviderError> {
        let stats_http = StatsHttp::new(url)?;
        let provider = Arc::new(Provider::new(stats_http));

        let log_task = if log_interval_secs > 0 {
            Some(tokio::spawn(async move {
                let mut interval = tokio::time::interval(
                    tokio::time::Duration::from_secs(log_interval_secs)
                );
                loop {
                    interval.tick().await;
                    log_api_stats();
                }
            }))
        } else {
            None
        };

        Ok(Self { provider, log_task })
    }

    /// 获取 Provider 引用
    pub fn provider(&self) -> Arc<Provider<StatsHttp>> {
        self.provider.clone()
    }

    /// 停止统计日志任务并输出最终统计
    pub fn stop(&mut self) {
        // 输出最终统计
        log_api_stats();

        // 停止定时任务
        if let Some(handle) = self.log_task.take() {
            handle.abort();
        }
    }
}

impl Drop for RpcStatsProvider {
    fn drop(&mut self) {
        if let Some(handle) = self.log_task.take() {
            handle.abort();
        }
    }
}
