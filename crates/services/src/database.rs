use anyhow::Result;
use sqlx::{mysql::MySqlPoolOptions, MySql, Pool};
use tracing::info;

pub struct Database {
    pool: Pool<MySql>,
}

impl Database {
    pub async fn new(database_url: &str, max_connections: u32) -> Result<Self> {
        let pool = MySqlPoolOptions::new()
            .max_connections(max_connections)
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    // 设置会话时区为上海时区 (UTC+8)
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

    /// 从已有连接池创建 Database 实例
    pub fn from_pool(pool: Pool<MySql>) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &Pool<MySql> {
        &self.pool
    }

    /// 初始化数据库表
    pub async fn initialize_tables(&self) -> Result<()> {
        info!("开始初始化数据库表...");

        // 套利策略配置表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS arbitrage_strategies (
                id BIGINT PRIMARY KEY AUTO_INCREMENT,
                name VARCHAR(200) NOT NULL,
                chain_id BIGINT NOT NULL,
                status VARCHAR(50) NOT NULL DEFAULT 'stopped',
                min_profit_threshold_usd DECIMAL(20, 8) NOT NULL DEFAULT 10.0,
                max_slippage DECIMAL(10, 6) NOT NULL DEFAULT 0.0005,
                max_gas_price_gwei DECIMAL(20, 8) NOT NULL DEFAULT 100.0,
                max_position_size_usd DECIMAL(20, 8) NOT NULL DEFAULT 10000.0,
                use_flash_loan BOOLEAN NOT NULL DEFAULT TRUE,
                flash_loan_provider VARCHAR(50) NOT NULL DEFAULT 'uniswap_v3',
                target_tokens JSON NOT NULL,
                target_dexes JSON NOT NULL,
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
                INDEX idx_chain_id (chain_id),
                INDEX idx_status (status)
            ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("✓ 套利策略配置表已创建/验证");

        // 交易记录表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS trade_records (
                id BIGINT PRIMARY KEY AUTO_INCREMENT,
                strategy_id BIGINT NOT NULL,
                tx_hash VARCHAR(66) NOT NULL UNIQUE,
                arbitrage_type VARCHAR(50) NOT NULL,
                path JSON NOT NULL,
                input_token VARCHAR(42) NOT NULL,
                input_amount DECIMAL(36, 18) NOT NULL,
                output_amount DECIMAL(36, 18) NOT NULL,
                profit_usd DECIMAL(20, 8) NOT NULL,
                gas_used DECIMAL(20, 0) NOT NULL,
                gas_price_gwei DECIMAL(20, 8) NOT NULL,
                gas_cost_usd DECIMAL(20, 8) NOT NULL,
                net_profit_usd DECIMAL(20, 8) NOT NULL,
                status VARCHAR(50) NOT NULL,
                error_message TEXT,
                block_number BIGINT NOT NULL,
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                INDEX idx_strategy_id (strategy_id),
                INDEX idx_status (status),
                INDEX idx_block_number (block_number),
                INDEX idx_created_at (created_at)
            ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("✓ 交易记录表已创建/验证");

        // 策略统计表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS strategy_statistics (
                strategy_id BIGINT PRIMARY KEY,
                total_trades BIGINT NOT NULL DEFAULT 0,
                successful_trades BIGINT NOT NULL DEFAULT 0,
                failed_trades BIGINT NOT NULL DEFAULT 0,
                total_profit_usd DECIMAL(20, 8) NOT NULL DEFAULT 0,
                total_gas_cost_usd DECIMAL(20, 8) NOT NULL DEFAULT 0,
                net_profit_usd DECIMAL(20, 8) NOT NULL DEFAULT 0,
                win_rate DECIMAL(10, 4) NOT NULL DEFAULT 0,
                avg_profit_per_trade DECIMAL(20, 8) NOT NULL DEFAULT 0,
                max_profit_trade DECIMAL(20, 8) NOT NULL DEFAULT 0,
                max_loss_trade DECIMAL(20, 8) NOT NULL DEFAULT 0,
                last_trade_at TIMESTAMP NULL,
                updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
            ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("✓ 策略统计表已创建/验证");

        // 价格监控记录表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS price_monitor_records (
                id BIGINT PRIMARY KEY AUTO_INCREMENT,
                token0 VARCHAR(42) NOT NULL,
                token1 VARCHAR(42) NOT NULL,
                dex_type VARCHAR(50) NOT NULL,
                pool_address VARCHAR(42) NOT NULL,
                price DECIMAL(36, 18) NOT NULL,
                liquidity DECIMAL(36, 18) NOT NULL,
                block_number BIGINT NOT NULL,
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                INDEX idx_tokens (token0, token1),
                INDEX idx_pool (pool_address),
                INDEX idx_block_number (block_number),
                INDEX idx_created_at (created_at)
            ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("✓ 价格监控记录表已创建/验证");

        // 套利机会表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS arbitrage_opportunities (
                id BIGINT PRIMARY KEY AUTO_INCREMENT,
                strategy_id BIGINT NOT NULL,
                opportunity_uuid VARCHAR(64),
                path JSON NOT NULL,
                input_amount VARCHAR(78) NOT NULL,
                expected_output VARCHAR(78) NOT NULL,
                expected_profit_usd DECIMAL(20, 8) NOT NULL,
                gas_estimate VARCHAR(78) NOT NULL,
                gas_cost_usd DECIMAL(20, 8) NOT NULL,
                net_profit_usd DECIMAL(20, 8) NOT NULL,
                profit_percentage DECIMAL(10, 4) NOT NULL,
                block_number BIGINT NOT NULL,
                executed BOOLEAN NOT NULL DEFAULT FALSE,
                tx_hash VARCHAR(66),
                error_message TEXT,
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
                INDEX idx_strategy_id (strategy_id),
                INDEX idx_net_profit (net_profit_usd),
                INDEX idx_block_number (block_number),
                INDEX idx_executed (executed),
                INDEX idx_created_at (created_at)
            ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("✓ 套利机会表已创建/验证");

        // 检查并升级旧版 arbitrage_opportunities 表结构
        // 如果表存在但缺少 strategy_id 字段，添加它
        let _ = sqlx::query(
            "ALTER TABLE arbitrage_opportunities ADD COLUMN IF NOT EXISTS strategy_id BIGINT NOT NULL DEFAULT 0 AFTER id"
        )
        .execute(&self.pool)
        .await;

        let _ = sqlx::query(
            "ALTER TABLE arbitrage_opportunities ADD COLUMN IF NOT EXISTS error_message TEXT AFTER tx_hash"
        )
        .execute(&self.pool)
        .await;

        let _ = sqlx::query(
            "ALTER TABLE arbitrage_opportunities ADD COLUMN IF NOT EXISTS updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP"
        )
        .execute(&self.pool)
        .await;

        // 池子信息缓存表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS pool_cache (
                address VARCHAR(42) PRIMARY KEY,
                chain_id BIGINT NOT NULL,
                dex_type VARCHAR(50) NOT NULL,
                token0 VARCHAR(42) NOT NULL,
                token0_symbol VARCHAR(20) NOT NULL DEFAULT '',
                token1 VARCHAR(42) NOT NULL,
                token1_symbol VARCHAR(20) NOT NULL DEFAULT '',
                fee INT NOT NULL,
                reserve0 VARCHAR(78),
                reserve1 VARCHAR(78),
                sqrt_price_x96 VARCHAR(78),
                tick INT,
                liquidity VARCHAR(78),
                last_updated_block BIGINT NOT NULL,
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
                INDEX idx_chain_id (chain_id),
                INDEX idx_tokens (token0, token1),
                INDEX idx_dex_type (dex_type)
            ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("✓ 池子信息缓存表已创建/验证");

        // 升级 pool_cache 表：添加代币符号字段
        let _ = sqlx::query(
            "ALTER TABLE pool_cache ADD COLUMN IF NOT EXISTS token0_symbol VARCHAR(20) NOT NULL DEFAULT '' AFTER token0"
        )
        .execute(&self.pool)
        .await;

        let _ = sqlx::query(
            "ALTER TABLE pool_cache ADD COLUMN IF NOT EXISTS token1_symbol VARCHAR(20) NOT NULL DEFAULT '' AFTER token1"
        )
        .execute(&self.pool)
        .await;

        // 套利代币配置表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS arbitrage_tokens (
                id BIGINT PRIMARY KEY AUTO_INCREMENT,
                chain_id BIGINT NOT NULL,
                address VARCHAR(42) NOT NULL,
                symbol VARCHAR(20) NOT NULL,
                decimals INT NOT NULL DEFAULT 18,
                is_stable BOOLEAN NOT NULL DEFAULT FALSE,
                price_symbol VARCHAR(20) NOT NULL COMMENT '币安交易对符号，如ETH、BTC',
                optimal_input_amount VARCHAR(78) NOT NULL DEFAULT '1000000000000000000' COMMENT '最优输入金额(wei)',
                enabled BOOLEAN NOT NULL DEFAULT TRUE,
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
                UNIQUE KEY uk_chain_address (chain_id, address),
                INDEX idx_chain_enabled (chain_id, enabled),
                INDEX idx_symbol (symbol)
            ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("✓ 套利代币配置表已创建/验证");

        // 三角套利组合配置表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS arbitrage_triangles (
                id BIGINT PRIMARY KEY AUTO_INCREMENT,
                chain_id BIGINT NOT NULL,
                name VARCHAR(100) NOT NULL COMMENT '组合名称，如 DAI-USDC-USDT',
                token_a VARCHAR(42) NOT NULL,
                token_b VARCHAR(42) NOT NULL,
                token_c VARCHAR(42) NOT NULL,
                priority INT NOT NULL DEFAULT 100 COMMENT '优先级，数值越小优先级越高',
                category VARCHAR(50) NOT NULL DEFAULT 'general' COMMENT '分类: stablecoin/eth_stable/btc_stable/major',
                description TEXT COMMENT '说明',
                enabled BOOLEAN NOT NULL DEFAULT TRUE,
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
                UNIQUE KEY uk_chain_tokens (chain_id, token_a, token_b, token_c),
                INDEX idx_chain_enabled (chain_id, enabled),
                INDEX idx_priority (priority)
            ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("✓ 三角套利组合配置表已创建/验证");

        // 套利池子配置表 (替代 pool_cache 的套利功能)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS arbitrage_pools (
                id BIGINT PRIMARY KEY AUTO_INCREMENT,
                chain_id BIGINT NOT NULL,
                address VARCHAR(42) NOT NULL,
                dex_type VARCHAR(50) NOT NULL,
                token0 VARCHAR(42) NOT NULL,
                token0_symbol VARCHAR(20) NOT NULL,
                token1 VARCHAR(42) NOT NULL,
                token1_symbol VARCHAR(20) NOT NULL,
                fee INT NOT NULL COMMENT '手续费，单位：万分之一 (100=0.01%, 500=0.05%, 3000=0.3%)',
                enabled BOOLEAN NOT NULL DEFAULT TRUE,
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
                UNIQUE KEY uk_chain_address (chain_id, address),
                INDEX idx_chain_enabled (chain_id, enabled),
                INDEX idx_tokens (token0, token1)
            ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("✓ 套利池子配置表已创建/验证");

        // 池子-路径映射表：每个池子触发时应检查的套利路径
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS arbitrage_pool_paths (
                id BIGINT PRIMARY KEY AUTO_INCREMENT,
                chain_id BIGINT NOT NULL,
                trigger_pool VARCHAR(42) NOT NULL COMMENT '触发Swap事件的池子地址',
                path_name VARCHAR(100) NOT NULL COMMENT '路径名称，如 DAI→USDC→USDT→DAI',
                triangle_name VARCHAR(100) NOT NULL COMMENT '所属三角组合名称',
                token_a VARCHAR(42) NOT NULL COMMENT '起始/结束代币',
                token_b VARCHAR(42) NOT NULL COMMENT '第二个代币',
                token_c VARCHAR(42) NOT NULL COMMENT '第三个代币',
                pool1 VARCHAR(42) NOT NULL COMMENT '第一跳池子 (A->B)',
                pool2 VARCHAR(42) NOT NULL COMMENT '第二跳池子 (B->C)',
                pool3 VARCHAR(42) NOT NULL COMMENT '第三跳池子 (C->A)',
                priority INT NOT NULL DEFAULT 100 COMMENT '优先级，数值越小优先级越高',
                enabled BOOLEAN NOT NULL DEFAULT TRUE,
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
                UNIQUE KEY uk_chain_pool_path (chain_id, trigger_pool, path_name),
                INDEX idx_chain_trigger (chain_id, trigger_pool),
                INDEX idx_enabled (enabled),
                INDEX idx_priority (priority)
            ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
            "#,
        )
        .execute(&self.pool)
        .await?;
        info!("✓ 池子-路径映射表已创建/验证");

        info!("数据库表初始化完成");
        Ok(())
    }
}

/// 策略数据库操作
pub struct StrategyDb {
    pool: Pool<MySql>,
}

impl StrategyDb {
    pub fn new(pool: Pool<MySql>) -> Self {
        Self { pool }
    }

