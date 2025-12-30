use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub database: DatabaseConfig,
    pub ethereum: ChainConfig,
    pub bsc: ChainConfig,
    /// 所有启用的链配置 (chain_id -> ChainConfig)
    pub chains: HashMap<u64, ChainConfig>,
    /// 启用的链列表
    pub enabled_chains: Vec<u64>,
    pub arbitrage: ArbitrageConfig,
    pub flash_loan: FlashLoanConfig,
    pub mev: MevConfig,
    pub wallet: WalletConfig,
    pub api: ApiConfig,
    pub log: LogConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
}

/// 支持的区块链枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum SupportedChain {
    Ethereum = 1,
    Bsc = 56,
    Polygon = 137,
    Arbitrum = 42161,
    Base = 8453,
    Optimism = 10,
    Avalanche = 43114,
}

impl SupportedChain {
    pub fn from_chain_id(chain_id: u64) -> Option<Self> {
        match chain_id {
            1 => Some(SupportedChain::Ethereum),
            56 => Some(SupportedChain::Bsc),
            137 => Some(SupportedChain::Polygon),
            42161 => Some(SupportedChain::Arbitrum),
            8453 => Some(SupportedChain::Base),
            10 => Some(SupportedChain::Optimism),
            43114 => Some(SupportedChain::Avalanche),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            SupportedChain::Ethereum => "Ethereum",
            SupportedChain::Bsc => "BSC",
            SupportedChain::Polygon => "Polygon",
            SupportedChain::Arbitrum => "Arbitrum",
            SupportedChain::Base => "Base",
            SupportedChain::Optimism => "Optimism",
            SupportedChain::Avalanche => "Avalanche",
        }
    }

    pub fn native_token(&self) -> &'static str {
        match self {
            SupportedChain::Ethereum => "ETH",
            SupportedChain::Bsc => "BNB",
            SupportedChain::Polygon => "MATIC",
            SupportedChain::Arbitrum => "ETH",
            SupportedChain::Base => "ETH",
            SupportedChain::Optimism => "ETH",
            SupportedChain::Avalanche => "AVAX",
        }
    }
}

/// 链上合约地址配置
#[derive(Debug, Clone, Deserialize)]
pub struct ChainContracts {
    /// Uniswap V3 / PancakeSwap V3 QuoterV2 合约地址
    pub quoter_v2: String,
    /// Multicall3 合约地址 (大多数链都是相同的)
    pub multicall3: String,
    /// Wrapped Native Token 地址 (WETH/WBNB/WMATIC 等)
    pub wrapped_native: String,
    /// 主要 DEX Router 地址
    pub swap_router: Option<String>,
    /// 闪电贷合约地址 (可选)
    pub flash_loan_pool: Option<String>,
}

impl ChainContracts {
    /// 获取以太坊主网合约地址
    pub fn ethereum() -> Self {
        Self {
            quoter_v2: "0x61fFE014bA17989E743c5F6cB21bF9697530B21e".to_string(), // Uniswap V3 QuoterV2
            multicall3: "0xcA11bde05977b3631167028862bE2a173976CA11".to_string(),
            wrapped_native: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string(), // WETH
            swap_router: Some("0xE592427A0AEce92De3Edee1F18E0157C05861564".to_string()), // Uniswap V3 Router
            flash_loan_pool: None,
        }
    }

    /// 获取 BSC 主网合约地址
    pub fn bsc() -> Self {
        Self {
            quoter_v2: "0xB048Bbc1Ee6b733FFfCFb9e9CeF7375518e25997".to_string(), // PancakeSwap V3 QuoterV2
            multicall3: "0xcA11bde05977b3631167028862bE2a173976CA11".to_string(),
            wrapped_native: "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c".to_string(), // WBNB
            swap_router: Some("0x1b81D678ffb9C0263b24A97847620C99d213eB14".to_string()), // PancakeSwap V3 Router
            flash_loan_pool: None,
        }
    }

    /// 获取 Polygon 主网合约地址
    pub fn polygon() -> Self {
        Self {
            quoter_v2: "0x61fFE014bA17989E743c5F6cB21bF9697530B21e".to_string(), // Uniswap V3 QuoterV2
            multicall3: "0xcA11bde05977b3631167028862bE2a173976CA11".to_string(),
            wrapped_native: "0x0d500B1d8E8eF31E21C99d1Db9A6444d3ADf1270".to_string(), // WMATIC
            swap_router: Some("0xE592427A0AEce92De3Edee1F18E0157C05861564".to_string()),
            flash_loan_pool: None,
        }
    }

