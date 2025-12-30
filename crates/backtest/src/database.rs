//! 回测数据库操作

use anyhow::Result;
use sqlx::{mysql::MySqlPoolOptions, MySql, Pool, Row};
use tracing::info;

use crate::models::{PoolConfig, PoolPathConfig};

/// 回测数据库
pub struct BacktestDatabase {
    pool: Pool<MySql>,
}

impl BacktestDatabase {
    /// 创建数据库连接
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = MySqlPoolOptions::new()
            .max_connections(10)
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    sqlx::query("SET time_zone = '+08:00'")
                        .execute(&mut *conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(database_url)
            .await?;

        Ok(Self { pool })
    }

    /// 初始化回测相关表
    pub async fn initialize_tables(&self) -> Result<()> {
        // 回测 Swap 数据表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS backtest_swaps (
                id BIGINT PRIMARY KEY AUTO_INCREMENT,
                chain_id BIGINT NOT NULL,
                block_number BIGINT NOT NULL,
                block_timestamp BIGINT NOT NULL,
                tx_hash VARCHAR(66) NOT NULL,
                log_index INT NOT NULL,
                pool_address VARCHAR(42) NOT NULL,
                amount0 VARCHAR(78) NOT NULL,
                amount1 VARCHAR(78) NOT NULL,
                sqrt_price_x96 VARCHAR(78) NOT NULL,
                tick INT NOT NULL,
                liquidity VARCHAR(78) NOT NULL,
                usd_volume DECIMAL(30, 8) NOT NULL DEFAULT 0,
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                INDEX idx_chain_block (chain_id, block_number),
                INDEX idx_pool_block (pool_address, block_number),
                UNIQUE KEY uk_tx_log (tx_hash, log_index)
            ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("✓ backtest_swaps 表已创建/验证");

        // 回测结果表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS backtest_results (
                id BIGINT PRIMARY KEY AUTO_INCREMENT,
                chain_id BIGINT NOT NULL,
                start_block BIGINT NOT NULL,
                end_block BIGINT NOT NULL,
                start_time TIMESTAMP NOT NULL,
                end_time TIMESTAMP NOT NULL,
                total_blocks BIGINT NOT NULL,
                blocks_with_swaps BIGINT NOT NULL,
                total_volume_usd DECIMAL(30, 8) NOT NULL,
                total_opportunities BIGINT NOT NULL,
                profitable_opportunities BIGINT NOT NULL,
                max_profit_usd DECIMAL(20, 8) NOT NULL,
                total_profit_usd DECIMAL(20, 8) NOT NULL,
                config_json JSON NOT NULL,
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                INDEX idx_chain_time (chain_id, created_at)
            ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("✓ backtest_results 表已创建/验证");

        // 回测机会详情表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS backtest_opportunities (
                id BIGINT PRIMARY KEY AUTO_INCREMENT,
                backtest_id BIGINT NOT NULL,
                block_number BIGINT NOT NULL,
                block_timestamp BIGINT NOT NULL,
                path_name VARCHAR(100) NOT NULL,
                triangle_name VARCHAR(100) NOT NULL,
                real_volume_usd DECIMAL(30, 8) NOT NULL,
                capture_percent INT NOT NULL,
                input_amount_usd DECIMAL(30, 8) NOT NULL,
                output_amount_usd DECIMAL(30, 8) NOT NULL,
                gross_profit_usd DECIMAL(20, 8) NOT NULL,
                gas_cost_usd DECIMAL(20, 8) NOT NULL,
                net_profit_usd DECIMAL(20, 8) NOT NULL,
                is_profitable BOOLEAN NOT NULL,
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                INDEX idx_backtest_id (backtest_id),
                INDEX idx_profitable (is_profitable, net_profit_usd DESC)
            ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("✓ backtest_opportunities 表已创建/验证");

        Ok(())
    }

