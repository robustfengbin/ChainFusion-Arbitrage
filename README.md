# ChainFusion Arbitrage

**DEX 三角套利机器人** - 基于 Rust 的高性能去中心化交易所套利系统

一个专业级的自动化套利机器人，能够实时监控多个 DEX 流动性池，检测并执行三角套利策略。支持闪电贷、Flashbots MEV 保护、多链部署等企业级功能。

## 核心特性

- **实时套利检测** - 毫秒级监控 14 个主力池子的 Swap 事件
- **三角套利策略** - 预配置 6 个高流动性三角组合，26 条套利路径
- **闪电贷支持** - 无需初始资金，支持 Uniswap V3/V4、Aave、Balancer
- **MEV 保护** - 集成 Flashbots，防止三明治攻击和抢跑
- **多 DEX 支持** - Uniswap V2/V3/V4、Curve、PancakeSwap、SushiSwap
- **多链支持** - Ethereum、BSC、Polygon、Arbitrum、Base、Optimism、Solana
- **REST API** - 完整的策略管理和监控接口
- **历史回测** - 下载历史数据进行策略验证

## 快速开始

### 环境要求

- Rust 1.75+
- MySQL 8.0+
- Node.js 18+ (可选，用于 PM2 进程管理)

### 安装

```bash
# 克隆仓库
git clone https://github.com/your-username/chainfusion-arbitrage.git
cd chainfusion-arbitrage

# 编译项目
cargo build --release
```

### 配置

```bash
# 复制配置模板
cp .env.example .env

# 编辑配置文件
vim .env
```

关键配置项：

```bash
# 数据库
DB_HOST=127.0.0.1
DB_PORT=3306
DB_USER=root
DB_PASSWORD=your_password
DB_NAME=dex_arbitrage

# RPC 节点 (推荐使用 Alchemy 或 Infura)
ETH_RPC_URL=https://eth-mainnet.g.alchemy.com/v2/your-api-key
ETH_WS_URL=wss://eth-mainnet.g.alchemy.com/v2/your-api-key

# 钱包私钥 (请妥善保管!)
PRIVATE_KEY=your_private_key_here

# 套利参数
MIN_PROFIT_THRESHOLD=10.0  # 最低利润 $10
MAX_SLIPPAGE=0.0005        # 最大滑点 0.05%
```

### 运行

```bash
# 直接运行
cargo run --release -p main

# 或使用 PM2 (推荐生产环境)
chmod +x pm2.sh
./pm2.sh
```

## 系统架构

```
┌─────────────────────────────────────────────────────────────────┐
│                      ChainFusion Arbitrage                       │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐           │
│  │   区块订阅    │  │   池子同步    │  │   价格服务    │           │
│  │ Block Sub    │  │ Pool Syncer  │  │ Price Service│           │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘           │
│         │                  │                  │                   │
│         ▼                  ▼                  ▼                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                     事件驱动扫描器                         │   │
│  │                Event Driven Scanner                       │   │
│  │  • 监控 14 个主力池子的 Swap 事件                          │   │
│  │  • 本地快速估算 + 链上精确报价双层验证                      │   │
│  │  • 实时计算 26 条路径的套利机会                            │   │
│  └──────────────────────────┬───────────────────────────────┘   │
│                              │                                    │
│                              ▼                                    │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                      套利执行器                            │   │
│  │                  Arbitrage Executor                       │   │
│  │  • Normal: 公开 mempool 发送                              │   │
│  │  • Flashbots: 私密 bundle 发送                            │   │
│  │  • Both: 同时发送双保险                                    │   │
│  └──────────────────────────┬───────────────────────────────┘   │
│                              │                                    │
│                              ▼                                    │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                     智能合约执行                           │   │
│  │                FlashArbitrage.sol                         │   │
│  │  • Uniswap V3 闪电贷                                       │   │
│  │  • 原子化三角套利                                          │   │
│  │  • 利润自动提取                                            │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

## 三角套利组合

系统预配置了 6 个高流动性三角套利组合：

| 优先级 | 组合 | 类型 | 说明 |
|--------|------|------|------|
| 10 | DAI-USDC-USDT | 稳定币 | 手续费最低(0.01%)，滑点极低 |
| 20 | USDC-WETH-USDT | ETH-稳定币 | 大 ETH 成交触发，高频机会 |
| 30 | DAI-USDC-WETH | DAI-ETH | 适合事件驱动策略 |
| 40 | WBTC-USDC-USDT | BTC-稳定币 | 单次利润高，频率较低 |
| 50 | WBTC-WETH-USDC | BTC-ETH-USDC | CEX-DEX 同步延迟机会 |
| 60 | WBTC-WETH-USDT | BTC-ETH-USDT | 剧烈行情时出现 |

### 套利路径示例

```
正向: DAI → USDC → USDT → DAI
      │        │        │
      └─ 0.01% ─┴─ 0.01% ─┴─ 0.01%

