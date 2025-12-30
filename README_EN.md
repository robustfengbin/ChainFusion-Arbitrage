# ChainFusion Arbitrage

**DEX Triangular Arbitrage Bot** - High-performance decentralized exchange arbitrage system built with Rust

A professional-grade automated arbitrage bot that monitors multiple DEX liquidity pools in real-time, detecting and executing triangular arbitrage strategies. Features flash loans, Flashbots MEV protection, multi-chain deployment, and other enterprise-level capabilities.

## Key Features

- **Real-time Arbitrage Detection** - Millisecond-level monitoring of Swap events across 14 major pools
- **Triangular Arbitrage Strategy** - 6 pre-configured high-liquidity triangular combinations with 26 arbitrage paths
- **Flash Loan Support** - Zero initial capital required, supports Uniswap V3/V4, Aave, Balancer
- **MEV Protection** - Integrated Flashbots to prevent sandwich attacks and front-running
- **Multi-DEX Support** - Uniswap V2/V3/V4, Curve, PancakeSwap, SushiSwap
- **Multi-Chain Support** - Ethereum, BSC, Polygon, Arbitrum, Base, Optimism, Solana
- **REST API** - Complete strategy management and monitoring interface
- **Historical Backtesting** - Download historical data for strategy validation

## Quick Start

### Requirements

- Rust 1.75+
- MySQL 8.0+
- Node.js 18+ (optional, for PM2 process management)

### Installation

```bash
# Clone repository
git clone https://github.com/robustfengbin/ChainFusion-Arbitrage.git
cd ChainFusion-Arbitrage

# Build project
cargo build --release
```

### Configuration

```bash
# Copy configuration template
cp .env.example .env

# Edit configuration file
vim .env
```

Key configuration items:

```bash
# Database
DB_HOST=127.0.0.1
DB_PORT=3306
DB_USER=root
DB_PASSWORD=your_password
DB_NAME=dex_arbitrage

# RPC Node (Alchemy or Infura recommended)
ETH_RPC_URL=https://eth-mainnet.g.alchemy.com/v2/your-api-key
ETH_WS_URL=wss://eth-mainnet.g.alchemy.com/v2/your-api-key

# Wallet Private Key (Keep it safe!)
PRIVATE_KEY=your_private_key_here

# Arbitrage Parameters
MIN_PROFIT_THRESHOLD=10.0  # Minimum profit $10
MAX_SLIPPAGE=0.0005        # Maximum slippage 0.05%
```

### Running

```bash
# Direct run
cargo run --release -p main

# Or use PM2 (recommended for production)
chmod +x pm2.sh
./pm2.sh
```

## System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      ChainFusion Arbitrage                       │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐           │
│  │  Block Sub   │  │ Pool Syncer  │  │Price Service │           │
│  │  Subscriber  │  │              │  │              │           │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘           │
│         │                  │                  │                   │
│         ▼                  ▼                  ▼                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                 Event Driven Scanner                      │   │
│  │  • Monitor Swap events from 14 major pools               │   │
│  │  • Local fast estimation + on-chain precise quotes       │   │
│  │  • Real-time calculation of 26 path opportunities        │   │
│  └──────────────────────────┬───────────────────────────────┘   │
│                              │                                    │
│                              ▼                                    │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                  Arbitrage Executor                       │   │
│  │  • Normal: Send via public mempool                       │   │
│  │  • Flashbots: Private bundle submission                  │   │
│  │  • Both: Dual submission for redundancy                  │   │
│  └──────────────────────────┬───────────────────────────────┘   │
│                              │                                    │
│                              ▼                                    │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                 Smart Contract Execution                  │   │
│  │                   FlashArbitrage.sol                      │   │
│  │  • Uniswap V3 Flash Loan                                 │   │
│  │  • Atomic Triangular Arbitrage                           │   │
│  │  • Automatic Profit Extraction                           │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

## Triangular Arbitrage Combinations

The system comes pre-configured with 6 high-liquidity triangular arbitrage combinations:

| Priority | Combination | Type | Description |
|----------|-------------|------|-------------|
| 10 | DAI-USDC-USDT | Stablecoin | Lowest fees (0.01%), minimal slippage |
| 20 | USDC-WETH-USDT | ETH-Stablecoin | Triggered by large ETH trades, high frequency |
| 30 | DAI-USDC-WETH | DAI-ETH | Suitable for event-driven strategies |
| 40 | WBTC-USDC-USDT | BTC-Stablecoin | Higher per-trade profit, lower frequency |
| 50 | WBTC-WETH-USDC | BTC-ETH-USDC | CEX-DEX sync delay opportunities |
| 60 | WBTC-WETH-USDT | BTC-ETH-USDT | Appears during volatile markets |

### Arbitrage Path Example

```
Forward: DAI → USDC → USDT → DAI
         │        │        │
         └─ 0.01% ─┴─ 0.01% ─┴─ 0.01%

Reverse: DAI → USDT → USDC → DAI
```

## Project Structure

```
chainfusion-arbitrage/
├── crates/
│   ├── main/           # Main application entry
│   ├── models/         # Data models (Token, Pool, ArbitragePath)
│   ├── config/         # Configuration management
│   ├── services/       # Core services
│   │   ├── block_subscriber.rs    # Block subscription
│   │   ├── pool_syncer.rs         # Pool synchronization
│   │   ├── price_service.rs       # Price service
│   │   └── database.rs            # Database operations
│   ├── strategies/     # Arbitrage strategies (Core!)
│   │   ├── event_driven_scanner.rs  # Event-driven scanning
│   │   ├── arbitrage_executor.rs    # Arbitrage execution
│   │   ├── profit_calculator.rs     # Profit calculation
│   │   └── path_finder.rs           # Path finding
│   ├── executor/       # Transaction executor
│   │   ├── executor.rs           # Main executor
│   │   ├── flash_arbitrage.rs    # Flash loan execution
│   │   └── flashbots/            # Flashbots integration
│   ├── dex/            # DEX integrations
│   │   ├── uniswap/    # Uniswap V2/V3/V4
│   │   ├── curve/      # Curve
│   │   ├── pancakeswap/# PancakeSwap
│   │   └── flashloan/  # Flash loan providers
│   ├── api/            # REST API (Axum)
│   ├── backtest/       # Historical backtesting tools
│   ├── solana/         # Solana chain support
│   └── utils/          # Utility library
├── docs/               # Documentation
├── scripts/            # Script tools
└── .env.example        # Configuration template
```

## API Endpoints

The system provides a complete REST API for strategy management and monitoring:

### Health Check

```bash
GET /health
```

### Strategy Management

```bash
# List all strategies
GET /api/strategies

# Create strategy
POST /api/strategies
Content-Type: application/json
{
  "name": "ETH-Stable Triangle",
  "min_profit_usd": 10.0,
  "max_slippage": 0.0005
}

# Start/Stop strategy
POST /api/strategies/:id/start
POST /api/strategies/:id/stop

# View running strategies
GET /api/strategies/running
```

### Trade Records

```bash
# Trade list
GET /api/trades

# Trade details
GET /api/trades/:id
```

### Statistics

```bash
# System statistics
GET /api/statistics

# Strategy statistics
GET /api/statistics/:strategy_id
```

### System Status

```bash
# System status
GET /api/system/status

# Pool list
GET /api/system/pools

# Arbitrage opportunities
GET /api/opportunities
```

## Configuration Reference

### Complete Configuration Items