    /// 获取启用的池子配置
    pub async fn get_enabled_pools(&self, chain_id: i64) -> Result<Vec<PoolConfig>> {
        let pools = sqlx::query_as::<_, PoolConfig>(
            r#"
            SELECT id, chain_id, address, dex_type, token0, token0_symbol,
                   token1, token1_symbol, fee, enabled
            FROM arbitrage_pools
            WHERE chain_id = ? AND enabled = TRUE
            ORDER BY id
            "#,
        )
        .bind(chain_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(pools)
    }

    /// 获取启用的套利路径配置
    pub async fn get_enabled_paths(&self, chain_id: i64) -> Result<Vec<PoolPathConfig>> {
        let paths = sqlx::query_as::<_, PoolPathConfig>(
            r#"
            SELECT id, chain_id, trigger_pool, path_name, triangle_name,
                   token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled
            FROM arbitrage_pool_paths
            WHERE chain_id = ? AND enabled = TRUE
            ORDER BY priority, id
            "#,
        )
        .bind(chain_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(paths)
    }

    /// 保存 Swap 数据（批量）
    pub async fn save_swaps(&self, swaps: &[(i64, crate::models::SwapEventData, f64)]) -> Result<u64> {
        if swaps.is_empty() {
            return Ok(0);
        }

        let mut saved = 0u64;

        // 批量插入，每次 1000 条
        for chunk in swaps.chunks(1000) {
            let mut query = String::from(
                "INSERT IGNORE INTO backtest_swaps (chain_id, block_number, block_timestamp, tx_hash, log_index, pool_address, amount0, amount1, sqrt_price_x96, tick, liquidity, usd_volume) VALUES "
            );

            let values: Vec<String> = chunk
                .iter()
                .map(|(chain_id, swap, usd_vol)| {
                    format!(
                        "({}, {}, {}, '{}', {}, '{}', '{}', '{}', '{}', {}, '{}', {})",
                        chain_id,
                        swap.block_number,
                        swap.block_timestamp,
                        format!("{:?}", swap.tx_hash),
                        swap.log_index,
                        format!("{:?}", swap.pool_address),
                        swap.amount0,
                        swap.amount1,
                        swap.sqrt_price_x96,
                        swap.tick,
                        swap.liquidity,
                        usd_vol
                    )
                })
                .collect();

            query.push_str(&values.join(", "));

            let result = sqlx::query(&query).execute(&self.pool).await?;
            saved += result.rows_affected();
        }

        Ok(saved)
    }

    /// 获取已下载的最新区块
    pub async fn get_latest_downloaded_block(&self, chain_id: i64) -> Result<Option<u64>> {
        let row = sqlx::query(
            "SELECT MAX(block_number) as max_block FROM backtest_swaps WHERE chain_id = ?",
        )
        .bind(chain_id)
        .fetch_one(&self.pool)
        .await?;

        let max_block: Option<i64> = row.try_get("max_block").ok();
        Ok(max_block.map(|b| b as u64))
    }

    /// 获取区块范围内的 Swap 数据（简化版本）
    pub async fn get_swaps_in_range(
        &self,
        chain_id: i64,
        start_block: u64,
        end_block: u64,
    ) -> Result<Vec<(u64, u64, String, f64)>> {
        // 返回 (block_number, block_timestamp, pool_address, usd_volume)
        let rows = sqlx::query(
            r#"
            SELECT block_number, block_timestamp, pool_address, usd_volume
            FROM backtest_swaps
            WHERE chain_id = ? AND block_number >= ? AND block_number <= ?
            ORDER BY block_number, log_index
            "#,
        )
        .bind(chain_id)
        .bind(start_block as i64)
        .bind(end_block as i64)
        .fetch_all(&self.pool)
        .await?;

        let results: Vec<_> = rows
            .iter()
            .map(|row| {
                let block_number: i64 = row.get("block_number");
                let block_timestamp: i64 = row.get("block_timestamp");
                let pool_address: String = row.get("pool_address");
                let usd_volume: rust_decimal::Decimal = row.get("usd_volume");
                (block_number as u64, block_timestamp as u64, pool_address, usd_volume.to_string().parse().unwrap_or(0.0))
            })
            .collect();

        Ok(results)
    }

    /// 获取区块范围内的完整 Swap 数据（包含价格和流动性）
    pub async fn get_full_swaps_in_range(
        &self,
        chain_id: i64,
        start_block: u64,
        end_block: u64,
    ) -> Result<Vec<crate::models::SwapRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT block_number, block_timestamp, pool_address, amount0, amount1,
                   sqrt_price_x96, tick, liquidity, usd_volume
            FROM backtest_swaps
            WHERE chain_id = ? AND block_number >= ? AND block_number <= ?
            ORDER BY block_number, log_index
            "#,
        )
        .bind(chain_id)
        .bind(start_block as i64)
        .bind(end_block as i64)
        .fetch_all(&self.pool)
        .await?;

        let results: Vec<_> = rows
            .iter()
            .map(|row| {
                crate::models::SwapRecord {
                    block_number: row.get::<i64, _>("block_number") as u64,
                    block_timestamp: row.get::<i64, _>("block_timestamp") as u64,
                    pool_address: row.get("pool_address"),
                    amount0: row.get("amount0"),
                    amount1: row.get("amount1"),
                    sqrt_price_x96: row.get("sqrt_price_x96"),
                    tick: row.get("tick"),
                    liquidity: row.get("liquidity"),
                    usd_volume: {
                        let vol: rust_decimal::Decimal = row.get("usd_volume");
                        vol.to_string().parse().unwrap_or(0.0)
                    },
                }
            })
            .collect();

        Ok(results)
    }

