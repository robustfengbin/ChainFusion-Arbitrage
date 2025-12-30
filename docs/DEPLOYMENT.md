# 部署指南

本文档详细介绍 ChainFusion Arbitrage 的部署流程，包括本地开发环境和生产环境的配置。

## 目录

- [环境要求](#环境要求)
- [本地开发环境](#本地开发环境)
- [生产环境部署](#生产环境部署)
- [数据库配置](#数据库配置)
- [RPC 节点配置](#rpc-节点配置)
- [智能合约部署](#智能合约部署)
- [监控与日志](#监控与日志)
- [故障排查](#故障排查)

---

## 环境要求

### 软件要求

| 软件 | 版本 | 说明 |
|------|------|------|
| Rust | 1.75+ | 编程语言 |
| MySQL | 8.0+ | 数据库 |
| Node.js | 18+ | 可选，用于 PM2 |
| Git | 2.0+ | 版本控制 |

### 硬件建议

**最低配置**:
- CPU: 2 核
- 内存: 4 GB
- 硬盘: 20 GB SSD
- 网络: 稳定的互联网连接

**推荐配置** (生产环境):
- CPU: 4 核+
- 内存: 8 GB+
- 硬盘: 50 GB SSD
- 网络: 低延迟专线

---

## 本地开发环境

### 1. 安装 Rust

```bash
# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 配置环境变量
source $HOME/.cargo/env

# 验证安装
rustc --version
cargo --version
```

### 2. 安装 MySQL

**macOS**:
```bash
brew install mysql
brew services start mysql
```

**Ubuntu**:
```bash
sudo apt update
sudo apt install mysql-server
sudo systemctl start mysql
```

### 3. 克隆项目

```bash
git clone https://github.com/your-username/chainfusion-arbitrage.git
cd chainfusion-arbitrage
```

### 4. 配置环境变量

```bash
# 复制配置模板
cp .env.example .env

# 编辑配置
vim .env
```

必须配置的项目：

```bash
# 数据库
DB_HOST=127.0.0.1
DB_PORT=3306
DB_USER=root
DB_PASSWORD=your_password
DB_NAME=dex_arbitrage

# RPC 节点
ETH_RPC_URL=https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY
ETH_WS_URL=wss://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY

# 钱包私钥 (开发环境可使用测试钱包)
PRIVATE_KEY=your_private_key
```

### 5. 初始化数据库

```bash
# 登录 MySQL
mysql -u root -p

# 创建数据库
CREATE DATABASE dex_arbitrage CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

# 退出
exit
```

程序首次运行时会自动创建表结构。

### 6. 编译运行

```bash
# 开发模式编译
cargo build

# 运行
cargo run -p main

# 或者 release 模式
cargo build --release
./target/release/chainfusion_arbitrage
```

---

## 生产环境部署

### 1. 服务器准备

```bash
# 更新系统
sudo apt update && sudo apt upgrade -y

# 安装依赖
sudo apt install -y build-essential pkg-config libssl-dev
```

### 2. 安装 Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

### 3. 安装 MySQL

```bash
sudo apt install -y mysql-server
sudo systemctl enable mysql
sudo systemctl start mysql

# 安全配置
sudo mysql_secure_installation
```

### 4. 部署应用

```bash
# 克隆代码
git clone https://github.com/your-username/chainfusion-arbitrage.git
cd chainfusion-arbitrage

# 配置环境变量
cp .env.example .env
vim .env

# 编译 release 版本
cargo build --release
```

### 5. 使用 PM2 管理进程

```bash
# 安装 Node.js 和 PM2
curl -fsSL https://deb.nodesource.com/setup_18.x | sudo -E bash -
sudo apt install -y nodejs
sudo npm install -g pm2

# 使用启动脚本
chmod +x pm2.sh
./pm2.sh

# 或手动启动
pm2 start target/release/chainfusion_arbitrage --name arbitrage

# 设置开机自启
pm2 startup
pm2 save
```

### 6. 配置 Systemd (替代方案)

```bash
# 创建 service 文件
sudo vim /etc/systemd/system/arbitrage.service
```

内容：

```ini
[Unit]
Description=ChainFusion Arbitrage Bot
After=network.target mysql.service

[Service]
Type=simple
User=ubuntu
WorkingDirectory=/home/ubuntu/chainfusion-arbitrage
Environment="RUST_LOG=info"
EnvironmentFile=/home/ubuntu/chainfusion-arbitrage/.env
ExecStart=/home/ubuntu/chainfusion-arbitrage/target/release/chainfusion_arbitrage
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

启动服务：

```bash
sudo systemctl daemon-reload
sudo systemctl enable arbitrage
sudo systemctl start arbitrage
sudo systemctl status arbitrage
```

---

## 数据库配置

### 表结构

程序会自动创建以下表：

| 表名 | 说明 |
|------|------|
| arbitrage_tokens | 代币配置 |
| arbitrage_pools | 流动性池配置 |
| arbitrage_triangles | 三角套利组合 |
| arbitrage_strategies | 策略配置 |
| trade_records | 交易记录 |
| strategy_statistics | 策略统计 |

### 初始化数据

系统启动时会自动初始化 14 个主力池子和 6 个三角套利组合。

### 数据库备份

```bash
# 备份
mysqldump -u root -p dex_arbitrage > backup_$(date +%Y%m%d).sql

# 恢复
mysql -u root -p dex_arbitrage < backup_20240115.sql
```

---

## RPC 节点配置

### 推荐节点服务商

| 服务商 | 说明 | 免费额度 |
|--------|------|----------|
| [Alchemy](https://www.alchemy.com/) | 稳定可靠，推荐 | 3亿 CU/月 |
| [Infura](https://infura.io/) | 老牌服务商 | 10万请求/天 |
| [QuickNode](https://www.quicknode.com/) | 低延迟 | 付费 |

### 配置多个 RPC

建议配置多个 RPC 节点作为备用：

```bash
# 主节点
ETH_RPC_URL=https://eth-mainnet.g.alchemy.com/v2/key1
ETH_WS_URL=wss://eth-mainnet.g.alchemy.com/v2/key1

# 备用节点 (可在代码中配置)
ETH_RPC_URL_BACKUP=https://mainnet.infura.io/v3/key2
```

### WebSocket 连接

套利检测依赖 WebSocket 实时订阅区块事件，确保：

1. WebSocket URL 正确配置
2. 网络防火墙允许 WebSocket 连接
3. 节点服务商支持 WebSocket

---

## 智能合约部署

### 使用 Foundry 部署

```bash
# 安装 Foundry
curl -L https://foundry.paradigm.xyz | bash
foundryup

# 进入合约目录
cd crates/executor/contracts

# 编译合约
forge build

# 部署到测试网 (Goerli)
forge create --rpc-url $ETH_RPC_URL \
  --private-key $PRIVATE_KEY \
  src/FlashArbitrage.sol:FlashArbitrage

# 部署到主网
forge create --rpc-url $ETH_RPC_URL \
  --private-key $PRIVATE_KEY \
  --verify \
  src/FlashArbitrage.sol:FlashArbitrage
```

### 合约验证

```bash
forge verify-contract \
  --chain-id 1 \
  --compiler-version v0.8.19 \
  $CONTRACT_ADDRESS \
  src/FlashArbitrage.sol:FlashArbitrage
```

### 更新配置

部署完成后，更新 `.env`：

```bash
ARBITRAGE_CONTRACT_ADDRESS=0x...your_contract_address
```

---

## 监控与日志

### 日志配置

```bash
# .env 中配置
RUST_LOG=info
LOG_FILE_PATH=./logs/arbitrage.log
```

日志级别：
- `error`: 仅错误
- `warn`: 警告和错误
- `info`: 常规信息 (推荐)
- `debug`: 调试信息
- `trace`: 详细追踪

### 查看日志

```bash
# 实时查看
tail -f logs/arbitrage.log

# 搜索错误
grep "ERROR" logs/arbitrage.log

# PM2 日志
pm2 logs arbitrage
```

### 健康检查

```bash
# 检查服务状态
curl http://localhost:9530/health

# 检查系统状态
curl http://localhost:9530/api/system/status
```

### 告警配置

配置邮件通知：

```bash
# .env
SMTP_HOST=smtp.gmail.com
SMTP_PORT=587
SMTP_USER=your@email.com
SMTP_PASSWORD=your_app_password
ALERT_EMAIL=alert@email.com
```

---

## 故障排查

### 常见问题

#### 1. 数据库连接失败

```
Error: Failed to connect to database
```

**解决方案**:
- 检查 MySQL 服务是否运行：`sudo systemctl status mysql`
- 验证数据库配置：`mysql -u root -p`
- 检查防火墙设置

#### 2. RPC 连接超时

```
Error: Provider error: request timeout
```

**解决方案**:
- 检查网络连接
- 更换 RPC 节点
- 检查节点配额是否用尽

#### 3. WebSocket 断开

```
Error: WebSocket connection closed
```

**解决方案**:
- 程序会自动重连
- 检查网络稳定性
- 确认 WebSocket URL 正确

#### 4. 编译错误

```
Error: failed to compile
```

**解决方案**:
- 更新 Rust：`rustup update`
- 清理缓存：`cargo clean`
- 检查依赖：`cargo update`

### 性能优化

1. **使用 Release 模式**
   ```bash
   cargo build --release
   ```

2. **优化数据库连接池**
   ```bash
   DB_MAX_CONNECTIONS=20
   ```

3. **调整日志级别**
   ```bash
   RUST_LOG=warn  # 生产环境
   ```

4. **使用低延迟 RPC**
   - 选择地理位置近的节点
   - 考虑专用节点服务

### 获取帮助

如遇到问题，请：

1. 查看日志文件获取详细错误信息
2. 搜索 [Issues](https://github.com/your-username/chainfusion-arbitrage/issues)
3. 提交新的 Issue，附上：
   - 操作系统版本
   - Rust 版本
   - 完整错误日志
   - 重现步骤
