use anyhow::Result;
use ethers::types::Address;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::{info, warn, debug};

/// 价格服务配置
#[derive(Debug, Clone)]
pub struct PriceServiceConfig {
    pub update_interval_secs: u64,
    /// 币安 API 基础 URL
    pub binance_api_url: String,
}

impl Default for PriceServiceConfig {
    fn default() -> Self {
        Self {
            update_interval_secs: 30,
            binance_api_url: "https://api.binance.com".to_string(),
        }
    }
}

/// Token 价格信息
#[derive(Debug, Clone)]
pub struct TokenPrice {
    pub symbol: String,
    pub price_usd: Decimal,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

/// 币安价格响应
#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct BinanceTickerPrice {
    symbol: String,
    price: String,
}

/// 实时价格服务 (使用币安现货 API)
pub struct PriceService {
    config: PriceServiceConfig,
    http_client: reqwest::Client,
    /// symbol -> price (如 "ETH" -> price)
    prices: RwLock<HashMap<String, TokenPrice>>,
    /// address -> symbol 映射
    address_to_symbol: RwLock<HashMap<Address, String>>,
    /// 是否正在运行
    running: RwLock<bool>,
}

impl PriceService {
    pub fn new(config: PriceServiceConfig) -> Self {
        Self {
            config,
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            prices: RwLock::new(HashMap::new()),
            address_to_symbol: RwLock::new(HashMap::new()),
            running: RwLock::new(false),
        }
    }

    /// 初始化常见代币地址映射
    pub async fn init_token_mappings(&self) {
        let mut mapping = self.address_to_symbol.write().await;

        // Ethereum Mainnet - 主流代币
        mapping.insert(
            "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".parse().unwrap(), // WETH
            "ETH".to_string(),
        );
        mapping.insert(
            "0xdAC17F958D2ee523a2206206994597C13D831ec7".parse().unwrap(), // USDT
            "USDT".to_string(),
        );
        mapping.insert(
            "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".parse().unwrap(), // USDC
            "USDC".to_string(),
        );
        mapping.insert(
            "0x6B175474E89094C44Da98b954EedeAC495271d0F".parse().unwrap(), // DAI
            "DAI".to_string(),
        );
        mapping.insert(
            "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599".parse().unwrap(), // WBTC
            "BTC".to_string(),
        );

        // Ethereum Mainnet - DeFi 代币
        mapping.insert(
            "0x1f9840a85d5aF5bf1D1762F925BDADdC4201F984".parse().unwrap(), // UNI
            "UNI".to_string(),
        );
        mapping.insert(
            "0x514910771AF9Ca656af840dff83E8264EcF986CA".parse().unwrap(), // LINK
            "LINK".to_string(),
        );
        mapping.insert(
            "0x7Fc66500c84A76Ad7e9c93437bFc5Ac33E2DDaE9".parse().unwrap(), // AAVE
            "AAVE".to_string(),
        );
        mapping.insert(
            "0x9f8F72aA9304c8B593d555F12eF6589cC3A579A2".parse().unwrap(), // MKR
            "MKR".to_string(),
        );
        mapping.insert(
            "0xD533a949740bb3306d119CC777fa900bA034cd52".parse().unwrap(), // CRV
            "CRV".to_string(),
        );
        mapping.insert(
            "0x5A98FcBEA516Cf06857215779Fd812CA3beF1B32".parse().unwrap(), // LDO
            "LDO".to_string(),
        );
        mapping.insert(
            "0x4d224452801ACEd8B2F0aebE155379bb5D594381".parse().unwrap(), // APE
            "APE".to_string(),
        );

        // Ethereum Mainnet - Liquid Staking
        mapping.insert(
            "0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84".parse().unwrap(), // stETH
            "stETH".to_string(),
        );

        // Ethereum Mainnet - Meme 币
        mapping.insert(
            "0x6982508145454Ce325dDbE47a25d4ec3d2311933".parse().unwrap(), // PEPE
            "PEPE".to_string(),
        );
        mapping.insert(
            "0x95aD61b0a150d79219dCF64E1E6Cc01f0B64C4cE".parse().unwrap(), // SHIB
            "SHIB".to_string(),
        );

        // BSC
        mapping.insert(
            "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c".parse().unwrap(), // WBNB
            "BNB".to_string(),
        );
        mapping.insert(
            "0xe9e7CEA3DedcA5984780Bafc599bD69ADd087D56".parse().unwrap(), // BUSD
            "BUSD".to_string(),
        );
        mapping.insert(
            "0x55d398326f99059fF775485246999027B3197955".parse().unwrap(), // BSC-USDT
            "USDT".to_string(),
        );

        info!("初始化了 {} 个代币地址映射", mapping.len());
    }