    /// 获取 Arbitrum 主网合约地址
    pub fn arbitrum() -> Self {
        Self {
            quoter_v2: "0x61fFE014bA17989E743c5F6cB21bF9697530B21e".to_string(), // Uniswap V3 QuoterV2
            multicall3: "0xcA11bde05977b3631167028862bE2a173976CA11".to_string(),
            wrapped_native: "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1".to_string(), // WETH
            swap_router: Some("0xE592427A0AEce92De3Edee1F18E0157C05861564".to_string()),
            flash_loan_pool: None,
        }
    }

    /// 获取 Base 主网合约地址
    pub fn base() -> Self {
        Self {
            quoter_v2: "0x3d4e44Eb1374240CE5F1B871ab261CD16335B76a".to_string(), // Uniswap V3 QuoterV2 on Base
            multicall3: "0xcA11bde05977b3631167028862bE2a173976CA11".to_string(),
            wrapped_native: "0x4200000000000000000000000000000000000006".to_string(), // WETH on Base
            swap_router: Some("0x2626664c2603336E57B271c5C0b26F421741e481".to_string()), // Uniswap V3 Router on Base
            flash_loan_pool: None,
        }
    }

    /// 根据 chain_id 获取合约地址
    pub fn for_chain(chain_id: u64) -> Option<Self> {
        match chain_id {
            1 => Some(Self::ethereum()),
            56 => Some(Self::bsc()),
            137 => Some(Self::polygon()),
            42161 => Some(Self::arbitrum()),
            8453 => Some(Self::base()),
            _ => None,
        }
    }
}

/// 通用链配置 (替代之前的 EthereumConfig 和 BscConfig)
#[derive(Debug, Clone, Deserialize)]
pub struct ChainConfig {
    pub chain_id: u64,
    pub name: String,
    pub rpc_url: String,
    pub ws_url: String,
    /// 是否启用该链
    pub enabled: bool,
    /// 链上合约地址
    pub contracts: ChainContracts,
    /// 原生代币符号 (ETH/BNB/MATIC 等)
    pub native_token: String,
    /// 区块时间 (秒)
    pub block_time_secs: u64,
    /// 该链的套利合约地址 (可选，覆盖全局配置)
    pub arbitrage_contract: Option<String>,
}

impl ChainConfig {
    /// 创建以太坊主网配置
    pub fn ethereum(rpc_url: String, ws_url: String) -> Self {
        Self {
            chain_id: 1,
            name: "Ethereum".to_string(),
            rpc_url,
            ws_url,
            enabled: true,
            contracts: ChainContracts::ethereum(),
            native_token: "ETH".to_string(),
            block_time_secs: 12,
            arbitrage_contract: None,
        }
    }

    /// 创建 BSC 主网配置
    pub fn bsc(rpc_url: String, ws_url: String) -> Self {
        Self {
            chain_id: 56,
            name: "BSC".to_string(),
            rpc_url,
            ws_url,
            enabled: true,
            contracts: ChainContracts::bsc(),
            native_token: "BNB".to_string(),
            block_time_secs: 3,
            arbitrage_contract: None,
        }
    }

    /// 创建 Polygon 主网配置
    pub fn polygon(rpc_url: String, ws_url: String) -> Self {
        Self {
            chain_id: 137,
            name: "Polygon".to_string(),
            rpc_url,
            ws_url,
            enabled: true,
            contracts: ChainContracts::polygon(),
            native_token: "MATIC".to_string(),
            block_time_secs: 2,
            arbitrage_contract: None,
        }
    }

    /// 创建 Arbitrum 主网配置
    pub fn arbitrum(rpc_url: String, ws_url: String) -> Self {
        Self {
            chain_id: 42161,
            name: "Arbitrum".to_string(),
            rpc_url,
            ws_url,
            enabled: true,
            contracts: ChainContracts::arbitrum(),
            native_token: "ETH".to_string(),
            block_time_secs: 1, // Arbitrum 出块很快
            arbitrage_contract: None,
        }
    }

    /// 创建 Base 主网配置
    pub fn base(rpc_url: String, ws_url: String) -> Self {
        Self {
            chain_id: 8453,
            name: "Base".to_string(),
            rpc_url,
            ws_url,
            enabled: true,
            contracts: ChainContracts::base(),
            native_token: "ETH".to_string(),
            block_time_secs: 2,
            arbitrage_contract: None,
        }
    }
}

