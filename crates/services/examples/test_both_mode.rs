//! Both 模式测试 - 同时通过 Flashbots 和公开 mempool 发送交易
//!
//! 测试两种发送渠道是否都能正常工作:
//! - Flashbots: 私密交易，防止 MEV 攻击
//! - Public Mempool: 通过 Alchemy RPC 发送到公开内存池
//!
//! 两边使用不同的 nonce，都能被打包执行
//!
//! 运行方式:
//! ```bash
//! cd backend_rust
//! cargo run --example test_both_mode -p services
//! ```

use anyhow::Result;
use ethers::prelude::*;
use ethers::types::{Address, U256};
use executor::{
    ArbitrageExecutor, ExecutorConfig, FlashbotsConfig, GasStrategy, SendMode,
    ArbitrageParamsBuilder, RevertDecoder,
};
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{info, warn, error};

/// ETH Mainnet 代币地址
mod tokens {
    use ethers::types::Address;
    use std::str::FromStr;

    /// USDT - Tether USD
    pub fn usdt() -> Address {
        Address::from_str("0xdAC17F958D2ee523a2206206994597C13D831ec7").unwrap()
    }

    /// USDC - USD Coin
    pub fn usdc() -> Address {
        Address::from_str("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48").unwrap()
    }

    /// WETH - Wrapped Ether
    pub fn weth() -> Address {
        Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap()
    }
}