反向: DAI → USDT → USDC → DAI
```

## 项目结构

```
chainfusion-arbitrage/
├── crates/
│   ├── main/           # 主应用入口
│   ├── models/         # 数据模型 (Token, Pool, ArbitragePath)
│   ├── config/         # 配置管理
│   ├── services/       # 核心服务
│   │   ├── block_subscriber.rs    # 区块订阅
│   │   ├── pool_syncer.rs         # 池子同步
│   │   ├── price_service.rs       # 价格服务
│   │   └── database.rs            # 数据库操作
│   ├── strategies/     # 套利策略 (核心!)
│   │   ├── event_driven_scanner.rs  # 事件驱动扫描
│   │   ├── arbitrage_executor.rs    # 套利执行
│   │   ├── profit_calculator.rs     # 利润计算
│   │   └── path_finder.rs           # 路径查找
│   ├── executor/       # 交易执行器
│   │   ├── executor.rs           # 主执行器
│   │   ├── flash_arbitrage.rs    # 闪电贷执行
│   │   └── flashbots/            # Flashbots 集成
│   ├── dex/            # DEX 集成
│   │   ├── uniswap/    # Uniswap V2/V3/V4
│   │   ├── curve/      # Curve
│   │   ├── pancakeswap/# PancakeSwap
│   │   └── flashloan/  # 闪电贷提供者
│   ├── api/            # REST API (Axum)
│   ├── backtest/       # 历史回测工具
│   ├── solana/         # Solana 链支持
│   └── utils/          # 工具库
├── docs/               # 详细文档
├── scripts/            # 脚本工具
└── .env.example        # 配置模板
```

## API 接口

系统提供完整的 REST API 用于策略管理和监控：

### 健康检查

```bash
GET /health
```

### 策略管理

```bash
# 列出所有策略
GET /api/strategies

# 创建策略
POST /api/strategies
Content-Type: application/json
{
  "name": "ETH-Stable Triangle",
  "min_profit_usd": 10.0,
  "max_slippage": 0.0005
}

# 启动/停止策略
POST /api/strategies/:id/start
POST /api/strategies/:id/stop

# 查看运行中的策略
GET /api/strategies/running
```

### 交易记录

```bash
# 交易列表
GET /api/trades

# 交易详情
GET /api/trades/:id
```

### 统计信息

```bash
# 系统统计
GET /api/statistics

# 策略统计
GET /api/statistics/:strategy_id
```

### 系统状态

```bash
# 系统状态
GET /api/system/status

# 池子列表
GET /api/system/pools

# 套利机会
GET /api/opportunities
```

## 配置说明

### 完整配置项

```bash
# ============================
# 数据库配置
# ============================
DB_HOST=127.0.0.1
DB_PORT=3306
DB_USER=root
DB_PASSWORD=your_password_here
DB_NAME=dex_arbitrage
DB_MAX_CONNECTIONS=10

# ============================
# RPC 配置
# ============================
# Ethereum Mainnet
ETH_RPC_URL=https://eth-mainnet.g.alchemy.com/v2/your-api-key
ETH_WS_URL=wss://eth-mainnet.g.alchemy.com/v2/your-api-key

# BSC Mainnet
BSC_RPC_URL=https://bsc-dataseed1.binance.org
BSC_WS_URL=wss://bsc-ws-node.nariox.org:443