    /// 获取指定区块的所有池子最新价格快照
    pub async fn get_block_prices(
        &self,
        chain_id: i64,
        block_number: u64,
    ) -> Result<std::collections::HashMap<String, crate::models::PriceSnapshot>> {
        // 获取该区块或之前最近的价格数据
        let rows = sqlx::query(
            r#"
            SELECT s.pool_address, s.sqrt_price_x96, s.tick, s.liquidity, s.block_number
            FROM backtest_swaps s
            INNER JOIN (
                SELECT pool_address, MAX(block_number) as max_block
                FROM backtest_swaps
                WHERE chain_id = ? AND block_number <= ?
                GROUP BY pool_address
            ) latest ON s.pool_address = latest.pool_address AND s.block_number = latest.max_block
            WHERE s.chain_id = ?
            "#,
        )
        .bind(chain_id)
        .bind(block_number as i64)
        .bind(chain_id)
        .fetch_all(&self.pool)
        .await?;

        let mut prices = std::collections::HashMap::new();
        for row in rows {
            let pool_address: String = row.get("pool_address");
            prices.insert(
                pool_address.to_lowercase(),
                crate::models::PriceSnapshot {
                    sqrt_price_x96: row.get("sqrt_price_x96"),
                    tick: row.get("tick"),
                    liquidity: row.get("liquidity"),
                    block_number: row.get::<i64, _>("block_number") as u64,
                },
            );
        }

        Ok(prices)
    }

    /// 保存回测结果
    pub async fn save_backtest_result(
        &self,
        chain_id: i64,
        start_block: u64,
        end_block: u64,
        start_time: chrono::DateTime<chrono::Utc>,
        end_time: chrono::DateTime<chrono::Utc>,
        total_blocks: u64,
        blocks_with_swaps: u64,
        total_volume_usd: f64,
        total_opportunities: u64,
        profitable_opportunities: u64,
        max_profit_usd: f64,
        total_profit_usd: f64,
        config_json: &str,
    ) -> Result<i64> {
        let result = sqlx::query(
            r#"
            INSERT INTO backtest_results
            (chain_id, start_block, end_block, start_time, end_time, total_blocks,
             blocks_with_swaps, total_volume_usd, total_opportunities, profitable_opportunities,
             max_profit_usd, total_profit_usd, config_json)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(chain_id)
        .bind(start_block as i64)
        .bind(end_block as i64)
        .bind(start_time)
        .bind(end_time)
        .bind(total_blocks as i64)
        .bind(blocks_with_swaps as i64)
        .bind(total_volume_usd)
        .bind(total_opportunities as i64)
        .bind(profitable_opportunities as i64)
        .bind(max_profit_usd)
        .bind(total_profit_usd)
        .bind(config_json)
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_id() as i64)
    }

    /// 保存套利机会详情
    pub async fn save_opportunities(
        &self,
        backtest_id: i64,
        opportunities: &[crate::models::ArbitrageOpportunity],
    ) -> Result<u64> {
        if opportunities.is_empty() {
            return Ok(0);
        }

        let mut saved = 0u64;

        for chunk in opportunities.chunks(1000) {
            let mut query = String::from(
                "INSERT INTO backtest_opportunities (backtest_id, block_number, block_timestamp, path_name, triangle_name, real_volume_usd, capture_percent, input_amount_usd, output_amount_usd, gross_profit_usd, gas_cost_usd, net_profit_usd, is_profitable) VALUES "
            );

            let values: Vec<String> = chunk
                .iter()
                .map(|opp| {
                    format!(
                        "({}, {}, {}, '{}', '{}', {}, {}, {}, {}, {}, {}, {}, {})",
                        backtest_id,
                        opp.block_number,
                        opp.block_timestamp,
                        opp.path_name,
                        opp.triangle_name,
                        opp.real_volume_usd,
                        opp.capture_percent,
                        opp.input_amount_usd,
                        opp.output_amount_usd,
                        opp.gross_profit_usd,
                        opp.gas_cost_usd,
                        opp.net_profit_usd,
                        opp.is_profitable
                    )
                })
                .collect();

            query.push_str(&values.join(", "));

            let result = sqlx::query(&query).execute(&self.pool).await?;
            saved += result.rows_affected();
        }

        Ok(saved)
    }
}