/// Uniswap V3 池子费率 (以 1/1000000 为单位)
mod fees {
    /// 0.01% 费率
    pub const FEE_100: u32 = 100;
    /// 0.05% 费率
    pub const FEE_500: u32 = 500;
    /// 0.3% 费率
    pub const FEE_3000: u32 = 3000;
}

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志 - 显示详细信息
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    println!("\n");
    println!("╔══════════════════════════════════════════════════════════════════════════╗");
    println!("║       🧪 Both 模式测试 - Flashbots + Public Mempool 双通道发送          ║");
    println!("╠══════════════════════════════════════════════════════════════════════════╣");
    println!("║ 测试目的:                                                                ║");
    println!("║   - Flashbots 通道: nonce N   (私密交易)                                 ║");
    println!("║   - Mempool 通道:   nonce N+1 (公开交易)                                 ║");
    println!("║ 两边都会执行，用于验证两个渠道是否正常工作                               ║");
    println!("╠══════════════════════════════════════════════════════════════════════════╣");
    println!("║ ⚠️ 警告: 这是真实的链上交易测试，会消耗 Gas!                             ║");
    println!("╚══════════════════════════════════════════════════════════════════════════╝");
    println!("\n");

    // 加载环境变量
    dotenv::dotenv().ok();

    // ==================== 1. 读取配置 ====================
    info!("📋 Step 1: 读取配置...");

    let rpc_url = std::env::var("ETH_RPC_URL")
        .expect("请设置 ETH_RPC_URL 环境变量");
    let private_key = std::env::var("PRIVATE_KEY")
        .expect("请设置 PRIVATE_KEY 环境变量");
    let contract_address = std::env::var("ARBITRAGE_CONTRACT_ADDRESS")
        .expect("请设置 ARBITRAGE_CONTRACT_ADDRESS 环境变量");

    // 读取 Flashbots 配置
    let flashbots_rpc = std::env::var("FLASHBOTS_RPC_URL")
        .unwrap_or_else(|_| "https://relay.flashbots.net".to_string());

    info!("   RPC URL (Mempool): {}...", &rpc_url[..50.min(rpc_url.len())]);
    info!("   Flashbots URL: {}", flashbots_rpc);
    info!("   合约地址: {}", contract_address);
    info!("   私钥已加载 (长度: {} 字符)", private_key.len());

    // ==================== 2. 创建 Provider ====================
    info!("\n📡 Step 2: 连接以太坊节点...");

    let provider = Provider::<Http>::try_from(&rpc_url)?;
    let provider = Arc::new(provider);

    // 获取链 ID 和当前区块
    let chain_id = provider.get_chainid().await?;
    let block_number = provider.get_block_number().await?;
    let gas_price = provider.get_gas_price().await?;
    let gas_price_gwei = gas_price.as_u64() as f64 / 1_000_000_000.0;

    info!("   ✅ 连接成功!");
    info!("   链 ID: {}", chain_id);
    info!("   当前区块: {}", block_number);
    info!("   当前 Gas Price: {:.4} Gwei", gas_price_gwei);

    // ==================== 3. 解析钱包地址 ====================
    info!("\n👛 Step 3: 解析钱包...");

    let wallet: LocalWallet = private_key.parse::<LocalWallet>()?.with_chain_id(chain_id.as_u64());
    let wallet_address = wallet.address();

    // 获取钱包 ETH 余额
    let eth_balance = provider.get_balance(wallet_address, None).await?;
    let eth_balance_f64 = eth_balance.as_u128() as f64 / 1e18;

    // 获取当前 nonce
    let current_nonce = provider.get_transaction_count(wallet_address, None).await?;

    info!("   钱包地址: {:?}", wallet_address);
    info!("   ETH 余额: {:.6} ETH", eth_balance_f64);
    info!("   当前 Nonce: {}", current_nonce);
    info!("   📋 Both 模式将使用:");
    info!("      - Flashbots: nonce = {}", current_nonce);
    info!("      - Mempool:   nonce = {}", current_nonce + 1);

    // ==================== 4. 构造套利参数 ====================
    info!("\n📝 Step 4: 构造套利参数...");

    // 使用小金额测试: 100 USDT
    let input_amount_usdt = 100.0_f64;
    let amount_in = U256::from((input_amount_usdt * 1_000_000.0) as u64);

    // swap 路径池子地址
    let swap_pools = vec![
        Address::from_str("0x4e68Ccd3E89f51C3074ca5072bbAC773960dFa36")?, // USDT/WETH 0.3%
        Address::from_str("0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640")?, // WETH/USDC 0.05%
        Address::from_str("0x3416cF6C708Da44DB2624D63ea0AAef7113527C6")?, // USDC/USDT 0.01%
    ];

    // 使用 ArbitrageParamsBuilder 自动选择最优闪电贷池
    info!("   🔍 自动选择闪电贷池...");
    let params_builder = ArbitrageParamsBuilder::new(provider.clone(), chain_id.as_u64());

    let params = params_builder
        .build_manual(
            tokens::usdt(),  // token_a: USDT
            tokens::weth(),  // token_b: WETH
            tokens::usdc(),  // token_c: USDC
            fees::FEE_3000,  // fee1: 0.3%
            fees::FEE_500,   // fee2: 0.05%
            fees::FEE_100,   // fee3: 0.01%
            amount_in,
            swap_pools.clone(),
            Decimal::from_str("0.1")?,   // estimated_profit_usd (测试用)
            Decimal::from_str("0.05")?,  // estimated_gas_cost_usd
        )
        .await?;

    info!("   ┌─────────────────────────────────────────────────────┐");
    info!("   │ 套利路径详情                                        │");
    info!("   ├─────────────────────────────────────────────────────┤");
    info!("   │ 🎯 闪电贷池: {:?}", params.flash_pool);
    info!("   │    费率: {} ({:.4}%)", params.flash_pool_fee, params.flash_pool_fee as f64 / 10000.0);
    info!("   │ 路径: USDT -> WETH -> USDC -> USDT                  │");
    info!("   │ 输入金额: {} USDT                                   │", input_amount_usdt);
    info!("   └─────────────────────────────────────────────────────┘");

    // ==================== 5. 创建执行器 (Both 模式) ====================
    info!("\n⚙️ Step 5: 创建套利执行器 (Both 模式)...");

    let contract_addr = Address::from_str(&contract_address)?;

    // Flashbots 配置
    let flashbots_config = FlashbotsConfig {
        enabled: true,
        relay_url: flashbots_rpc,
        chain_id: chain_id.as_u64(),
        max_block_retries: 3,
        signer_key: None,
    };

    let executor_config = ExecutorConfig {
        contract_address: contract_addr,
        chain_id: chain_id.as_u64(),
        gas_strategy: GasStrategy {
            gas_price_multiplier: 1.2,
            max_gas_price_gwei: 0.1,        // 最大 0.1 Gwei
            gas_limit_multiplier: 1.3,
            use_eip1559: true,
            priority_fee_gwei: 0.05,       // 优先费 0.005 Gwei
            fixed_gas_limit: Some(500_000), // 固定 Gas Limit
        },
        confirmation_timeout_secs: 180,     // 3 分钟超时 (Both 模式需要更长时间)
        confirmations: 1,
        simulate_before_execute: false,     // 跳过模拟，直接发送
        private_key: Some(private_key.clone()),
        send_mode: SendMode::Both,          // 🔥 Both 模式: 同时发送到两个渠道
        flashbots_config,
    };

    println!("\n");
    println!("   ╔═══════════════════════════════════════════════════════════╗");
    println!("   ║              🚀 Both 模式配置                             ║");
    println!("   ╠═══════════════════════════════════════════════════════════╣");
    info!("   ║ 发送模式: {:?}", executor_config.send_mode);
    info!("   ║ ");
    info!("   ║ 📡 Flashbots 通道:");
    info!("   ║    - Relay URL: {}", executor_config.flashbots_config.relay_url);
    info!("   ║    - 最大重试区块: {}", executor_config.flashbots_config.max_block_retries);
    info!("   ║    - Nonce: {} (先发送)", current_nonce);
    info!("   ║ ");
    info!("   ║ 🌐 Public Mempool 通道:");
    info!("   ║    - RPC URL: {}...", &rpc_url[..50.min(rpc_url.len())]);
    info!("   ║    - Nonce: {} (后发送)", current_nonce + 1);
    info!("   ║ ");
    info!("   ║ ⛽ Gas 配置:");
    info!("   ║    - 最大 Gas Price: {} Gwei", executor_config.gas_strategy.max_gas_price_gwei);
    info!("   ║    - 优先费: {} Gwei", executor_config.gas_strategy.priority_fee_gwei);
    info!("   ║    - 固定 Gas Limit: {:?}", executor_config.gas_strategy.fixed_gas_limit);
    println!("   ╚═══════════════════════════════════════════════════════════╝");
    println!("\n");

    let signer = SignerMiddleware::new(provider.clone(), wallet);
    let signer = Arc::new(signer);

    let executor = ArbitrageExecutor::new(executor_config, signer)?;
    info!("   ✅ 执行器创建成功!");

    // ==================== 6. 检查合约状态 ====================
    info!("\n🔍 Step 6: 检查合约状态...");

    match executor.check_owner().await {
        Ok(owner) => {
            info!("   合约 Owner: {:?}", owner);
            if owner == wallet_address {
                info!("   ✅ 当前钱包是合约 Owner");
            } else {
                warn!("   ⚠️ 当前钱包不是合约 Owner");
            }
        }
        Err(e) => {
            error!("   ❌ 无法获取合约 Owner: {:?}", e);
        }
    }

    // 检查代币余额
    info!("\n   检查合约中的代币余额...");
    for (name, token) in [("USDT", tokens::usdt()), ("USDC", tokens::usdc()), ("WETH", tokens::weth())] {
        match executor.get_token_balance(token).await {
            Ok(balance) => {
                let decimals = if name == "WETH" { 18 } else { 6 };
                let balance_f64 = balance.as_u128() as f64 / 10_f64.powi(decimals);
                info!("   合约 {} 余额: {:.6}", name, balance_f64);
            }
            Err(e) => {
                warn!("   无法获取 {} 余额: {:?}", name, e);
            }
        }
    }

    // ==================== 7. 执行套利 (Both 模式) ====================
    println!("\n");
    println!("╔══════════════════════════════════════════════════════════════════════════╗");
    println!("║                   🚀 开始执行 Both 模式交易                              ║");
    println!("╠══════════════════════════════════════════════════════════════════════════╣");
    println!("║ 两个交易将并行发送:                                                      ║");
    println!("║   1. Flashbots (nonce={})  -> relay.flashbots.net                       ║", current_nonce);
    println!("║   2. Mempool   (nonce={})  -> Alchemy RPC                               ║", current_nonce + 1);
    println!("║                                                                          ║");
    println!("║ 观察日志中的 ✅ / ❌ 标记来判断各通道执行结果                            ║");
    println!("╚══════════════════════════════════════════════════════════════════════════╝");
    println!("\n");

    info!("⏳ 开始执行，请等待...");

    let start_time = std::time::Instant::now();

    match executor.execute(params.clone()).await {
        Ok(result) => {
            let elapsed = start_time.elapsed();

            println!("\n");
            println!("╔══════════════════════════════════════════════════════════════════════════╗");
            println!("║                       ✅ Both 模式执行成功!                              ║");
            println!("╠══════════════════════════════════════════════════════════════════════════╣");
            info!("║ 返回的交易哈希: {:?}", result.tx_hash);
            info!("║ 区块号: {}", result.block_number);
            info!("║ 执行耗时: {:.2}s", elapsed.as_secs_f64());
            println!("╠══════════════════════════════════════════════════════════════════════════╣");
            info!("║ 利润 (wei): {}", result.profit);
            info!("║ 利润 (USD): ${:.4}", result.profit_usd);
            info!("║ Gas 使用量: {}", result.gas_used);
            info!("║ Gas 成本 (USD): ${:.4}", result.gas_cost_usd);
            info!("║ 净利润 (USD): ${:.4}", result.net_profit_usd);
            println!("╚══════════════════════════════════════════════════════════════════════════╝");

            // Etherscan 链接
            println!("\n📎 Etherscan 链接:");
            println!("   https://etherscan.io/tx/{:?}", result.tx_hash);

            // 提示检查两个交易
            println!("\n💡 提示: 检查两个 nonce 的交易:");
            println!("   - Nonce {}: Flashbots 交易", current_nonce);
            println!("   - Nonce {}: Mempool 交易", current_nonce + 1);
        }
        Err(e) => {
            let elapsed = start_time.elapsed();
            let error_str = format!("{:?}", e);

            println!("\n");
            println!("╔══════════════════════════════════════════════════════════════════════════╗");
            println!("║                       ❌ Both 模式执行失败!                              ║");
            println!("╠══════════════════════════════════════════════════════════════════════════╣");
            error!("║ 错误类型: {:?}", e);
            error!("║ 执行耗时: {:.2}s", elapsed.as_secs_f64());
            println!("╚══════════════════════════════════════════════════════════════════════════╝");

            // 解析错误
            println!("\n📋 错误详情解析:");
            let decoded = RevertDecoder::decode_from_error_string(&error_str);
            println!("{}", decoded);

            // 不返回错误，继续检查状态
            warn!("继续检查最终状态...");
        }
    }

    // ==================== 8. 最终状态 ====================
    info!("\n📊 Step 8: 检查最终状态...");

    // 获取新的 nonce
    let new_nonce = provider.get_transaction_count(wallet_address, None).await?;
    info!("   新 Nonce: {} (之前: {})", new_nonce, current_nonce);

    if new_nonce > current_nonce {
        let tx_count = new_nonce - current_nonce;
        info!("   ✅ 成功执行了 {} 笔交易", tx_count);
    }

    // 检查代币余额变化
    info!("\n   检查合约中的代币余额变化...");
    for (name, token) in [("USDT", tokens::usdt()), ("USDC", tokens::usdc()), ("WETH", tokens::weth())] {
        match executor.get_token_balance(token).await {
            Ok(balance) => {
                let decimals = if name == "WETH" { 18 } else { 6 };
                let balance_f64 = balance.as_u128() as f64 / 10_f64.powi(decimals);
                info!("   合约 {} 余额: {:.6}", name, balance_f64);
            }
            Err(e) => {
                warn!("   无法获取 {} 余额: {:?}", name, e);
            }
        }
    }

    println!("\n");
    println!("╔══════════════════════════════════════════════════════════════════════════╗");
    println!("║                         🎉 Both 模式测试完成!                            ║");
    println!("╠══════════════════════════════════════════════════════════════════════════╣");
    println!("║ 请检查日志中的以下标记:                                                  ║");
    println!("║   ✅ Flashbots 发送成功 (nonce=X): 0x...                                 ║");
    println!("║   ✅ 公开 mempool 发送成功 (nonce=X): 0x...                              ║");
    println!("║                                                                          ║");
    println!("║ 如果两边都显示 ✅，说明两个通道都正常工作!                               ║");
    println!("╚══════════════════════════════════════════════════════════════════════════╝");
    println!("\n");

    Ok(())
}
