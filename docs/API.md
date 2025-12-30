# API 文档

ChainFusion Arbitrage 提供完整的 REST API 用于策略管理、交易记录查询和系统监控。

## 基础信息

- **基础 URL**: `http://localhost:9530`
- **数据格式**: JSON
- **字符编码**: UTF-8

## 响应格式

所有 API 响应均使用统一的格式：

```json
{
  "success": true,
  "data": { ... },
  "error": null
}
```

错误响应：

```json
{
  "success": false,
  "data": null,
  "error": "错误信息"
}
```

---

## 健康检查

### 检查服务状态

```
GET /health
```

**响应示例**:

```json
{
  "status": "ok",
  "timestamp": "2024-01-15T10:30:00Z"
}
```

---

## 策略管理

### 获取策略列表

```
GET /api/strategies
```

**响应示例**:

```json
{
  "success": true,
  "data": [
    {
      "id": 1,
      "name": "ETH-Stable Triangle",
      "chain_id": 1,
      "status": "running",
      "created_at": "2024-01-15T10:00:00Z"
    },
    {
      "id": 2,
      "name": "BTC-ETH Triangle",
      "chain_id": 1,
      "status": "stopped",
      "created_at": "2024-01-14T08:00:00Z"
    }
  ],
  "error": null
}
```

---

### 创建策略

```
POST /api/strategies
```

**请求体**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| name | string | 是 | 策略名称 |
| chain_id | number | 是 | 链 ID (1=Ethereum, 56=BSC) |
| min_profit_threshold_usd | number | 是 | 最低利润阈值 (USD) |
| max_slippage | number | 是 | 最大滑点 (0.0005 = 0.05%) |
| target_tokens | string[] | 是 | 目标代币地址列表 |
| target_dexes | string[] | 是 | 目标 DEX 列表 |

**请求示例**:

```json
{
  "name": "ETH-Stable Triangle",
  "chain_id": 1,
  "min_profit_threshold_usd": 10.0,
  "max_slippage": 0.0005,
  "target_tokens": [
    "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2",
    "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
    "0xdAC17F958D2ee523a2206206994597C13D831ec7"
  ],
  "target_dexes": ["uniswap_v3", "curve"]
}
```

**响应示例**:

```json
{
  "success": true,
  "data": {
    "id": 3,
    "name": "ETH-Stable Triangle",
    "chain_id": 1,
    "status": "stopped",
    "created_at": "2024-01-15T10:30:00Z"
  },
  "error": null
}
```

---

### 获取策略详情

```
GET /api/strategies/:id
```

**路径参数**:

| 参数 | 类型 | 说明 |
|------|------|------|
| id | number | 策略 ID |

**响应示例**:

```json
{
  "success": true,
  "data": {
    "id": 1,
    "name": "ETH-Stable Triangle",
    "chain_id": 1,
    "min_profit_threshold_usd": 10.0,
    "max_slippage": 0.0005,
    "target_tokens": [
      "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2",
      "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
      "0xdAC17F958D2ee523a2206206994597C13D831ec7"
    ],
    "target_dexes": ["uniswap_v3", "curve"],
    "status": "running",
    "created_at": "2024-01-15T10:00:00Z",
    "updated_at": "2024-01-15T10:30:00Z"
  },
  "error": null
}
```

---

### 更新策略

```
PUT /api/strategies/:id
```

**路径参数**:

| 参数 | 类型 | 说明 |
|------|------|------|
| id | number | 策略 ID |

**请求体**: 同创建策略

**响应示例**: 同获取策略详情

---

### 删除策略

```
DELETE /api/strategies/:id
```

**路径参数**:

| 参数 | 类型 | 说明 |
|------|------|------|
| id | number | 策略 ID |

**注意**: 运行中的策略无法删除，需先停止。

**响应示例**:

```json
{
  "success": true,
  "data": "策略已删除",
  "error": null
}
```

**错误响应**:

```json
{
  "success": false,
  "data": null,
  "error": "请先停止策略后再删除"
}
```

---

### 启动策略

```
POST /api/strategies/:id/start
```

**路径参数**:

| 参数 | 类型 | 说明 |
|------|------|------|
| id | number | 策略 ID |

**响应示例**:

```json
{
  "message": "策略 1 已启动"
}
```

---

### 停止策略

```
POST /api/strategies/:id/stop
```

**路径参数**:

| 参数 | 类型 | 说明 |
|------|------|------|
| id | number | 策略 ID |

**响应示例**:

```json
{
  "message": "策略 1 已停止"
}
```

---

### 获取运行中的策略

```
GET /api/strategies/running
```

**响应示例**:

```json
[1, 3, 5]
```

返回当前运行中的策略 ID 列表。

---

## 交易记录

### 获取交易列表

```
GET /api/trades
```

**查询参数**:

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| strategy_id | number | 否 | - | 按策略 ID 筛选 |
| limit | number | 否 | 50 | 返回数量限制 |
| offset | number | 否 | 0 | 分页偏移量 |

**请求示例**:

```
GET /api/trades?strategy_id=1&limit=20&offset=0
```

**响应示例**:

```json
{
  "success": true,
  "data": [
    {
      "id": 100,
      "strategy_id": 1,
      "tx_hash": "0x1234567890abcdef...",
      "arbitrage_type": "triangular",
      "profit_usd": 25.50,
      "gas_cost_usd": 8.20,
      "net_profit_usd": 17.30,
      "status": "confirmed",
      "created_at": "2024-01-15T10:30:00Z"
    }
  ],
  "error": null
}
```

---

### 获取交易详情

```
GET /api/trades/:id
```

**路径参数**:

| 参数 | 类型 | 说明 |
|------|------|------|
| id | number | 交易 ID |