    /// 启动价格更新服务
    pub async fn start(&self) -> Result<()> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(());
        }
        *running = true;
        drop(running);

        info!("价格服务启动 (币安现货), 更新间隔: {}s", self.config.update_interval_secs);

        // 初始化代币映射
        self.init_token_mappings().await;

        // 首次更新
        if let Err(e) = self.update_all_prices().await {
            warn!("首次价格更新失败: {}", e);
        }

        // 启动定时更新
        let mut update_interval = interval(Duration::from_secs(self.config.update_interval_secs));

        loop {
            let running = self.running.read().await;
            if !*running {
                break;
            }
            drop(running);

            update_interval.tick().await;

            if let Err(e) = self.update_all_prices().await {
                warn!("价格更新失败: {}", e);
            }
        }

        info!("价格服务停止");
        Ok(())
    }

    /// 停止价格服务
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
    }

    /// 更新所有价格 (使用币安现货 API)
    async fn update_all_prices(&self) -> Result<()> {
        // 需要获取价格的交易对 (相对 USDT)
        let symbols = vec![
            // 主流代币
            ("ETHUSDT", "ETH"),
            ("BTCUSDT", "BTC"),
            ("BNBUSDT", "BNB"),
            ("DAIUSDT", "DAI"),
            // DeFi 代币
            ("UNIUSDT", "UNI"),
            ("LINKUSDT", "LINK"),
            ("AAVEUSDT", "AAVE"),
            ("MKRUSDT", "MKR"),
            ("CRVUSDT", "CRV"),
            ("LDOUSDT", "LDO"),
            ("APEUSDT", "APE"),
            // Meme 币
            ("PEPEUSDT", "PEPE"),
            ("SHIBUSDT", "SHIB"),
        ];

        // stETH 价格等于 ETH (Liquid Staking 代币)
        let eth_linked_tokens = vec!["stETH"];

        // 稳定币固定价格
        let stablecoins = vec![
            ("USDT", Decimal::ONE),
            ("USDC", Decimal::ONE),
            ("BUSD", Decimal::ONE),
        ];

        let now = chrono::Utc::now();
        let mut prices = self.prices.write().await;

        // 设置稳定币价格
        for (symbol, price) in stablecoins {
            prices.insert(
                symbol.to_string(),
                TokenPrice {
                    symbol: symbol.to_string(),
                    price_usd: price,
                    last_updated: now,
                },
            );
        }

        // 从币安获取其他代币价格
        let mut eth_price = Decimal::from(3000); // 默认值
        for (pair, symbol) in &symbols {
            match self.fetch_binance_price(pair).await {
                Ok(price) => {
                    // 保存 ETH 价格用于关联代币
                    if *symbol == "ETH" {
                        eth_price = price;
                    }
                    prices.insert(
                        symbol.to_string(),
                        TokenPrice {
                            symbol: symbol.to_string(),
                            price_usd: price,
                            last_updated: now,
                        },
                    );
                    debug!("{}: ${}", symbol, price);
                }
                Err(e) => {
                    warn!("获取 {} 价格失败: {}", pair, e);
                }
            }
        }

        // 设置与 ETH 价格挂钩的代币 (stETH ≈ ETH)
        for symbol in eth_linked_tokens {
            prices.insert(
                symbol.to_string(),
                TokenPrice {
                    symbol: symbol.to_string(),
                    price_usd: eth_price,
                    last_updated: now,
                },
            );
        }

        info!("价格更新完成, {} 个代币", prices.len());
        Ok(())
    }

    /// 从币安获取单个交易对价格
    async fn fetch_binance_price(&self, symbol: &str) -> Result<Decimal> {
        let url = format!(
            "{}/api/v3/ticker/price?symbol={}",
            self.config.binance_api_url, symbol
        );

        let response = self.http_client.get(&url).send().await?;
        let ticker: BinanceTickerPrice = response.json().await?;

        let price = Decimal::from_str(&ticker.price)?;
        Ok(price)
    }

    /// 获取 ETH 价格
    pub async fn get_eth_price(&self) -> Decimal {
        let prices = self.prices.read().await;
        prices
            .get("ETH")
            .map(|p| p.price_usd)
            .unwrap_or(Decimal::from(2000))
    }

    /// 获取 BNB 价格
    pub async fn get_bnb_price(&self) -> Decimal {
        let prices = self.prices.read().await;
        prices
            .get("BNB")
            .map(|p| p.price_usd)
            .unwrap_or(Decimal::from(300))
    }

    /// 获取代币价格 (通过 symbol)
    pub async fn get_price_by_symbol(&self, symbol: &str) -> Option<Decimal> {
        let prices = self.prices.read().await;
        prices.get(symbol).map(|p| p.price_usd)
    }

    /// 获取代币价格 (通过地址)
    pub async fn get_price_by_address(&self, address: &Address) -> Option<Decimal> {
        let mapping = self.address_to_symbol.read().await;
        if let Some(symbol) = mapping.get(address) {
            return self.get_price_by_symbol(symbol).await;
        }
        None
    }

    /// 添加自定义代币映射
    pub async fn add_token_mapping(&self, address: Address, symbol: String) {
        let mut mapping = self.address_to_symbol.write().await;
        mapping.insert(address, symbol);
    }

    /// 获取所有价格
    pub async fn get_all_prices(&self) -> HashMap<String, TokenPrice> {
        let prices = self.prices.read().await;
        prices.clone()
    }
}

/// 可共享的价格服务
pub type SharedPriceService = Arc<PriceService>;

/// 创建共享的价格服务
pub fn create_price_service(config: PriceServiceConfig) -> SharedPriceService {
    Arc::new(PriceService::new(config))
}