// 保持向后兼容的类型别名
pub type EthereumConfig = ChainConfig;
pub type BscConfig = ChainConfig;

#[derive(Debug, Clone, Deserialize)]
pub struct ArbitrageConfig {
    pub max_slippage: f64,           // 最大滑点 (如 0.0005 = 0.05%)
    pub min_profit_threshold: f64,   // 最低利润阈值 (USD)
    pub max_path_hops: u32,          // 最大路径跳数
    pub gas_price_multiplier: f64,   // Gas 价格倍数
    pub max_gas_price_gwei: Option<f64>, // 最大 Gas 价格 (Gwei) - 支持小数，如 0.08
    pub dry_run: Option<bool>,       // 是否干运行模式
    pub auto_execute: Option<bool>,  // 是否自动执行套利
    pub min_swap_value_usd: f64,     // 最小交易金额过滤阈值 (USD)
    pub skip_local_calc_threshold_usd: f64, // 超过该阈值跳过本地计算直接链上计算 (USD)，默认 5000
    // 动态利润门槛配置 (根据 Gas 价格调整最小利润要求)
    pub min_profit_ultra_low_gas: f64,  // Gas < 1 Gwei 时的最小利润 (USD)
    pub min_profit_low_gas: f64,        // Gas 1-5 Gwei 时的最小利润 (USD)
    pub min_profit_normal_gas: f64,     // Gas 5-20 Gwei 时的最小利润 (USD)
    pub min_profit_high_gas: f64,       // Gas 20-50 Gwei 时的最小利润 (USD)
    pub min_profit_very_high_gas: f64,  // Gas >= 50 Gwei 时的最小利润 (USD)
}