```bash
# ============================
# Database Configuration
# ============================
DB_HOST=127.0.0.1
DB_PORT=3306
DB_USER=root
DB_PASSWORD=your_password_here
DB_NAME=dex_arbitrage
DB_MAX_CONNECTIONS=10

# ============================
# RPC Configuration
# ============================
# Ethereum Mainnet
ETH_RPC_URL=https://eth-mainnet.g.alchemy.com/v2/your-api-key
ETH_WS_URL=wss://eth-mainnet.g.alchemy.com/v2/your-api-key

# BSC Mainnet
BSC_RPC_URL=https://bsc-dataseed1.binance.org
BSC_WS_URL=wss://bsc-ws-node.nariox.org:443

# ============================
# Arbitrage Parameters
# ============================
MAX_SLIPPAGE=0.0005           # Max slippage 0.05%
MIN_PROFIT_THRESHOLD=10.0     # Min profit threshold $10
MAX_PATH_HOPS=3               # Max path hops
GAS_PRICE_MULTIPLIER=1.2      # Gas price multiplier

# ============================
# Flash Loan Configuration
# ============================
# Supported: uniswap_v3, uniswap_v4, aave, balancer
FLASH_LOAN_PROVIDER=uniswap_v3

# ============================
# MEV Protection
# ============================
USE_FLASHBOTS=false
FLASHBOTS_RPC_URL=https://relay.flashbots.net
USE_PUBLIC_MEMPOOL=false

# ============================
# Wallet Configuration
# ============================
PRIVATE_KEY=your_private_key_here

# ============================
# Server Configuration
# ============================
SERVER_PORT=9530
SERVER_HOST=0.0.0.0

# ============================
# Logging Configuration
# ============================
RUST_LOG=info
LOG_FILE_PATH=./logs/dex_arbitrage.log
```

### Execution Modes

| Mode | USE_FLASHBOTS | USE_PUBLIC_MEMPOOL | Description |
|------|---------------|--------------------|----|
| Normal | false | - | Send via public mempool |
| Flashbots | true | false | Private Flashbots only |
| Both | true | true | Dual submission for redundancy |

## Historical Backtesting

Use the backtesting tool to validate strategy performance:

```bash
# Download 90 days of historical data
cargo run -p backtest -- download --days 90

# Analyze arbitrage opportunities
cargo run -p backtest -- analyze

# Complete workflow
cargo run -p backtest -- all --days 90
```

## Tech Stack

### Backend

| Category | Technology |
|----------|------------|
| Language | Rust 1.75+ |
| Async Runtime | Tokio |
| Web Framework | Axum 0.7 |
| Database | MySQL + SQLx |
| Ethereum | ethers-rs + alloy |
| Serialization | Serde |
| Logging | tracing |

### Smart Contracts

| Category | Technology |
|----------|------------|
| Language | Solidity 0.8+ |
| Framework | Foundry |
| Flash Loan | Uniswap V3 Flash |

### Key Dependencies

```toml
tokio = "1"                    # Async runtime
ethers = "2"                   # Ethereum interaction
alloy = "0.3"                  # Modern Ethereum library
axum = "0.7"                   # Web framework
sqlx = "0.8"                   # Database
rust_decimal = "1.33"          # Precise calculations
dashmap = "5.5"                # Concurrent data structures
tracing = "0.1"                # Structured logging
```

## Security Considerations

1. **Private Key Security** - Never commit private keys to the repository
2. **RPC Nodes** - Use reliable node service providers
3. **Slippage Settings** - Set reasonable slippage parameters to avoid large losses
4. **Gas Fees** - Exercise caution during high gas periods
5. **Testnet First** - Always validate on testnet before mainnet deployment

## Risk Disclaimer

**Important Notice:**

- This project is for educational and research purposes only
- Arbitrage trading carries significant risks and may result in financial losses
- Please fully understand DeFi and MEV-related risks before use
- The authors are not responsible for any losses incurred from using this software
- Please comply with local laws and regulations

## Contributing

Issues and Pull Requests are welcome!

1. Fork this repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Create a Pull Request

### Code Standards

- Follow Rust official style guidelines
- Use `cargo fmt` to format code
- Use `cargo clippy` to check code quality
- Add necessary tests and documentation

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details

## Contact

For questions or suggestions, please contact us through:

- Submit an [Issue](https://github.com/robustfengbin/ChainFusion-Arbitrage/issues)
- Email: ucgygah@gmail.com

---

**Disclaimer:** This project does not constitute investment advice. Cryptocurrency trading carries extremely high risks. Please invest cautiously.