    /// 获取所有活跃策略
    pub async fn get_active_strategies(&self) -> Result<Vec<models::ArbitrageStrategyConfig>> {
        let strategies = sqlx::query_as::<_, models::ArbitrageStrategyConfig>(
            "SELECT * FROM arbitrage_strategies WHERE status = 'running'"
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(strategies)
    }

    /// 更新策略状态
    pub async fn update_status(&self, strategy_id: i64, status: &str) -> Result<()> {
        sqlx::query("UPDATE arbitrage_strategies SET status = ? WHERE id = ?")
            .bind(status)
            .bind(strategy_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// 记录交易
    pub async fn insert_trade_record(&self, record: &models::TradeRecord) -> Result<i64> {
        let result = sqlx::query(
            r#"
            INSERT INTO trade_records
            (strategy_id, tx_hash, arbitrage_type, path, input_token, input_amount,
             output_amount, profit_usd, gas_used, gas_price_gwei, gas_cost_usd,
             net_profit_usd, status, error_message, block_number)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(record.strategy_id)
        .bind(&record.tx_hash)
        .bind(&record.arbitrage_type)
        .bind(&record.path)
        .bind(&record.input_token)
        .bind(record.input_amount)
        .bind(record.output_amount)
        .bind(record.profit_usd)
        .bind(record.gas_used)
        .bind(record.gas_price_gwei)
        .bind(record.gas_cost_usd)
        .bind(record.net_profit_usd)
        .bind(&record.status)
        .bind(&record.error_message)
        .bind(record.block_number)
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_id() as i64)
    }
}

/// 套利代币配置
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ArbitrageTokenConfig {
    pub id: i64,
    pub chain_id: i64,
    pub address: String,
    pub symbol: String,
    pub decimals: i32,
    pub is_stable: bool,
    pub price_symbol: String,
    pub optimal_input_amount: String,
    pub enabled: bool,
}

/// 三角套利组合配置
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ArbitrageTriangleConfig {
    pub id: i64,
    pub chain_id: i64,
    pub name: String,
    pub token_a: String,
    pub token_b: String,
    pub token_c: String,
    pub priority: i32,
    pub category: String,
    pub enabled: bool,
}

/// 套利配置数据库操作
pub struct ArbitrageConfigDb {
    pool: Pool<MySql>,
}

impl ArbitrageConfigDb {
    pub fn new(pool: Pool<MySql>) -> Self {
        Self { pool }
    }

    /// 获取所有启用的代币配置
    pub async fn get_enabled_tokens(&self, chain_id: u64) -> Result<Vec<ArbitrageTokenConfig>> {
        let tokens = sqlx::query_as::<_, ArbitrageTokenConfig>(
            "SELECT id, chain_id, address, symbol, decimals, is_stable, price_symbol, optimal_input_amount, enabled
             FROM arbitrage_tokens WHERE chain_id = ? AND enabled = TRUE ORDER BY symbol"
        )
        .bind(chain_id as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(tokens)
    }

    /// 获取所有启用的三角组合配置
    pub async fn get_enabled_triangles(&self, chain_id: u64) -> Result<Vec<ArbitrageTriangleConfig>> {
        let triangles = sqlx::query_as::<_, ArbitrageTriangleConfig>(
            "SELECT id, chain_id, name, token_a, token_b, token_c, priority, category, enabled
             FROM arbitrage_triangles WHERE chain_id = ? AND enabled = TRUE ORDER BY priority"
        )
        .bind(chain_id as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(triangles)
    }

    /// 根据地址获取代币配置
    pub async fn get_token_by_address(&self, chain_id: u64, address: &str) -> Result<Option<ArbitrageTokenConfig>> {
        let token = sqlx::query_as::<_, ArbitrageTokenConfig>(
            "SELECT id, chain_id, address, symbol, decimals, is_stable, price_symbol, optimal_input_amount, enabled
             FROM arbitrage_tokens WHERE chain_id = ? AND LOWER(address) = LOWER(?)"
        )
        .bind(chain_id as i64)
        .bind(address)
        .fetch_optional(&self.pool)
        .await?;

        Ok(token)
    }

    /// 初始化默认代币配置 (Ethereum Mainnet)
    pub async fn init_default_tokens(&self) -> Result<()> {
        let chain_id: i64 = 1; // Ethereum Mainnet

        // 检查是否已有配置
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM arbitrage_tokens WHERE chain_id = ?")
            .bind(chain_id)
            .fetch_one(&self.pool)
            .await?;

        if count.0 > 0 {
            info!("代币配置已存在，跳过初始化");
            return Ok(());
        }

        info!("初始化默认代币配置...");

        // Ethereum Mainnet 代币配置
        let tokens = vec![
            // (address, symbol, decimals, is_stable, price_symbol, optimal_input_amount)
            ("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", "WETH", 18, false, "ETH", "1000000000000000000"), // 1 ETH
            ("0xdAC17F958D2ee523a2206206994597C13D831ec7", "USDT", 6, true, "USDT", "3000000000"), // 3000 USDT
            ("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", "USDC", 6, true, "USDC", "3000000000"), // 3000 USDC
            ("0x6B175474E89094C44Da98b954EedeAC495271d0F", "DAI", 18, true, "DAI", "3000000000000000000000"), // 3000 DAI
            ("0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599", "WBTC", 8, false, "BTC", "10000000"), // 0.1 BTC
        ];

        let token_count = tokens.len();
        for (address, symbol, decimals, is_stable, price_symbol, optimal_input) in tokens {
            sqlx::query(
                r#"INSERT IGNORE INTO arbitrage_tokens
                   (chain_id, address, symbol, decimals, is_stable, price_symbol, optimal_input_amount)
                   VALUES (?, ?, ?, ?, ?, ?, ?)"#
            )
            .bind(chain_id)
            .bind(address)
            .bind(symbol)
            .bind(decimals)
            .bind(is_stable)
            .bind(price_symbol)
            .bind(optimal_input)
            .execute(&self.pool)
            .await?;
        }

        info!("✓ 初始化了 {} 个代币配置", token_count);
        Ok(())
    }

    /// 初始化 BSC 默认代币配置
    pub async fn init_bsc_default_tokens(&self) -> Result<()> {
        let chain_id: i64 = 56; // BSC Mainnet

        // 检查是否已有配置
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM arbitrage_tokens WHERE chain_id = ?")
            .bind(chain_id)
            .fetch_one(&self.pool)
            .await?;

        if count.0 > 0 {
            info!("[BSC] 代币配置已存在，跳过初始化");
            return Ok(());
        }

        info!("[BSC] 初始化默认代币配置...");

        // BSC Mainnet 代币配置
        let tokens = vec![
            // (address, symbol, decimals, is_stable, price_symbol, optimal_input_amount)
            ("0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c", "WBNB", 18, false, "BNB", "5000000000000000000"), // 5 BNB
            ("0x55d398326f99059fF775485246999027B3197955", "USDT", 18, true, "USDT", "3000000000000000000000"), // 3000 USDT
            ("0x8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d", "USDC", 18, true, "USDC", "3000000000000000000000"), // 3000 USDC
            ("0xe9e7CEA3DedcA5984780Bafc599bD69ADd087D56", "BUSD", 18, true, "BUSD", "3000000000000000000000"), // 3000 BUSD
            ("0x2170Ed0880ac9A755fd29B2688956BD959F933F8", "ETH", 18, false, "ETH", "1000000000000000000"), // 1 ETH
            ("0x7130d2A12B9BCbFAe4f2634d864A1Ee1Ce3Ead9c", "BTCB", 18, false, "BTC", "10000000000000000"), // 0.01 BTC
        ];

        let token_count = tokens.len();
        for (address, symbol, decimals, is_stable, price_symbol, optimal_input) in tokens {
            sqlx::query(
                r#"INSERT IGNORE INTO arbitrage_tokens
                   (chain_id, address, symbol, decimals, is_stable, price_symbol, optimal_input_amount)
                   VALUES (?, ?, ?, ?, ?, ?, ?)"#
            )
            .bind(chain_id)
            .bind(address)
            .bind(symbol)
            .bind(decimals)
            .bind(is_stable)
            .bind(price_symbol)
            .bind(optimal_input)
            .execute(&self.pool)
            .await?;
        }

        info!("[BSC] ✓ 初始化了 {} 个代币配置", token_count);
        Ok(())
    }

    /// 初始化 BSC 默认三角套利组合配置
    pub async fn init_bsc_default_triangles(&self) -> Result<()> {
        let chain_id: i64 = 56; // BSC Mainnet

        // 检查是否已有配置
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM arbitrage_triangles WHERE chain_id = ?")
            .bind(chain_id)
            .fetch_one(&self.pool)
            .await?;

        if count.0 > 0 {
            info!("[BSC] 三角套利组合配置已存在，跳过初始化");
            return Ok(());
        }

        info!("[BSC] 初始化默认三角套利组合配置...");

        // BSC 代币地址
        let wbnb = "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c";
        let usdt = "0x55d398326f99059fF775485246999027B3197955";
        let usdc = "0x8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d";
        let busd = "0xe9e7CEA3DedcA5984780Bafc599bD69ADd087D56";
        let eth = "0x2170Ed0880ac9A755fd29B2688956BD959F933F8";
        let btcb = "0x7130d2A12B9BCbFAe4f2634d864A1Ee1Ce3Ead9c";

        // BSC 三角套利组合
        let triangles = vec![
            // 稳定币三角
            ("USDT-USDC-BUSD", usdt, usdc, busd, 10, "stablecoin", "BSC稳定币三角，低滑点"),

            // WBNB-稳定币三角
            ("WBNB-USDT-USDC", wbnb, usdt, usdc, 20, "bnb_stable", "BNB与稳定币三角"),
            ("WBNB-USDT-BUSD", wbnb, usdt, busd, 30, "bnb_stable", "BNB-USDT-BUSD组合"),

            // ETH-稳定币三角
            ("ETH-USDT-USDC", eth, usdt, usdc, 40, "eth_stable", "BSC上的ETH套利"),

            // BTCB三角
            ("BTCB-USDT-USDC", btcb, usdt, usdc, 50, "btc_stable", "BTCB与稳定币"),
            ("BTCB-WBNB-USDT", btcb, wbnb, usdt, 60, "major", "BTCB-BNB-USDT主流币"),
        ];

        let triangle_count = triangles.len();
        for (name, token_a, token_b, token_c, priority, category, description) in triangles {
            sqlx::query(
                r#"INSERT IGNORE INTO arbitrage_triangles
                   (chain_id, name, token_a, token_b, token_c, priority, category, description)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#
            )
            .bind(chain_id)
            .bind(name)
            .bind(token_a)
            .bind(token_b)
            .bind(token_c)
            .bind(priority)
            .bind(category)
            .bind(description)
            .execute(&self.pool)
            .await?;
        }

        info!("[BSC] ✓ 初始化了 {} 个三角套利组合配置", triangle_count);
        Ok(())
    }

    /// 初始化 BSC 默认套利池子配置 (PancakeSwap V3)
    pub async fn init_bsc_default_pools(&self) -> Result<()> {
        let chain_id: i64 = 56; // BSC Mainnet

        // 检查是否已有配置
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM arbitrage_pools WHERE chain_id = ?")
            .bind(chain_id)
            .fetch_one(&self.pool)
            .await?;

        if count.0 > 0 {
            info!("[BSC] 套利池子配置已存在，跳过初始化");
            return Ok(());
        }

        info!("[BSC] 初始化默认套利池子配置...");

        // BSC 代币地址
        let wbnb = "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c";
        let usdt = "0x55d398326f99059fF775485246999027B3197955";
        let usdc = "0x8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d";
        let busd = "0xe9e7CEA3DedcA5984780Bafc599bD69ADd087D56";
        let eth = "0x2170Ed0880ac9A755fd29B2688956BD959F933F8";
        let btcb = "0x7130d2A12B9BCbFAe4f2634d864A1Ee1Ce3Ead9c";

        // PancakeSwap V3 池子 (需要根据实际情况更新地址)
        // (address, dex_type, token0, token0_symbol, token1, token1_symbol, fee)
        let pools = vec![
            // WBNB-USDT 池
            ("0x36696169C63e42cd08ce11f5deeBbCeBae652050", "pancakeswap_v3", wbnb, "WBNB", usdt, "USDT", 500),   // 0.05%
            ("0x172fcD41E0913e95784454622d1c3724f546f849", "pancakeswap_v3", wbnb, "WBNB", usdt, "USDT", 2500),  // 0.25%

            // WBNB-USDC 池
            ("0x7f51c8AaA6B0599aBd0565F3FFE38B61a4e7A1F0", "pancakeswap_v3", wbnb, "WBNB", usdc, "USDC", 500),

            // USDT-USDC 池
            ("0x92b7807bF19b7DDdf89b706143896d05228f3121", "pancakeswap_v3", usdt, "USDT", usdc, "USDC", 100),   // 0.01%

            // USDT-BUSD 池
            ("0x4f3126d5DE26413AbDCF6948943FB9D0847d9818", "pancakeswap_v3", usdt, "USDT", busd, "BUSD", 100),

            // ETH-USDT 池
            ("0x6e229C972d9F69c15Bdc7B07f385D2025225E72b", "pancakeswap_v3", eth, "ETH", usdt, "USDT", 500),

            // BTCB-USDT 池
            ("0x46Cf1cF8c69595804ba91dFdd8d6b960c9B0a7C4", "pancakeswap_v3", btcb, "BTCB", usdt, "USDT", 500),

            // BTCB-WBNB 池
            ("0xFC75f4E78bf71eD5066dB9ca771D4CcB7C1264E0", "pancakeswap_v3", btcb, "BTCB", wbnb, "WBNB", 500),
        ];

        let pool_count = pools.len();
        for (address, dex_type, token0, token0_symbol, token1, token1_symbol, fee) in pools {
            sqlx::query(
                r#"INSERT IGNORE INTO arbitrage_pools
                   (chain_id, address, dex_type, token0, token0_symbol, token1, token1_symbol, fee, enabled)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, TRUE)"#
            )
            .bind(chain_id)
            .bind(address)
            .bind(dex_type)
            .bind(token0)
            .bind(token0_symbol)
            .bind(token1)
            .bind(token1_symbol)
            .bind(fee)
            .execute(&self.pool)
            .await?;
        }

        info!("[BSC] ✓ 初始化了 {} 个套利池子配置", pool_count);
        Ok(())
    }

    /// 初始化 BSC 池子-路径映射配置
    pub async fn init_bsc_pool_paths(&self) -> Result<()> {
        let chain_id: i64 = 56; // BSC Mainnet

        // 检查是否已有配置
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM arbitrage_pool_paths WHERE chain_id = ?")
            .bind(chain_id)
            .fetch_one(&self.pool)
            .await?;

        if count.0 > 0 {
            info!("[BSC] 池子-路径映射配置已存在，跳过初始化");
            return Ok(());
        }

        info!("[BSC] 初始化池子-路径映射配置...");

        // BSC 代币地址
        let wbnb = "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c";
        let usdt = "0x55d398326f99059fF775485246999027B3197955";
        let usdc = "0x8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d";
        let busd = "0xe9e7CEA3DedcA5984780Bafc599bD69ADd087D56";
        let eth = "0x2170Ed0880ac9A755fd29B2688956BD959F933F8";
        let btcb = "0x7130d2A12B9BCbFAe4f2634d864A1Ee1Ce3Ead9c";

        // PancakeSwap V3 池子地址
        let wbnb_usdt_005 = "0x36696169C63e42cd08ce11f5deeBbCeBae652050";   // WBNB/USDT 0.05%
        let wbnb_usdt_025 = "0x172fcD41E0913e95784454622d1c3724f546f849";   // WBNB/USDT 0.25%
        let wbnb_usdc_005 = "0x7f51c8AaA6B0599aBd0565F3FFE38B61a4e7A1F0";   // WBNB/USDC 0.05%
        let usdt_usdc_001 = "0x92b7807bF19b7DDdf89b706143896d05228f3121";   // USDT/USDC 0.01%
        let usdt_busd_001 = "0x4f3126d5DE26413AbDCF6948943FB9D0847d9818";   // USDT/BUSD 0.01%
        let eth_usdt_005 = "0x6e229C972d9F69c15Bdc7B07f385D2025225E72b";    // ETH/USDT 0.05%
        let btcb_usdt_005 = "0x46Cf1cF8c69595804ba91dFdd8d6b960c9B0a7C4";   // BTCB/USDT 0.05%
        let btcb_wbnb_005 = "0xFC75f4E78bf71eD5066dB9ca771D4CcB7C1264E0";   // BTCB/WBNB 0.05%

        // 根据池子和三角套利组合定义的路径映射
        // (trigger_pool, path_name, triangle_name, token_a, token_b, token_c, priority)
        let pool_paths = vec![
            // ===== 池子 1: WBNB/USDT (0.05%) - 触发 3 条路径 =====
            (wbnb_usdt_005, "WBNB→USDT→USDC→WBNB", "WBNB-USDT-USDC", wbnb, usdt, usdc, 20),
            (wbnb_usdt_005, "WBNB→USDT→BUSD→WBNB", "WBNB-USDT-BUSD", wbnb, usdt, busd, 30),
            (wbnb_usdt_005, "USDT→WBNB→BTCB→USDT", "BTCB-WBNB-USDT", usdt, wbnb, btcb, 60),

            // ===== 池子 2: WBNB/USDT (0.25%) - 触发 3 条路径 =====
            (wbnb_usdt_025, "WBNB→USDT→USDC→WBNB", "WBNB-USDT-USDC", wbnb, usdt, usdc, 20),
            (wbnb_usdt_025, "WBNB→USDT→BUSD→WBNB", "WBNB-USDT-BUSD", wbnb, usdt, busd, 30),
            (wbnb_usdt_025, "USDT→WBNB→BTCB→USDT", "BTCB-WBNB-USDT", usdt, wbnb, btcb, 60),

            // ===== 池子 3: WBNB/USDC (0.05%) - 触发 2 条路径 =====
            (wbnb_usdc_005, "WBNB→USDC→USDT→WBNB", "WBNB-USDT-USDC", wbnb, usdc, usdt, 20),
            (wbnb_usdc_005, "USDC→WBNB→USDT→USDC", "WBNB-USDT-USDC", usdc, wbnb, usdt, 20),

            // ===== 池子 4: USDT/USDC (0.01%) - 触发 6 条路径 =====
            (usdt_usdc_001, "USDT→USDC→BUSD→USDT", "USDT-USDC-BUSD", usdt, usdc, busd, 10),
            (usdt_usdc_001, "USDC→USDT→BUSD→USDC", "USDT-USDC-BUSD", usdc, usdt, busd, 10),
            (usdt_usdc_001, "USDT→USDC→WBNB→USDT", "WBNB-USDT-USDC", usdt, usdc, wbnb, 20),
            (usdt_usdc_001, "USDC→USDT→WBNB→USDC", "WBNB-USDT-USDC", usdc, usdt, wbnb, 20),
            (usdt_usdc_001, "USDT→USDC→ETH→USDT", "ETH-USDT-USDC", usdt, usdc, eth, 40),
            (usdt_usdc_001, "USDT→USDC→BTCB→USDT", "BTCB-USDT-USDC", usdt, usdc, btcb, 50),

            // ===== 池子 5: USDT/BUSD (0.01%) - 触发 4 条路径 =====
            (usdt_busd_001, "USDT→BUSD→USDC→USDT", "USDT-USDC-BUSD", usdt, busd, usdc, 10),
            (usdt_busd_001, "BUSD→USDT→USDC→BUSD", "USDT-USDC-BUSD", busd, usdt, usdc, 10),
            (usdt_busd_001, "USDT→BUSD→WBNB→USDT", "WBNB-USDT-BUSD", usdt, busd, wbnb, 30),
            (usdt_busd_001, "BUSD→USDT→WBNB→BUSD", "WBNB-USDT-BUSD", busd, usdt, wbnb, 30),

            // ===== 池子 6: ETH/USDT (0.05%) - 触发 2 条路径 =====
            (eth_usdt_005, "ETH→USDT→USDC→ETH", "ETH-USDT-USDC", eth, usdt, usdc, 40),
            (eth_usdt_005, "USDT→ETH→USDC→USDT", "ETH-USDT-USDC", usdt, eth, usdc, 40),

            // ===== 池子 7: BTCB/USDT (0.05%) - 触发 4 条路径 =====
            (btcb_usdt_005, "BTCB→USDT→USDC→BTCB", "BTCB-USDT-USDC", btcb, usdt, usdc, 50),
            (btcb_usdt_005, "USDT→BTCB→WBNB→USDT", "BTCB-WBNB-USDT", usdt, btcb, wbnb, 60),
            (btcb_usdt_005, "BTCB→USDT→WBNB→BTCB", "BTCB-WBNB-USDT", btcb, usdt, wbnb, 60),
            (btcb_usdt_005, "USDT→BTCB→USDC→USDT", "BTCB-USDT-USDC", usdt, btcb, usdc, 50),

            // ===== 池子 8: BTCB/WBNB (0.05%) - 触发 4 条路径 =====
            (btcb_wbnb_005, "BTCB→WBNB→USDT→BTCB", "BTCB-WBNB-USDT", btcb, wbnb, usdt, 60),
            (btcb_wbnb_005, "WBNB→BTCB→USDT→WBNB", "BTCB-WBNB-USDT", wbnb, btcb, usdt, 60),
            (btcb_wbnb_005, "BTCB→WBNB→USDC→BTCB", "BTCB-USDT-USDC", btcb, wbnb, usdc, 50),
            (btcb_wbnb_005, "WBNB→BTCB→USDC→WBNB", "BTCB-USDT-USDC", wbnb, btcb, usdc, 50),
        ];

        let path_count = pool_paths.len();
        for (trigger_pool, path_name, triangle_name, token_a, token_b, token_c, priority) in pool_paths {
            sqlx::query(
                r#"INSERT IGNORE INTO arbitrage_pool_paths
                   (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, priority)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#
            )
            .bind(chain_id)
            .bind(trigger_pool)
            .bind(path_name)
            .bind(triangle_name)
            .bind(token_a)
            .bind(token_b)
            .bind(token_c)
            .bind(priority)
            .execute(&self.pool)
            .await?;
        }

        info!("[BSC] ✓ 初始化了 {} 条池子-路径映射配置", path_count);
        Ok(())
    }

    /// 初始化默认三角套利组合配置 (Ethereum Mainnet)
    pub async fn init_default_triangles(&self) -> Result<()> {
        let chain_id: i64 = 1; // Ethereum Mainnet

        // 检查是否已有配置
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM arbitrage_triangles WHERE chain_id = ?")
            .bind(chain_id)
            .fetch_one(&self.pool)
            .await?;

        if count.0 > 0 {
            info!("三角套利组合配置已存在，跳过初始化");
            return Ok(());
        }

        info!("初始化默认三角套利组合配置...");

        // 代币地址
        let dai = "0x6B175474E89094C44Da98b954EedeAC495271d0F";
        let usdc = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
        let usdt = "0xdAC17F958D2ee523a2206206994597C13D831ec7";
        let weth = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";
        let wbtc = "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599";

        // 6个有效的三角套利组合
        let triangles = vec![
            // (name, token_a, token_b, token_c, priority, category, description)
            // 稳定币三角 - 最高优先级
            ("DAI-USDC-USDT", dai, usdc, usdt, 10, "stablecoin", "稳定币三角，手续费最低(0.01%)，滑点极低，练功房+主阵地"),

            // WETH-稳定币三角 - 核心
            ("USDC-WETH-USDT", usdc, weth, usdt, 20, "eth_stable", "WETH-USDT高波动边，USDC-USDT锚定边，大ETH成交触发"),
            ("DAI-USDC-WETH", dai, usdc, weth, 30, "eth_stable", "稳定币+ETH，fee极低，适合event-driven"),

            // WBTC-稳定币三角 - BTC主流
            ("WBTC-USDC-USDT", wbtc, usdc, usdt, 40, "btc_stable", "BTC大资金成交触发，单次利润可能更高，频率低于ETH"),

            // WBTC-WETH三角 - 核心主流币
            ("WBTC-WETH-USDC", wbtc, weth, usdc, 50, "major", "BTC-ETH汇率错位，CEX-DEX同步延迟，职业套利常规路径"),
            ("WBTC-WETH-USDT", wbtc, weth, usdt, 60, "major", "波动性最大，Gas与fee更敏感，通常只在剧烈行情出现"),
        ];

        let triangle_count = triangles.len();
        for (name, token_a, token_b, token_c, priority, category, description) in triangles {
            sqlx::query(
                r#"INSERT IGNORE INTO arbitrage_triangles
                   (chain_id, name, token_a, token_b, token_c, priority, category, description)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#
            )
            .bind(chain_id)
            .bind(name)
            .bind(token_a)
            .bind(token_b)
            .bind(token_c)
            .bind(priority)
            .bind(category)
            .bind(description)
            .execute(&self.pool)
            .await?;
        }

        info!("✓ 初始化了 {} 个三角套利组合配置", triangle_count);
        Ok(())
    }

    /// 初始化默认套利池子配置 (Ethereum Mainnet)
    /// 只包含6个三角组合涉及的池子
    pub async fn init_default_pools(&self) -> Result<()> {
        let chain_id: i64 = 1; // Ethereum Mainnet

        // 检查是否已有配置
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM arbitrage_pools WHERE chain_id = ?")
            .bind(chain_id)
            .fetch_one(&self.pool)
            .await?;

        if count.0 > 0 {
            info!("套利池子配置已存在，跳过初始化");
            return Ok(());
        }

        info!("初始化默认套利池子配置...");

        // 6个三角组合涉及的所有池子
        // (address, dex_type, token0, token0_symbol, token1, token1_symbol, fee)
        let pools = vec![
            // ===== 稳定币相关池 =====
            // DAI-USDC (0.01%)
            ("0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168", "uniswap_v3",
             "0x6B175474E89094C44Da98b954EedeAC495271d0F", "DAI",
             "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", "USDC", 100),
            // DAI-USDC (0.05%) - 备用
            ("0x6c6Bc977E13Df9b0de53b251522280BB72383700", "uniswap_v3",
             "0x6B175474E89094C44Da98b954EedeAC495271d0F", "DAI",
             "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", "USDC", 500),
            // DAI-USDT (0.01%)
            ("0x6f48ECa74B38d2936B02ab603FF4e36A6C0E3A77", "uniswap_v3",
             "0x6B175474E89094C44Da98b954EedeAC495271d0F", "DAI",
             "0xdAC17F958D2ee523a2206206994597C13D831ec7", "USDT", 100),
            // USDC-USDT (0.01%)
            ("0x3416cF6C708Da44DB2624D63ea0AAef7113527C6", "uniswap_v3",
             "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", "USDC",
             "0xdAC17F958D2ee523a2206206994597C13D831ec7", "USDT", 100),

            // ===== WETH-稳定币池 =====
            // WETH-USDC (0.05%) - 主力池
            ("0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640", "uniswap_v3",
             "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", "USDC",
             "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", "WETH", 500),
            // WETH-USDC (0.30%) - 备用
            ("0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8", "uniswap_v3",
             "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", "USDC",
             "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", "WETH", 3000),
            // WETH-USDT (0.05%)
            ("0x11b815efB8f581194ae79006d24E0d814B7697F6", "uniswap_v3",
             "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", "WETH",
             "0xdAC17F958D2ee523a2206206994597C13D831ec7", "USDT", 500),
            // WETH-USDT (0.30%) - 备用
            ("0x4e68Ccd3E89f51C3074ca5072bBaC773960dFa36", "uniswap_v3",
             "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", "WETH",
             "0xdAC17F958D2ee523a2206206994597C13D831ec7", "USDT", 3000),
            // DAI-WETH (0.05%)
            ("0x60594a405d53811d3BC4766596EFD80fd545A270", "uniswap_v3",
             "0x6B175474E89094C44Da98b954EedeAC495271d0F", "DAI",
             "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", "WETH", 500),
            // DAI-WETH (0.30%) - 备用
            ("0xC2e9F25Be6257c210d7Adf0D4Cd6E3E881ba25f8", "uniswap_v3",
             "0x6B175474E89094C44Da98b954EedeAC495271d0F", "DAI",
             "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", "WETH", 3000),

            // ===== WBTC相关池 =====
            // WBTC-USDC (0.30%)
            ("0x99ac8cA7087fA4A2A1FB6357269965A2014ABc35", "uniswap_v3",
             "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599", "WBTC",
             "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", "USDC", 3000),
            // WBTC-USDT (0.30%)
            ("0x9Db9e0e53058C89e5B94e29621a205198648425B", "uniswap_v3",
             "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599", "WBTC",
             "0xdAC17F958D2ee523a2206206994597C13D831ec7", "USDT", 3000),
            // WBTC-WETH (0.05%) - 主力池
            ("0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0", "uniswap_v3",
             "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599", "WBTC",
             "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", "WETH", 500),
            // WBTC-WETH (0.30%) - 备用
            ("0xCBCdF9626bC03E24f779434178A73a0B4bad62eD", "uniswap_v3",
             "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599", "WBTC",
             "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", "WETH", 3000),
        ];

        let pool_count = pools.len();
        for (address, dex_type, token0, token0_symbol, token1, token1_symbol, fee) in pools {
            sqlx::query(
                r#"INSERT IGNORE INTO arbitrage_pools
                   (chain_id, address, dex_type, token0, token0_symbol, token1, token1_symbol, fee)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#
            )
            .bind(chain_id)
            .bind(address)
            .bind(dex_type)
            .bind(token0)
            .bind(token0_symbol)
            .bind(token1)
            .bind(token1_symbol)
            .bind(fee)
            .execute(&self.pool)
            .await?;
        }

        info!("✓ 初始化了 {} 个套利池子配置", pool_count);
        Ok(())
    }

    /// 获取所有启用的套利池子
    pub async fn get_enabled_pools(&self, chain_id: u64) -> Result<Vec<ArbitragePoolConfig>> {
        let pools = sqlx::query_as::<_, ArbitragePoolConfig>(
            "SELECT id, chain_id, address, dex_type, token0, token0_symbol, token1, token1_symbol, fee, enabled
             FROM arbitrage_pools WHERE chain_id = ? AND enabled = TRUE"
        )
        .bind(chain_id as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(pools)
    }

    /// 初始化池子-路径映射配置 (根据文档 triangular_arbitrage_paths.md)
    pub async fn init_pool_paths(&self) -> Result<()> {
        let chain_id: i64 = 1; // Ethereum Mainnet

        // 检查是否已有配置
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM arbitrage_pool_paths WHERE chain_id = ?")
            .bind(chain_id)
            .fetch_one(&self.pool)
            .await?;

        if count.0 > 0 {
            info!("池子-路径映射配置已存在，跳过初始化");
            return Ok(());
        }

        info!("初始化池子-路径映射配置...");

        // 代币地址
        let dai = "0x6B175474E89094C44Da98b954EedeAC495271d0F";
        let usdc = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
        let usdt = "0xdAC17F958D2ee523a2206206994597C13D831ec7";
        let weth = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";
        let wbtc = "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599";

        // 池子地址
        let dai_usdc_001 = "0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168";   // DAI/USDC 0.01%
        let dai_usdc_005 = "0x6c6Bc977E13Df9b0de53b251522280BB72383700";   // DAI/USDC 0.05%
        let dai_usdt_001 = "0x6f48ECa74B38d2936B02ab603FF4e36A6C0E3A77";   // DAI/USDT 0.01%
        let usdc_usdt_001 = "0x3416cF6C708Da44DB2624D63ea0AAef7113527C6";  // USDC/USDT 0.01%
        let usdc_weth_005 = "0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640";  // USDC/WETH 0.05%
        let usdc_weth_030 = "0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8";  // USDC/WETH 0.30%
        let weth_usdt_005 = "0x11b815efB8f581194ae79006d24E0d814B7697F6";  // WETH/USDT 0.05%
        let weth_usdt_030 = "0x4e68Ccd3E89f51C3074ca5072bBaC773960dFa36";  // WETH/USDT 0.30%
        let dai_weth_005 = "0x60594a405d53811d3BC4766596EFD80fd545A270";   // DAI/WETH 0.05%
        let dai_weth_030 = "0xC2e9F25Be6257c210d7Adf0D4Cd6E3E881ba25f8";   // DAI/WETH 0.30%
        let wbtc_usdc_030 = "0x99ac8cA7087fA4A2A1FB6357269965A2014ABc35";  // WBTC/USDC 0.30%
        let wbtc_usdt_030 = "0x9Db9e0e53058C89e5B94e29621a205198648425B";  // WBTC/USDT 0.30%
        let wbtc_weth_005 = "0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0";  // WBTC/WETH 0.05%
        let wbtc_weth_030 = "0xCBCdF9626bC03E24f779434178A73a0B4bad62eD";  // WBTC/WETH 0.30%

        // 根据文档定义的 14 个池子各自触发的路径
        // (trigger_pool, path_name, triangle_name, token_a, token_b, token_c, priority)
        let pool_paths = vec![
            // ===== 池子 1: DAI/USDC (0.01%) - 触发 2 条路径 =====
            (dai_usdc_001, "DAI→USDC→USDT→DAI", "DAI-USDC-USDT", dai, usdc, usdt, 10),
            (dai_usdc_001, "DAI→USDC→WETH→DAI", "DAI-USDC-WETH", dai, usdc, weth, 30),

            // ===== 池子 2: DAI/USDC (0.05%) - 触发 2 条路径 =====
            (dai_usdc_005, "DAI→USDC→USDT→DAI", "DAI-USDC-USDT", dai, usdc, usdt, 10),
            (dai_usdc_005, "DAI→USDC→WETH→DAI", "DAI-USDC-WETH", dai, usdc, weth, 30),

            // ===== 池子 3: DAI/USDT (0.01%) - 触发 1 条路径 =====
            (dai_usdt_001, "DAI→USDT→USDC→DAI", "DAI-USDC-USDT", dai, usdt, usdc, 10),

            // ===== 池子 4: USDC/USDT (0.01%) - 触发 3 条路径 =====
            (usdc_usdt_001, "USDC→USDT→DAI→USDC", "DAI-USDC-USDT", usdc, usdt, dai, 10),
            (usdc_usdt_001, "USDC→USDT→WETH→USDC", "USDC-WETH-USDT", usdc, usdt, weth, 20),
            (usdc_usdt_001, "USDC→USDT→WBTC→USDC", "WBTC-USDC-USDT", usdc, usdt, wbtc, 40),

            // ===== 池子 5: USDC/WETH (0.05%) - 触发 4 条路径 =====
            (usdc_weth_005, "USDC→WETH→USDT→USDC", "USDC-WETH-USDT", usdc, weth, usdt, 20),
            (usdc_weth_005, "USDC→WETH→DAI→USDC", "DAI-USDC-WETH", usdc, weth, dai, 30),
            (usdc_weth_005, "USDC→WETH→WBTC→USDC", "WBTC-WETH-USDC", usdc, weth, wbtc, 50),
            (usdc_weth_005, "WETH→USDC→USDT→WETH", "USDC-WETH-USDT", weth, usdc, usdt, 20),

            // ===== 池子 6: USDC/WETH (0.30%) - 触发 4 条路径 =====
            (usdc_weth_030, "USDC→WETH→USDT→USDC", "USDC-WETH-USDT", usdc, weth, usdt, 20),
            (usdc_weth_030, "USDC→WETH→DAI→USDC", "DAI-USDC-WETH", usdc, weth, dai, 30),
            (usdc_weth_030, "USDC→WETH→WBTC→USDC", "WBTC-WETH-USDC", usdc, weth, wbtc, 50),
            (usdc_weth_030, "WETH→USDC→USDT→WETH", "USDC-WETH-USDT", weth, usdc, usdt, 20),

            // ===== 池子 7: WETH/USDT (0.05%) - 触发 3 条路径 =====
            (weth_usdt_005, "WETH→USDT→USDC→WETH", "USDC-WETH-USDT", weth, usdt, usdc, 20),
            (weth_usdt_005, "WETH→USDT→WBTC→WETH", "WBTC-WETH-USDT", weth, usdt, wbtc, 60),
            (weth_usdt_005, "USDT→WETH→USDC→USDT", "USDC-WETH-USDT", usdt, weth, usdc, 20),

            // ===== 池子 8: WETH/USDT (0.30%) - 触发 3 条路径 =====
            (weth_usdt_030, "WETH→USDT→USDC→WETH", "USDC-WETH-USDT", weth, usdt, usdc, 20),
            (weth_usdt_030, "WETH→USDT→WBTC→WETH", "WBTC-WETH-USDT", weth, usdt, wbtc, 60),
            (weth_usdt_030, "USDT→WETH→USDC→USDT", "USDC-WETH-USDT", usdt, weth, usdc, 20),

            // ===== 池子 9: DAI/WETH (0.05%) - 触发 2 条路径 =====
            (dai_weth_005, "DAI→WETH→USDC→DAI", "DAI-USDC-WETH", dai, weth, usdc, 30),
            (dai_weth_005, "WETH→DAI→USDC→WETH", "DAI-USDC-WETH", weth, dai, usdc, 30),

            // ===== 池子 10: DAI/WETH (0.30%) - 触发 2 条路径 =====
            (dai_weth_030, "DAI→WETH→USDC→DAI", "DAI-USDC-WETH", dai, weth, usdc, 30),
            (dai_weth_030, "WETH→DAI→USDC→WETH", "DAI-USDC-WETH", weth, dai, usdc, 30),

            // ===== 池子 11: WBTC/USDC (0.30%) - 触发 2 条路径 =====
            (wbtc_usdc_030, "WBTC→USDC→USDT→WBTC", "WBTC-USDC-USDT", wbtc, usdc, usdt, 40),
            (wbtc_usdc_030, "WBTC→USDC→WETH→WBTC", "WBTC-WETH-USDC", wbtc, usdc, weth, 50),

            // ===== 池子 12: WBTC/USDT (0.30%) - 触发 2 条路径 =====
            (wbtc_usdt_030, "WBTC→USDT→USDC→WBTC", "WBTC-USDC-USDT", wbtc, usdt, usdc, 40),
            (wbtc_usdt_030, "WBTC→USDT→WETH→WBTC", "WBTC-WETH-USDT", wbtc, usdt, weth, 60),

            // ===== 池子 13: WBTC/WETH (0.05%) - 触发 4 条路径 =====
            (wbtc_weth_005, "WBTC→WETH→USDC→WBTC", "WBTC-WETH-USDC", wbtc, weth, usdc, 50),
            (wbtc_weth_005, "WBTC→WETH→USDT→WBTC", "WBTC-WETH-USDT", wbtc, weth, usdt, 60),
            (wbtc_weth_005, "WETH→WBTC→USDC→WETH", "WBTC-WETH-USDC", weth, wbtc, usdc, 50),
            (wbtc_weth_005, "WETH→WBTC→USDT→WETH", "WBTC-WETH-USDT", weth, wbtc, usdt, 60),

            // ===== 池子 14: WBTC/WETH (0.30%) - 触发 4 条路径 =====
            (wbtc_weth_030, "WBTC→WETH→USDC→WBTC", "WBTC-WETH-USDC", wbtc, weth, usdc, 50),
            (wbtc_weth_030, "WBTC→WETH→USDT→WBTC", "WBTC-WETH-USDT", wbtc, weth, usdt, 60),
            (wbtc_weth_030, "WETH→WBTC→USDC→WETH", "WBTC-WETH-USDC", weth, wbtc, usdc, 50),
            (wbtc_weth_030, "WETH→WBTC→USDT→WETH", "WBTC-WETH-USDT", weth, wbtc, usdt, 60),
        ];

        let path_count = pool_paths.len();
        for (trigger_pool, path_name, triangle_name, token_a, token_b, token_c, priority) in pool_paths {
            sqlx::query(
                r#"INSERT IGNORE INTO arbitrage_pool_paths
                   (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, priority)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#
            )
            .bind(chain_id)
            .bind(trigger_pool)
            .bind(path_name)
            .bind(triangle_name)
            .bind(token_a)
            .bind(token_b)
            .bind(token_c)
            .bind(priority)
            .execute(&self.pool)
            .await?;
        }

        info!("✓ 初始化了 {} 条池子-路径映射配置", path_count);
        Ok(())
    }

    /// 获取指定池子触发时应检查的所有路径
    pub async fn get_paths_by_trigger_pool(&self, chain_id: u64, trigger_pool: &str) -> Result<Vec<ArbitragePoolPathConfig>> {
        let paths = sqlx::query_as::<_, ArbitragePoolPathConfig>(
            "SELECT id, chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, priority, enabled
             FROM arbitrage_pool_paths
             WHERE chain_id = ? AND LOWER(trigger_pool) = LOWER(?) AND enabled = TRUE
             ORDER BY priority"
        )
        .bind(chain_id as i64)
        .bind(trigger_pool)
        .fetch_all(&self.pool)
        .await?;

        Ok(paths)
    }

    /// 获取所有启用的池子-路径映射
    pub async fn get_all_pool_paths(&self, chain_id: u64) -> Result<Vec<ArbitragePoolPathConfig>> {
        let paths = sqlx::query_as::<_, ArbitragePoolPathConfig>(
            "SELECT id, chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, priority, enabled
             FROM arbitrage_pool_paths
             WHERE chain_id = ? AND enabled = TRUE
             ORDER BY trigger_pool, priority"
        )
        .bind(chain_id as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(paths)
    }
}

/// 套利池子配置
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ArbitragePoolConfig {
    pub id: i64,
    pub chain_id: i64,
    pub address: String,
    pub dex_type: String,
    pub token0: String,
    pub token0_symbol: String,
    pub token1: String,
    pub token1_symbol: String,
    pub fee: i32,
    pub enabled: bool,
}

/// 池子-路径映射配置
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ArbitragePoolPathConfig {
    pub id: i64,
    pub chain_id: i64,
    pub trigger_pool: String,
    pub path_name: String,
    pub triangle_name: String,
    pub token_a: String,
    pub token_b: String,
    pub token_c: String,
    pub priority: i32,
    pub enabled: bool,
}