#[derive(Debug, Clone, Deserialize)]
pub struct FlashLoanConfig {
    pub provider: FlashLoanProvider,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub enum FlashLoanProvider {
    UniswapV3,
    UniswapV4,
    Aave,
    Balancer,
}

impl Default for FlashLoanProvider {
    fn default() -> Self {
        FlashLoanProvider::UniswapV3
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MevConfig {
    /// 是否使用 Flashbots 发送交易（防止 MEV 攻击）
    pub use_flashbots: bool,
    /// Flashbots 中继 URL（默认自动选择）
    pub flashbots_rpc: Option<String>,
    /// 是否同时使用公开 mempool（Both 模式）
    /// 当 use_flashbots=true 且 use_public_mempool=true 时，同时发送到两个渠道
    pub use_public_mempool: bool,
    /// 优先费（Gwei）- 支持小数，如 0.005
    pub priority_fee_gwei: Option<f64>,
    /// Flashbots Bundle 签名私钥（可选，默认使用交易私钥）
    pub flashbots_signer_key: Option<String>,
    /// 最大重试区块数
    pub max_block_retries: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WalletConfig {
    pub private_key: Option<String>,
    pub arbitrage_contract_address: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LogConfig {
    pub level: String,
    pub file_path: String,
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        // 加载 .env 文件
        dotenv::dotenv().ok();

        // 数据库配置
        let db_host = env::var("DB_HOST").context("DB_HOST not set")?;
        let db_port = env::var("DB_PORT").context("DB_PORT not set")?;
        let db_user = env::var("DB_USER").context("DB_USER not set")?;
        let db_password = env::var("DB_PASSWORD").context("DB_PASSWORD not set")?;
        let db_name = env::var("DB_NAME").context("DB_NAME not set")?;
        let db_max_connections = env::var("DB_MAX_CONNECTIONS")
            .unwrap_or_else(|_| "10".to_string())
            .parse()
            .context("Invalid DB_MAX_CONNECTIONS")?;

        // URL encode username and password to handle special characters
        let encoded_user = urlencoding::encode(&db_user);
        let encoded_password = urlencoding::encode(&db_password);

        let database_url = format!(
            "mysql://{}:{}@{}:{}/{}",
            encoded_user, encoded_password, db_host, db_port, db_name
        );

        // 解析启用的链列表 (逗号分隔, 例如: "1,56,137")
        let enabled_chains: Vec<u64> = env::var("ENABLED_CHAINS")
            .unwrap_or_else(|_| "1".to_string()) // 默认只启用以太坊
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();

        // 以太坊配置
        let eth_rpc = env::var("ETH_RPC_URL")
            .unwrap_or_else(|_| "https://eth-mainnet.g.alchemy.com/v2/demo".to_string());
        let eth_ws = env::var("ETH_WS_URL")
            .unwrap_or_else(|_| "wss://eth-mainnet.g.alchemy.com/v2/demo".to_string());
        let mut ethereum = ChainConfig::ethereum(eth_rpc, eth_ws);
        ethereum.enabled = enabled_chains.contains(&1);
        ethereum.arbitrage_contract = env::var("ETH_ARBITRAGE_CONTRACT").ok().filter(|s| !s.is_empty());

        // BSC 配置
        let bsc_rpc = env::var("BSC_RPC_URL")
            .unwrap_or_else(|_| "https://bsc-dataseed1.binance.org".to_string());
        let bsc_ws = env::var("BSC_WS_URL")
            .unwrap_or_else(|_| "wss://bsc-ws-node.nariox.org:443".to_string());
        let mut bsc = ChainConfig::bsc(bsc_rpc, bsc_ws);
        bsc.enabled = enabled_chains.contains(&56);
        bsc.arbitrage_contract = env::var("BSC_ARBITRAGE_CONTRACT").ok().filter(|s| !s.is_empty());

        // Polygon 配置 (可选)
        let polygon_rpc = env::var("POLYGON_RPC_URL").ok();
        let polygon_ws = env::var("POLYGON_WS_URL").ok();
        let polygon = if let (Some(rpc), Some(ws)) = (polygon_rpc, polygon_ws) {
            let mut cfg = ChainConfig::polygon(rpc, ws);
            cfg.enabled = enabled_chains.contains(&137);
            cfg.arbitrage_contract = env::var("POLYGON_ARBITRAGE_CONTRACT").ok().filter(|s| !s.is_empty());
            Some(cfg)
        } else {
            None
        };

        // Arbitrum 配置 (可选)
        let arbitrum_rpc = env::var("ARBITRUM_RPC_URL").ok();
        let arbitrum_ws = env::var("ARBITRUM_WS_URL").ok();
        let arbitrum = if let (Some(rpc), Some(ws)) = (arbitrum_rpc, arbitrum_ws) {
            let mut cfg = ChainConfig::arbitrum(rpc, ws);
            cfg.enabled = enabled_chains.contains(&42161);
            cfg.arbitrage_contract = env::var("ARBITRUM_ARBITRAGE_CONTRACT").ok().filter(|s| !s.is_empty());
            Some(cfg)
        } else {
            None
        };

        // Base 配置 (可选)
        let base_rpc = env::var("BASE_RPC_URL").ok();
        let base_ws = env::var("BASE_WS_URL").ok();
        let base = if let (Some(rpc), Some(ws)) = (base_rpc, base_ws) {
            let mut cfg = ChainConfig::base(rpc, ws);
            cfg.enabled = enabled_chains.contains(&8453);
            cfg.arbitrage_contract = env::var("BASE_ARBITRAGE_CONTRACT").ok().filter(|s| !s.is_empty());
            Some(cfg)
        } else {
            None
        };

        // 构建链配置 HashMap
        let mut chains: HashMap<u64, ChainConfig> = HashMap::new();
        chains.insert(1, ethereum.clone());
        chains.insert(56, bsc.clone());
        if let Some(cfg) = polygon {
            chains.insert(137, cfg);
        }
        if let Some(cfg) = arbitrum {
            chains.insert(42161, cfg);
        }
        if let Some(cfg) = base {
            chains.insert(8453, cfg);
        }

        // 套利配置
        let arbitrage = ArbitrageConfig {
            max_slippage: env::var("MAX_SLIPPAGE")
                .unwrap_or_else(|_| "0.0005".to_string())
                .parse()
                .unwrap_or(0.0005),
            min_profit_threshold: env::var("MIN_PROFIT_THRESHOLD")
                .unwrap_or_else(|_| "10.0".to_string())
                .parse()
                .unwrap_or(10.0),
            max_path_hops: env::var("MAX_PATH_HOPS")
                .unwrap_or_else(|_| "3".to_string())
                .parse()
                .unwrap_or(3),
            gas_price_multiplier: env::var("GAS_PRICE_MULTIPLIER")
                .unwrap_or_else(|_| "1.2".to_string())
                .parse()
                .unwrap_or(1.2),
            max_gas_price_gwei: env::var("MAX_GAS_PRICE_GWEI")
                .ok()
                .and_then(|s| s.parse().ok()),
            dry_run: env::var("DRY_RUN")
                .ok()
                .and_then(|s| s.parse().ok()),
            auto_execute: env::var("AUTO_EXECUTE")
                .ok()
                .and_then(|s| s.parse().ok()),
            min_swap_value_usd: env::var("MIN_SWAP_VALUE_USD")
                .unwrap_or_else(|_| "1.0".to_string())
                .parse()
                .unwrap_or(1.0),
            skip_local_calc_threshold_usd: env::var("SKIP_LOCAL_CALC_THRESHOLD_USD")
                .unwrap_or_else(|_| "5000.0".to_string())
                .parse()
                .unwrap_or(5000.0),
            // 动态利润门槛配置
            min_profit_ultra_low_gas: env::var("MIN_PROFIT_ULTRA_LOW_GAS")
                .unwrap_or_else(|_| "1.0".to_string())
                .parse()
                .unwrap_or(1.0),
            min_profit_low_gas: env::var("MIN_PROFIT_LOW_GAS")
                .unwrap_or_else(|_| "3.0".to_string())
                .parse()
                .unwrap_or(3.0),
            min_profit_normal_gas: env::var("MIN_PROFIT_NORMAL_GAS")
                .unwrap_or_else(|_| "5.0".to_string())
                .parse()
                .unwrap_or(5.0),
            min_profit_high_gas: env::var("MIN_PROFIT_HIGH_GAS")
                .unwrap_or_else(|_| "15.0".to_string())
                .parse()
                .unwrap_or(15.0),
            min_profit_very_high_gas: env::var("MIN_PROFIT_VERY_HIGH_GAS")
                .unwrap_or_else(|_| "30.0".to_string())
                .parse()
                .unwrap_or(30.0),
        };

        // 闪电贷配置
        let flash_loan_provider = match env::var("FLASH_LOAN_PROVIDER")
            .unwrap_or_else(|_| "uniswap_v3".to_string())
            .to_lowercase()
            .as_str()
        {
            "uniswap_v3" => FlashLoanProvider::UniswapV3,
            "uniswap_v4" => FlashLoanProvider::UniswapV4,
            "aave" => FlashLoanProvider::Aave,
            "balancer" => FlashLoanProvider::Balancer,
            _ => FlashLoanProvider::UniswapV3,
        };

        let flash_loan = FlashLoanConfig {
            provider: flash_loan_provider,
        };

        // MEV 保护配置
        let mev = MevConfig {
            use_flashbots: env::var("USE_FLASHBOTS")
                .unwrap_or_else(|_| "false".to_string())
                .parse()
                .unwrap_or(false),
            flashbots_rpc: env::var("FLASHBOTS_RPC_URL").ok(),
            use_public_mempool: env::var("USE_PUBLIC_MEMPOOL")
                .unwrap_or_else(|_| "false".to_string())
                .parse()
                .unwrap_or(false),
            priority_fee_gwei: env::var("PRIORITY_FEE_GWEI")
                .ok()
                .and_then(|s| s.parse().ok()),
            flashbots_signer_key: env::var("FLASHBOTS_SIGNER_KEY").ok(),
            max_block_retries: env::var("FLASHBOTS_MAX_BLOCK_RETRIES")
                .ok()
                .and_then(|s| s.parse().ok()),
        };

        // 钱包配置 (全局默认，可被链级别覆盖)
        let wallet = WalletConfig {
            private_key: env::var("PRIVATE_KEY").ok().filter(|s| !s.is_empty()),
            arbitrage_contract_address: env::var("ARBITRAGE_CONTRACT_ADDRESS")
                .ok()
                .filter(|s| !s.is_empty()),
        };

        // API 配置
        let api = ApiConfig {
            host: env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env::var("SERVER_PORT")
                .unwrap_or_else(|_| "9530".to_string())
                .parse()
                .context("Invalid SERVER_PORT")?,
        };

        // 日志配置
        let log = LogConfig {
            level: env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
            file_path: env::var("LOG_FILE_PATH")
                .unwrap_or_else(|_| "./logs/dex_arbitrage.log".to_string()),
        };

        Ok(Self {
            database: DatabaseConfig {
                url: database_url,
                max_connections: db_max_connections,
            },
            ethereum,
            bsc,
            chains,
            enabled_chains,
            arbitrage,
            flash_loan,
            mev,
            wallet,
            api,
            log,
        })
    }
}
