use anyhow::Result;
use ethers::types::Address;
use rust_decimal::Decimal;
use dashmap::DashMap;
use tracing::info;

/// Token 价格缓存
pub struct PriceCache {
    /// token address -> USD price
    prices: DashMap<Address, Decimal>,
    /// 最后更新时间
    last_updated: DashMap<Address, chrono::DateTime<chrono::Utc>>,
}

impl PriceCache {
    pub fn new() -> Self {
        Self {
            prices: DashMap::new(),
            last_updated: DashMap::new(),
        }
    }

    pub fn get_price(&self, token: &Address) -> Option<Decimal> {
        self.prices.get(token).map(|p| *p)
    }

    pub fn set_price(&self, token: Address, price: Decimal) {
        self.prices.insert(token, price);
        self.last_updated.insert(token, chrono::Utc::now());
    }

    pub fn is_stale(&self, token: &Address, max_age_seconds: i64) -> bool {
        if let Some(updated) = self.last_updated.get(token) {
            let age = chrono::Utc::now() - *updated;
            age.num_seconds() > max_age_seconds
        } else {
            true
        }
    }
}

impl Default for PriceCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Gas 价格信息
#[derive(Debug, Clone)]
pub struct GasPrice {
    pub base_fee: u128,
    pub priority_fee: u128,
    pub max_fee: u128,
}

/// ETH 价格获取器
pub struct EthPriceFetcher {
    client: reqwest::Client,
    cache: PriceCache,
}

impl EthPriceFetcher {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            cache: PriceCache::new(),
        }
    }

    /// 从 CoinGecko 获取 ETH 价格
    pub async fn fetch_eth_price(&self) -> Result<Decimal> {
        // 检查缓存
        let eth_address = Address::zero(); // 使用零地址表示 ETH
        if !self.cache.is_stale(&eth_address, 60) {
            if let Some(price) = self.cache.get_price(&eth_address) {
                return Ok(price);
            }
        }

        // 从 CoinGecko 获取价格
        let url = "https://api.coingecko.com/api/v3/simple/price?ids=ethereum&vs_currencies=usd";
        let response: serde_json::Value = self.client.get(url).send().await?.json().await?;

        let price = response["ethereum"]["usd"]
            .as_f64()
            .map(|p| Decimal::from_f64_retain(p).unwrap_or(Decimal::ZERO))
            .unwrap_or(Decimal::ZERO);

        self.cache.set_price(eth_address, price);
        info!("获取 ETH 价格: ${}", price);

        Ok(price)
    }

    /// 从 CoinGecko 获取 token 价格
    pub async fn fetch_token_price(&self, coingecko_id: &str) -> Result<Decimal> {
        let url = format!(
            "https://api.coingecko.com/api/v3/simple/price?ids={}&vs_currencies=usd",
            coingecko_id
        );
        let response: serde_json::Value = self.client.get(&url).send().await?.json().await?;

        let price = response[coingecko_id]["usd"]
            .as_f64()
            .map(|p| Decimal::from_f64_retain(p).unwrap_or(Decimal::ZERO))
            .unwrap_or(Decimal::ZERO);

        Ok(price)
    }
}

impl Default for EthPriceFetcher {
    fn default() -> Self {
        Self::new()
    }
}
