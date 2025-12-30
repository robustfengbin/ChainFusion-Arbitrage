//! API 请求统计模块
//!
//! 统计 RPC 请求和 WebSocket 事件

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tracing::info;

/// 时间窗口统计器
struct TimeWindowCounter {
    /// 时间戳队列
    timestamps: RwLock<VecDeque<Instant>>,
    /// 总计数
    total: AtomicU64,
}

impl TimeWindowCounter {
    fn new() -> Self {
        Self {
            timestamps: RwLock::new(VecDeque::with_capacity(10000)),
            total: AtomicU64::new(0),
        }
    }

    /// 记录一次事件
    fn record(&self) {
        let now = Instant::now();
        self.total.fetch_add(1, Ordering::Relaxed);

        let mut timestamps = self.timestamps.write();
        timestamps.push_back(now);

        // 清理超过1小时的旧时间戳
        let one_hour_ago = now - Duration::from_secs(3600);
        while let Some(front) = timestamps.front() {
            if *front < one_hour_ago {
                timestamps.pop_front();
            } else {
                break;
            }
        }
    }

    /// 获取统计
    fn get_counts(&self) -> (u64, u64, u64, u64) {
        let now = Instant::now();
        let timestamps = self.timestamps.read();

        let one_second_ago = now - Duration::from_secs(1);
        let one_minute_ago = now - Duration::from_secs(60);
        let one_hour_ago = now - Duration::from_secs(3600);

        let mut last_1s = 0u64;
        let mut last_1m = 0u64;
        let mut last_1h = 0u64;

        for ts in timestamps.iter().rev() {
            if *ts >= one_second_ago {
                last_1s += 1;
                last_1m += 1;
                last_1h += 1;
            } else if *ts >= one_minute_ago {
                last_1m += 1;
                last_1h += 1;
            } else if *ts >= one_hour_ago {
                last_1h += 1;
            } else {
                break;
            }
        }

        (last_1s, last_1m, last_1h, self.total.load(Ordering::Relaxed))
    }
}

/// API 统计器
///
/// 统计 RPC 请求和 WebSocket 事件
pub struct ApiStats {
    /// 启动时间
    start_time: Instant,
    /// RPC 请求统计
    rpc: TimeWindowCounter,
    /// WebSocket 区块事件统计
    ws_blocks: TimeWindowCounter,
    /// WebSocket Swap 事件统计
    ws_swaps: TimeWindowCounter,
}

impl ApiStats {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            rpc: TimeWindowCounter::new(),
            ws_blocks: TimeWindowCounter::new(),
            ws_swaps: TimeWindowCounter::new(),
        }
    }

    /// 记录 RPC 请求
    pub fn record_rpc(&self) {
        self.rpc.record();
    }

    /// 记录 WebSocket 区块事件
    pub fn record_ws_block(&self) {
        self.ws_blocks.record();
    }

    /// 记录 WebSocket Swap 事件
    pub fn record_ws_swap(&self) {
        self.ws_swaps.record();
    }

    /// 获取统计快照
    pub fn get_stats(&self) -> ApiStatsSnapshot {
        let uptime = self.start_time.elapsed().as_secs();

        let (rpc_1s, rpc_1m, rpc_1h, rpc_total) = self.rpc.get_counts();
        let (ws_block_1s, ws_block_1m, ws_block_1h, ws_block_total) = self.ws_blocks.get_counts();
        let (ws_swap_1s, ws_swap_1m, ws_swap_1h, ws_swap_total) = self.ws_swaps.get_counts();

        let rpc_avg = if uptime > 0 { rpc_total as f64 / uptime as f64 } else { 0.0 };
        let ws_block_avg = if uptime > 0 { ws_block_total as f64 / uptime as f64 } else { 0.0 };
        let ws_swap_avg = if uptime > 0 { ws_swap_total as f64 / uptime as f64 } else { 0.0 };

        ApiStatsSnapshot {
            uptime_seconds: uptime,
            rpc: CounterSnapshot {
                last_1s: rpc_1s,
                last_1m: rpc_1m,
                last_1h: rpc_1h,
                total: rpc_total,
                avg_per_sec: rpc_avg,
            },
            ws_blocks: CounterSnapshot {
                last_1s: ws_block_1s,
                last_1m: ws_block_1m,
                last_1h: ws_block_1h,
                total: ws_block_total,
                avg_per_sec: ws_block_avg,
            },
            ws_swaps: CounterSnapshot {
                last_1s: ws_swap_1s,
                last_1m: ws_swap_1m,
                last_1h: ws_swap_1h,
                total: ws_swap_total,
                avg_per_sec: ws_swap_avg,
            },
        }
    }

    /// 输出统计日志
    pub fn log_stats(&self) {
        let s = self.get_stats();

        info!(
            target: "rpc_stats",
            uptime_secs = s.uptime_seconds,
            rpc_1m = s.rpc.last_1m,
            rpc_total = s.rpc.total,
            rpc_avg = format!("{:.2}", s.rpc.avg_per_sec),
            ws_block_1m = s.ws_blocks.last_1m,
            ws_block_total = s.ws_blocks.total,
            ws_swap_1m = s.ws_swaps.last_1m,
            ws_swap_total = s.ws_swaps.total,
            "API统计"
        );
    }
}

impl Default for ApiStats {
    fn default() -> Self {
        Self::new()
    }
}

/// 单项计数器快照
#[derive(Debug, Clone)]
pub struct CounterSnapshot {
    pub last_1s: u64,
    pub last_1m: u64,
    pub last_1h: u64,
    pub total: u64,
    pub avg_per_sec: f64,
}

/// API 统计快照
#[derive(Debug, Clone)]
pub struct ApiStatsSnapshot {
    pub uptime_seconds: u64,
    pub rpc: CounterSnapshot,
    pub ws_blocks: CounterSnapshot,
    pub ws_swaps: CounterSnapshot,
}

impl std::fmt::Display for ApiStatsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "运行{}s | RPC: {}次(avg {:.2}/s) | WS区块: {}个 | WS Swap: {}个",
            self.uptime_seconds,
            self.rpc.total,
            self.rpc.avg_per_sec,
            self.ws_blocks.total,
            self.ws_swaps.total,
        )
    }
}

/// 全局 API 统计实例
pub static API_STATS: Lazy<ApiStats> = Lazy::new(ApiStats::new);

/// 记录 RPC 请求
pub fn record_rpc_request() {
    API_STATS.record_rpc();
}

/// 记录 WebSocket 区块事件
pub fn record_ws_block() {
    API_STATS.record_ws_block();
}

/// 记录 WebSocket Swap 事件
pub fn record_ws_swap() {
    API_STATS.record_ws_swap();
}

/// 获取统计快照
pub fn get_api_stats() -> ApiStatsSnapshot {
    API_STATS.get_stats()
}

/// 输出统计日志
pub fn log_api_stats() {
    API_STATS.log_stats();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_stats() {
        let stats = ApiStats::new();

        for _ in 0..10 {
            stats.record_rpc();
        }
        for _ in 0..5 {
            stats.record_ws_block();
        }
        for _ in 0..20 {
            stats.record_ws_swap();
        }

        let snapshot = stats.get_stats();
        assert_eq!(snapshot.rpc.total, 10);
        assert_eq!(snapshot.ws_blocks.total, 5);
        assert_eq!(snapshot.ws_swaps.total, 20);
    }
}