**响应示例**:

```json
{
  "success": true,
  "data": {
    "id": 100,
    "strategy_id": 1,
    "tx_hash": "0x1234567890abcdef...",
    "arbitrage_type": "triangular",
    "profit_usd": 25.50,
    "gas_cost_usd": 8.20,
    "net_profit_usd": 17.30,
    "status": "confirmed",
    "created_at": "2024-01-15T10:30:00Z"
  },
  "error": null
}
```

---

## 统计信息

### 获取总体统计

```
GET /api/statistics
```

**响应示例**:

```json
{
  "success": true,
  "data": {
    "total_strategies": 5,
    "running_strategies": 2,
    "total_trades": 150,
    "total_profit_usd": 1250.80,
    "today_trades": 12,
    "today_profit_usd": 85.50
  },
  "error": null
}
```

**字段说明**:

| 字段 | 说明 |
|------|------|
| total_strategies | 策略总数 |
| running_strategies | 运行中的策略数 |
| total_trades | 总交易次数 |
| total_profit_usd | 累计净利润 (USD) |
| today_trades | 今日交易次数 |
| today_profit_usd | 今日净利润 (USD) |

---

### 获取策略统计

```
GET /api/statistics/:strategy_id
```

**路径参数**:

| 参数 | 类型 | 说明 |
|------|------|------|
| strategy_id | number | 策略 ID |

**响应示例**:

```json
{
  "success": true,
  "data": {
    "total_trades": 50,
    "successful_trades": 45,
    "failed_trades": 5,
    "total_profit_usd": 500.00,
    "total_gas_cost_usd": 150.00,
    "net_profit_usd": 350.00,
    "win_rate": 0.90,
    "avg_profit_per_trade": 7.78
  },
  "error": null
}
```

**字段说明**:

| 字段 | 说明 |
|------|------|
| total_trades | 总交易次数 |
| successful_trades | 成功交易次数 |
| failed_trades | 失败交易次数 |
| total_profit_usd | 总利润 (USD) |
| total_gas_cost_usd | 总 Gas 费用 (USD) |
| net_profit_usd | 净利润 (USD) |
| win_rate | 胜率 (0-1) |
| avg_profit_per_trade | 平均每笔利润 (USD) |

---

## 套利机会

### 获取套利机会列表

```
GET /api/opportunities
```

**响应示例**:

```json
{
  "success": true,
  "data": [
    {
      "id": 1,
      "path": "WETH → USDC → USDT → WETH",
      "expected_profit_usd": 15.30,
      "gas_estimate_usd": 5.50,
      "net_profit_usd": 9.80,
      "detected_at": "2024-01-15T10:30:00Z"
    }
  ],
  "error": null
}
```

---

## 系统状态

### 获取系统状态

```
GET /api/system/status
```

**响应示例**:

```json
{
  "success": true,
  "data": {
    "status": "running",
    "uptime_seconds": 3600,
    "block_number": 18500000,
    "gas_price_gwei": 25.5,
    "pending_txs": 0,
    "last_scan_at": "2024-01-15T10:30:00Z"
  },
  "error": null
}
```

---

### 获取池子列表

```
GET /api/system/pools
```

**响应示例**:

```json
{
  "success": true,
  "data": [
    {
      "address": "0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640",
      "token0": "USDC",
      "token1": "WETH",
      "fee": 500,
      "dex": "uniswap_v3",
      "liquidity": "1000000000000000000"
    }
  ],
  "error": null
}
```

---

## 错误码

| HTTP 状态码 | 说明 |
|-------------|------|
| 200 | 成功 |
| 400 | 请求参数错误 |
| 404 | 资源不存在 |
| 500 | 服务器内部错误 |

## 使用示例

### cURL

```bash
# 获取策略列表
curl -X GET http://localhost:9530/api/strategies

# 创建策略
curl -X POST http://localhost:9530/api/strategies \
  -H "Content-Type: application/json" \
  -d '{
    "name": "ETH Triangle",
    "chain_id": 1,
    "min_profit_threshold_usd": 10.0,
    "max_slippage": 0.0005,
    "target_tokens": ["0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"],
    "target_dexes": ["uniswap_v3"]
  }'

# 启动策略
curl -X POST http://localhost:9530/api/strategies/1/start

# 获取统计信息
curl -X GET http://localhost:9530/api/statistics
```

### Python

```python
import requests

BASE_URL = "http://localhost:9530"

# 获取策略列表
response = requests.get(f"{BASE_URL}/api/strategies")
strategies = response.json()

# 创建策略
new_strategy = {
    "name": "ETH Triangle",
    "chain_id": 1,
    "min_profit_threshold_usd": 10.0,
    "max_slippage": 0.0005,
    "target_tokens": ["0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"],
    "target_dexes": ["uniswap_v3"]
}
response = requests.post(f"{BASE_URL}/api/strategies", json=new_strategy)

# 启动策略
requests.post(f"{BASE_URL}/api/strategies/1/start")

# 获取交易记录
trades = requests.get(f"{BASE_URL}/api/trades", params={"limit": 20}).json()
```

### JavaScript

```javascript
const BASE_URL = 'http://localhost:9530';

// 获取策略列表
const strategies = await fetch(`${BASE_URL}/api/strategies`)
  .then(res => res.json());

// 创建策略
const newStrategy = await fetch(`${BASE_URL}/api/strategies`, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    name: 'ETH Triangle',
    chain_id: 1,
    min_profit_threshold_usd: 10.0,
    max_slippage: 0.0005,
    target_tokens: ['0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2'],
    target_dexes: ['uniswap_v3']
  })
}).then(res => res.json());

// 启动策略
await fetch(`${BASE_URL}/api/strategies/1/start`, { method: 'POST' });
```