# ============================
# 套利参数
# ============================
MAX_SLIPPAGE=0.0005           # 最大滑点 0.05%
MIN_PROFIT_THRESHOLD=10.0     # 最低利润阈值 $10
MAX_PATH_HOPS=3               # 最大路径跳数
GAS_PRICE_MULTIPLIER=1.2      # Gas 价格倍数

# ============================
# 闪电贷配置
# ============================
# 支持: uniswap_v3, uniswap_v4, aave, balancer
FLASH_LOAN_PROVIDER=uniswap_v3

# ============================
# MEV 保护
# ============================
USE_FLASHBOTS=false
FLASHBOTS_RPC_URL=https://relay.flashbots.net
USE_PUBLIC_MEMPOOL=false

# ============================
# 钱包配置
# ============================
PRIVATE_KEY=your_private_key_here

# ============================
# 服务器配置
# ============================
SERVER_PORT=9530
SERVER_HOST=0.0.0.0

# ============================
# 日志配置
# ============================
RUST_LOG=info
LOG_FILE_PATH=./logs/dex_arbitrage.log
```

### 执行模式

| 模式 | USE_FLASHBOTS | USE_PUBLIC_MEMPOOL | 说明 |
|------|---------------|--------------------|----|
| Normal | false | - | 通过公开 mempool 发送 |
| Flashbots | true | false | 仅通过 Flashbots 私密发送 |
| Both | true | true | 同时发送，双保险 |

## 历史回测

使用回测工具验证策略效果：

```bash
# 下载 90 天历史数据
cargo run -p backtest -- download --days 90

# 分析套利机会
cargo run -p backtest -- analyze

# 完整流程
cargo run -p backtest -- all --days 90
```

## 技术栈

### 后端

| 类别 | 技术 |
|------|------|
| 语言 | Rust 1.75+ |
| 异步运行时 | Tokio |
| Web 框架 | Axum 0.7 |
| 数据库 | MySQL + SQLx |
| 以太坊 | ethers-rs + alloy |
| 序列化 | Serde |
| 日志 | tracing |

### 智能合约

| 类别 | 技术 |
|------|------|
| 语言 | Solidity 0.8+ |
| 框架 | Foundry |
| 闪电贷 | Uniswap V3 Flash |

### 关键依赖

```toml
tokio = "1"                    # 异步运行时
ethers = "2"                   # 以太坊交互
alloy = "0.3"                  # 现代以太坊库
axum = "0.7"                   # Web 框架
sqlx = "0.8"                   # 数据库
rust_decimal = "1.33"          # 精确计算
dashmap = "5.5"                # 并发数据结构
tracing = "0.1"                # 结构化日志
```

## 安全注意事项

1. **私钥安全** - 切勿将私钥提交到代码仓库
2. **RPC 节点** - 使用可靠的节点服务商
3. **滑点设置** - 合理设置滑点参数，避免大额损失
4. **Gas 费用** - 高 Gas 时期谨慎执行
5. **测试网优先** - 正式运行前先在测试网验证

## 风险免责声明

**重要提示：**

- 本项目仅供学习和研究目的
- 套利交易存在较高风险，可能导致资金损失
- 使用前请充分了解 DeFi 和 MEV 相关风险
- 作者不对任何因使用本软件造成的损失负责
- 请遵守当地法律法规

## 贡献指南

欢迎提交 Issue 和 Pull Request！

1. Fork 本仓库
2. 创建特性分支 (`git checkout -b feature/amazing-feature`)
3. 提交更改 (`git commit -m 'Add amazing feature'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 创建 Pull Request

### 代码规范

- 遵循 Rust 官方风格指南
- 使用 `cargo fmt` 格式化代码
- 使用 `cargo clippy` 检查代码质量
- 添加必要的测试和文档

## 许可证

本项目采用 MIT 许可证 - 详见 [LICENSE](LICENSE) 文件

## 联系方式

如有问题或建议，请通过以下方式联系：

- 提交 [Issue](https://github.com/robustfengbin/ChainFusion-Arbitrage)
- 发送邮件至 ucgygah@gmail.com

---

**声明：** 本项目不构成投资建议。加密货币交易风险极高，请谨慎投资。
