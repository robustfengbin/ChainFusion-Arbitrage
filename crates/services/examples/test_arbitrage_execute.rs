//! 套利执行测试 example
//!
//! 基于真实发现的套利机会进行合约执行测试
//!
//! 测试参数来源:
//! - 时间: 2025-12-14 11:47:06 UTC
//! - 区块: 24010638
//! - 机会ID: 05945431-bab9-4c63-9bf7-d571f7b04b4c
//! - 套利路径: USDT(3000)/WETH -> WETH(500)/USDC -> USDC(100)/USDT
//! - 输入金额: 2393.9199 USDT
//! - 预期利润: $2.7461
//!
//! 运行方式:
//! ```bash
//! cd backend_rust
//! cargo run --example test_arbitrage_execute -p services
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
    /// 1% 费率
    #[allow(dead_code)]
    pub const FEE_10000: u32 = 10000;
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
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║             🧪 套利合约执行测试 - Test Arbitrage Execute         ║");
    println!("╠══════════════════════════════════════════════════════════════════╣");
    println!("║ 警告: 这是真实的链上交易测试，可能产生亏损!                       ║");
    println!("║ 仅用于调试合约执行是否正常。                                      ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
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

    info!("   RPC URL: {}...", &rpc_url[..50.min(rpc_url.len())]);
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
    info!("   当前 Gas Price: {:.2} Gwei", gas_price_gwei);

    // ==================== 3. 解析钱包地址 ====================
    info!("\n👛 Step 3: 解析钱包...");

    let wallet: LocalWallet = private_key.parse::<LocalWallet>()?.with_chain_id(chain_id.as_u64());
    let wallet_address = wallet.address();

    // 获取钱包 ETH 余额
    let eth_balance = provider.get_balance(wallet_address, None).await?;
    let eth_balance_f64 = eth_balance.as_u128() as f64 / 1e18;

    info!("   钱包地址: {:?}", wallet_address);
    info!("   ETH 余额: {:.6} ETH", eth_balance_f64);

    // ==================== 4. 构造套利参数 (自动选择闪电贷池) ====================
    info!("\n📝 Step 4: 构造套利参数 (自动选择闪电贷池)...");

    // 基于真实发现的套利机会:
    // 路径: USDT(3000)/WETH -> WETH(500)/USDC -> USDC(100)/USDT
    // 输入金额: 2393.9199 USDT

    // USDT 有 6 位小数
    // 2393.9199 USDT = 2393919900 (6 decimals)
    let input_amount_usdt = 2393.9199_f64;
    let amount_in = U256::from((input_amount_usdt * 1_000_000.0) as u64);

    // swap 路径中使用的池子地址 (需要从闪电贷池选择中排除)
    // 这些地址可以通过 Uniswap V3 Factory.getPool() 获取
    // USDT/WETH 0.3%: 0x4e68Ccd3E89f51C3074ca5072bbAC773960dFa36
    // WETH/USDC 0.05%: 0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640
    // USDC/USDT 0.01%: 0x3416cF6C708Da44DB2624D63ea0AAef7113527C6
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
            tokens::usdt(),  // token_a: USDT - 起始代币 (借入)
            tokens::weth(),  // token_b: WETH - 中间代币1
            tokens::usdc(),  // token_c: USDC - 中间代币2
            fees::FEE_3000,  // fee1: USDT -> WETH: 0.3%
            fees::FEE_500,   // fee2: WETH -> USDC: 0.05%
            fees::FEE_100,   // fee3: USDC -> USDT: 0.01%
            amount_in,
            swap_pools.clone(),
            Decimal::from_str("2.7461")?,   // estimated_profit_usd
            Decimal::from_str("0.0398")?,   // estimated_gas_cost_usd
        )
        .await?;

    info!("   ┌─────────────────────────────────────────────────────┐");
    info!("   │ 套利路径详情                                        │");
    info!("   ├─────────────────────────────────────────────────────┤");
    info!("   │ 🎯 闪电贷池 (自动选择):                              │");
    info!("   │    地址: {:?}", params.flash_pool);
    info!("   │    费率: {} ({:.4}%)", params.flash_pool_fee, params.flash_pool_fee as f64 / 10000.0);
    info!("   │    预估费用: {} wei", params.estimated_flash_fee);
    info!("   │                                                     │");
    info!("   │ 路径:                                               │");
    info!("   │   Token A (USDT): {:?}  │", tokens::usdt());
    info!("   │        ↓ Swap (Fee: 0.3%)                          │");
    info!("   │   Token B (WETH): {:?}  │", tokens::weth());
    info!("   │        ↓ Swap (Fee: 0.05%)                         │");
    info!("   │   Token C (USDC): {:?}  │", tokens::usdc());
    info!("   │        ↓ Swap (Fee: 0.01%)                         │");
    info!("   │   Token A (USDT): 归还闪电贷 + 利润                 │");
    info!("   │                                                     │");
    info!("   │ 输入金额: {} USDT                           │", input_amount_usdt);
    info!("   │ 输入金额 (wei): {}                          │", amount_in);
    info!("   │ 最小利润: {} (测试设为 0)                          │", params.min_profit);
    info!("   │ 预估利润: ${:.4}                                   │", params.estimated_profit_usd);
    info!("   │ 预估 Gas 成本: ${:.4}                              │", params.estimated_gas_cost_usd);
    info!("   └─────────────────────────────────────────────────────┘");

    // ==================== 5. 创建执行器 ====================
    info!("\n⚙️ Step 5: 创建套利执行器...");

    let contract_addr = Address::from_str(&contract_address)?;

    // Flashbots 配置 - 防止 MEV 攻击，交易不会进入公开内存池
    let flashbots_config = FlashbotsConfig {
        enabled: true,
        relay_url: "https://relay.flashbots.net".to_string(),
        chain_id: chain_id.as_u64(),
        max_block_retries: 3,           // 尝试 3 个区块
        signer_key: None,               // 使用交易私钥作为签名密钥
    };

    let executor_config = ExecutorConfig {
        contract_address: contract_addr,
        chain_id: chain_id.as_u64(),
        gas_strategy: GasStrategy {
            gas_price_multiplier: 1.2,  // Gas 价格 +20%
            max_gas_price_gwei: 0.06,   // 最大 0.06 Gwei (当前低 Gas 环境)
            gas_limit_multiplier: 1.3,  // Gas Limit +30%
            use_eip1559: true,
            priority_fee_gwei: 0.001,   // 优先费 0.001 Gwei
            fixed_gas_limit: Some(500_000),  // 固定 Gas Limit，跳过估算直接发送 Flashbots
        },
        confirmation_timeout_secs: 120,  // 2 分钟超时
        confirmations: 1,
        simulate_before_execute: false,  // 关闭模拟，直接通过 Flashbots 发送测试
        private_key: Some(private_key.clone()),
        send_mode: SendMode::Flashbots,   // Flashbots 模式，防止 MEV 三明治攻击
        flashbots_config,
    };

    info!("   合约地址: {:?}", executor_config.contract_address);
    info!("   Gas 策略:");
    info!("     - Gas Price 倍数: {:.1}x", executor_config.gas_strategy.gas_price_multiplier);
    info!("     - 最大 Gas Price: {} Gwei", executor_config.gas_strategy.max_gas_price_gwei);
    info!("     - Gas Limit 倍数: {:.1}x", executor_config.gas_strategy.gas_limit_multiplier);
    info!("     - 使用 EIP-1559: {}", executor_config.gas_strategy.use_eip1559);
    info!("     - 优先费: {} Gwei", executor_config.gas_strategy.priority_fee_gwei);
    info!("   模拟执行: {}", executor_config.simulate_before_execute);
    info!("   发送模式: {:?} (防 MEV 攻击)", executor_config.send_mode);
    info!("   Flashbots 配置:");
    info!("     - 中继 URL: {}", executor_config.flashbots_config.relay_url);
    info!("     - 最大重试区块数: {}", executor_config.flashbots_config.max_block_retries);
    info!("     - 启用: {}", executor_config.flashbots_config.enabled);

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
                warn!("   ⚠️ 当前钱包不是合约 Owner，可能无法执行某些操作");
            }
        }
        Err(e) => {
            error!("   ❌ 无法获取合约 Owner: {:?}", e);
        }
    }

    // 检查合约中的代币余额
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

    // ==================== 7. 执行套利 ====================
    info!("\n🚀 Step 7: 执行套利交易...");
    info!("   ⏳ 开始执行，请等待...");

    let start_time = std::time::Instant::now();

    match executor.execute(params.clone()).await {
        Ok(result) => {
            let elapsed = start_time.elapsed();

            println!("\n");
            println!("╔══════════════════════════════════════════════════════════════════╗");
            println!("║                    ✅ 套利执行成功!                              ║");
            println!("╠══════════════════════════════════════════════════════════════════╣");
            info!("║ 交易哈希: {:?}", result.tx_hash);
            info!("║ 区块号: {}", result.block_number);
            info!("║ 执行耗时: {:.2}s", elapsed.as_secs_f64());
            println!("╠══════════════════════════════════════════════════════════════════╣");
            info!("║ 利润 (wei): {}", result.profit);
            info!("║ 利润 (USD): ${:.4}", result.profit_usd);
            info!("║ Gas 使用量: {}", result.gas_used);
            info!("║ Gas 成本 (USD): ${:.4}", result.gas_cost_usd);
            info!("║ 净利润 (USD): ${:.4}", result.net_profit_usd);
            println!("╠══════════════════════════════════════════════════════════════════╣");

            if result.net_profit_usd >= Decimal::ZERO {
                info!("║ 💰 状态: 盈利!");
            } else {
                warn!("║ 💸 状态: 亏损 (测试预期)");
            }
            println!("╚══════════════════════════════════════════════════════════════════╝");

            // Etherscan 链接
            println!("\n📎 Etherscan 链接:");
            println!("   https://etherscan.io/tx/{:?}", result.tx_hash);
        }
        Err(e) => {
            let elapsed = start_time.elapsed();
            let error_str = format!("{:?}", e);

            println!("\n");
            println!("╔══════════════════════════════════════════════════════════════════╗");
            println!("║                    ❌ 套利执行失败!                              ║");
            println!("╠══════════════════════════════════════════════════════════════════╣");
            error!("║ 错误类型: {:?}", e);
            error!("║ 执行耗时: {:.2}s", elapsed.as_secs_f64());
            println!("╚══════════════════════════════════════════════════════════════════╝");

            // 使用 RevertDecoder 解析详细错误信息
            println!("\n📋 错误详情解析:");
            let decoded = RevertDecoder::decode_from_error_string(&error_str);
            println!("{}", decoded);

            // 返回错误
            return Err(anyhow::anyhow!("套利执行失败: {:?}", e));
        }
    }

    // ==================== 8. 最终状态 ====================
    info!("\n📊 Step 8: 检查最终状态...");

    // 再次检查合约中的代币余额
    info!("   检查合约中的代币余额变化...");
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
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║                    🎉 测试完成!                                  ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!("\n");

    Ok(())
}
