# 三角套利路径配置文档

## 概述

本系统配置了 **6 个三角套利组合**，使用 **14 个 Uniswap V3 池子**，每次套利检测会检查 **26 条路径**。

## 代币配置

| 代币 | 地址 | 精度 | 类型 |
|------|------|------|------|
| WETH | 0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2 | 18 | 主流币 |
| WBTC | 0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599 | 8 | 主流币 |
| USDC | 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48 | 6 | 稳定币 |
| USDT | 0xdAC17F958D2ee523a2206206994597C13D831ec7 | 6 | 稳定币 |
| DAI  | 0x6B175474E89094C44Da98b954EedeAC495271d0F | 18 | 稳定币 |

---

## 6 个三角套利组合

### 1. DAI-USDC-USDT (稳定币三角)
- **优先级**: 10 (最高)
- **类型**: stablecoin
- **说明**: 稳定币三角，手续费最低(0.01%)，滑点极低，练功房+主阵地

**路径方向**:
```
正向: DAI → USDC → USDT → DAI
反向: DAI → USDT → USDC → DAI
```

**涉及池子**:
| 交易对 | 池子地址 | 手续费 |
|--------|----------|--------|
| DAI/USDC | 0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168 | 0.01% |
| DAI/USDC | 0x6c6Bc977E13Df9b0de53b251522280BB72383700 | 0.05% |
| DAI/USDT | 0x6f48ECa74B38d2936B02ab603FF4e36A6C0E3A77 | 0.01% |
| USDC/USDT | 0x3416cF6C708Da44DB2624D63ea0AAef7113527C6 | 0.01% |

---

### 2. USDC-WETH-USDT (ETH-稳定币三角)
- **优先级**: 20
- **类型**: eth_stable
- **说明**: WETH-USDT高波动边，USDC-USDT锚定边，大ETH成交触发

**路径方向**:
```
正向: USDC → WETH → USDT → USDC
反向: USDC → USDT → WETH → USDC
```

**涉及池子**:
| 交易对 | 池子地址 | 手续费 |
|--------|----------|--------|
| USDC/WETH | 0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640 | 0.05% |
| USDC/WETH | 0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8 | 0.30% |
| WETH/USDT | 0x11b815efB8f581194ae79006d24E0d814B7697F6 | 0.05% |
| WETH/USDT | 0x4e68Ccd3E89f51C3074ca5072bBaC773960dFa36 | 0.30% |
| USDC/USDT | 0x3416cF6C708Da44DB2624D63ea0AAef7113527C6 | 0.01% |

---

### 3. DAI-USDC-WETH (DAI-ETH-稳定币三角)
- **优先级**: 30
- **类型**: eth_stable
- **说明**: 稳定币+ETH，fee极低，适合event-driven

**路径方向**:
```
正向: DAI → USDC → WETH → DAI
反向: DAI → WETH → USDC → DAI
```

**涉及池子**:
| 交易对 | 池子地址 | 手续费 |
|--------|----------|--------|
| DAI/USDC | 0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168 | 0.01% |
| DAI/USDC | 0x6c6Bc977E13Df9b0de53b251522280BB72383700 | 0.05% |
| USDC/WETH | 0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640 | 0.05% |
| USDC/WETH | 0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8 | 0.30% |
| DAI/WETH | 0x60594a405d53811d3BC4766596EFD80fd545A270 | 0.05% |
| DAI/WETH | 0xC2e9F25Be6257c210d7Adf0D4Cd6E3E881ba25f8 | 0.30% |

---

### 4. WBTC-USDC-USDT (BTC-稳定币三角)
- **优先级**: 40
- **类型**: btc_stable
- **说明**: BTC大资金成交触发，单次利润可能更高，频率低于ETH

**路径方向**:
```
正向: WBTC → USDC → USDT → WBTC
反向: WBTC → USDT → USDC → WBTC
```

**涉及池子**:
| 交易对 | 池子地址 | 手续费 |
|--------|----------|--------|
| WBTC/USDC | 0x99ac8cA7087fA4A2A1FB6357269965A2014ABc35 | 0.30% |
| WBTC/USDT | 0x9Db9e0e53058C89e5B94e29621a205198648425B | 0.30% |
| USDC/USDT | 0x3416cF6C708Da44DB2624D63ea0AAef7113527C6 | 0.01% |

---

### 5. WBTC-WETH-USDC (BTC-ETH-USDC三角)
- **优先级**: 50
- **类型**: major
- **说明**: BTC-ETH汇率错位，CEX-DEX同步延迟，职业套利常规路径

**路径方向**:
```
正向: WBTC → WETH → USDC → WBTC
反向: WBTC → USDC → WETH → WBTC
```

**涉及池子**:
| 交易对 | 池子地址 | 手续费 |
|--------|----------|--------|
| WBTC/WETH | 0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0 | 0.05% |
| WBTC/WETH | 0xCBCdF9626bC03E24f779434178A73a0B4bad62eD | 0.30% |
| USDC/WETH | 0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640 | 0.05% |
| USDC/WETH | 0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8 | 0.30% |
| WBTC/USDC | 0x99ac8cA7087fA4A2A1FB6357269965A2014ABc35 | 0.30% |

---

### 6. WBTC-WETH-USDT (BTC-ETH-USDT三角)
- **优先级**: 60
- **类型**: major
- **说明**: 波动性最大，Gas与fee更敏感，通常只在剧烈行情出现

**路径方向**:
```
正向: WBTC → WETH → USDT → WBTC
反向: WBTC → USDT → WETH → WBTC
```

**涉及池子**:
| 交易对 | 池子地址 | 手续费 |
|--------|----------|--------|
| WBTC/WETH | 0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0 | 0.05% |
| WBTC/WETH | 0xCBCdF9626bC03E24f779434178A73a0B4bad62eD | 0.30% |
| WETH/USDT | 0x11b815efB8f581194ae79006d24E0d814B7697F6 | 0.05% |
| WETH/USDT | 0x4e68Ccd3E89f51C3074ca5072bBaC773960dFa36 | 0.30% |
| WBTC/USDT | 0x9Db9e0e53058C89e5B94e29621a205198648425B | 0.30% |

---

## 14 个监控池子及其触发的路径

### 池子 1: DAI/USDC (0.01%)
- **地址**: `0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168`
- **手续费**: 100 (0.01%)

**触发的套利路径** (2条):
| # | 路径 | 三角组合 |
|---|------|----------|
| 1 | DAI → USDC → USDT → DAI | DAI-USDC-USDT |
| 2 | DAI → USDC → WETH → DAI | DAI-USDC-WETH |

---

### 池子 2: DAI/USDC (0.05%)
- **地址**: `0x6c6Bc977E13Df9b0de53b251522280BB72383700`
- **手续费**: 500 (0.05%)

**触发的套利路径** (2条):
| # | 路径 | 三角组合 |
|---|------|----------|
| 1 | DAI → USDC → USDT → DAI | DAI-USDC-USDT |
| 2 | DAI → USDC → WETH → DAI | DAI-USDC-WETH |

---

### 池子 3: DAI/USDT (0.01%)
- **地址**: `0x6f48ECa74B38d2936B02ab603FF4e36A6C0E3A77`
- **手续费**: 100 (0.01%)

**触发的套利路径** (1条):
| # | 路径 | 三角组合 |
|---|------|----------|
| 1 | DAI → USDT → USDC → DAI | DAI-USDC-USDT |

---

### 池子 4: USDC/USDT (0.01%)
- **地址**: `0x3416cF6C708Da44DB2624D63ea0AAef7113527C6`
- **手续费**: 100 (0.01%)

**触发的套利路径** (3条):
| # | 路径 | 三角组合 |
|---|------|----------|
| 1 | USDC → USDT → DAI → USDC | DAI-USDC-USDT |
| 2 | USDC → USDT → WETH → USDC | USDC-WETH-USDT |
| 3 | USDC → USDT → WBTC → USDC | WBTC-USDC-USDT |

---

### 池子 5: USDC/WETH (0.05%) - 主力池
- **地址**: `0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640`
- **手续费**: 500 (0.05%)

**触发的套利路径** (4条):
| # | 路径 | 三角组合 |
|---|------|----------|
| 1 | USDC → WETH → USDT → USDC | USDC-WETH-USDT |
| 2 | USDC → WETH → DAI → USDC | DAI-USDC-WETH |
| 3 | USDC → WETH → WBTC → USDC | WBTC-WETH-USDC |
| 4 | WETH → USDC → USDT → WETH | USDC-WETH-USDT |

---

### 池子 6: USDC/WETH (0.30%)
- **地址**: `0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8`
- **手续费**: 3000 (0.30%)

**触发的套利路径** (4条):
| # | 路径 | 三角组合 |
|---|------|----------|
| 1 | USDC → WETH → USDT → USDC | USDC-WETH-USDT |
| 2 | USDC → WETH → DAI → USDC | DAI-USDC-WETH |
| 3 | USDC → WETH → WBTC → USDC | WBTC-WETH-USDC |
| 4 | WETH → USDC → USDT → WETH | USDC-WETH-USDT |

---

### 池子 7: WETH/USDT (0.05%)
- **地址**: `0x11b815efB8f581194ae79006d24E0d814B7697F6`
- **手续费**: 500 (0.05%)

**触发的套利路径** (3条):
| # | 路径 | 三角组合 |
|---|------|----------|
| 1 | WETH → USDT → USDC → WETH | USDC-WETH-USDT |
| 2 | WETH → USDT → WBTC → WETH | WBTC-WETH-USDT |
| 3 | USDT → WETH → USDC → USDT | USDC-WETH-USDT |

---

### 池子 8: WETH/USDT (0.30%)
- **地址**: `0x4e68Ccd3E89f51C3074ca5072bBaC773960dFa36`
- **手续费**: 3000 (0.30%)

**触发的套利路径** (3条):
| # | 路径 | 三角组合 |
|---|------|----------|
| 1 | WETH → USDT → USDC → WETH | USDC-WETH-USDT |
| 2 | WETH → USDT → WBTC → WETH | WBTC-WETH-USDT |
| 3 | USDT → WETH → USDC → USDT | USDC-WETH-USDT |

---

### 池子 9: DAI/WETH (0.05%)
- **地址**: `0x60594a405d53811d3BC4766596EFD80fd545A270`
- **手续费**: 500 (0.05%)

**触发的套利路径** (2条):
| # | 路径 | 三角组合 |
|---|------|----------|
| 1 | DAI → WETH → USDC → DAI | DAI-USDC-WETH |
| 2 | WETH → DAI → USDC → WETH | DAI-USDC-WETH |

---

### 池子 10: DAI/WETH (0.30%)
- **地址**: `0xC2e9F25Be6257c210d7Adf0D4Cd6E3E881ba25f8`
- **手续费**: 3000 (0.30%)

**触发的套利路径** (2条):
| # | 路径 | 三角组合 |
|---|------|----------|
| 1 | DAI → WETH → USDC → DAI | DAI-USDC-WETH |
| 2 | WETH → DAI → USDC → WETH | DAI-USDC-WETH |

---

### 池子 11: WBTC/USDC (0.30%)
- **地址**: `0x99ac8cA7087fA4A2A1FB6357269965A2014ABc35`
- **手续费**: 3000 (0.30%)

**触发的套利路径** (2条):
| # | 路径 | 三角组合 |
|---|------|----------|
| 1 | WBTC → USDC → USDT → WBTC | WBTC-USDC-USDT |
| 2 | WBTC → USDC → WETH → WBTC | WBTC-WETH-USDC |

---

### 池子 12: WBTC/USDT (0.30%)
- **地址**: `0x9Db9e0e53058C89e5B94e29621a205198648425B`
- **手续费**: 3000 (0.30%)

**触发的套利路径** (2条):
| # | 路径 | 三角组合 |
|---|------|----------|
| 1 | WBTC → USDT → USDC → WBTC | WBTC-USDC-USDT |
| 2 | WBTC → USDT → WETH → WBTC | WBTC-WETH-USDT |

---

### 池子 13: WBTC/WETH (0.05%) - 主力池
- **地址**: `0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0`
- **手续费**: 500 (0.05%)

**触发的套利路径** (4条):
| # | 路径 | 三角组合 |
|---|------|----------|
| 1 | WBTC → WETH → USDC → WBTC | WBTC-WETH-USDC |
| 2 | WBTC → WETH → USDT → WBTC | WBTC-WETH-USDT |
| 3 | WETH → WBTC → USDC → WETH | WBTC-WETH-USDC |
| 4 | WETH → WBTC → USDT → WETH | WBTC-WETH-USDT |

---

### 池子 14: WBTC/WETH (0.30%)
- **地址**: `0xCBCdF9626bC03E24f779434178A73a0B4bad62eD`
- **手续费**: 3000 (0.30%)

**触发的套利路径** (4条):
| # | 路径 | 三角组合 |
|---|------|----------|
| 1 | WBTC → WETH → USDC → WBTC | WBTC-WETH-USDC |
| 2 | WBTC → WETH → USDT → WBTC | WBTC-WETH-USDT |
| 3 | WETH → WBTC → USDC → WETH | WBTC-WETH-USDC |
| 4 | WETH → WBTC → USDT → WETH | WBTC-WETH-USDT |

---

## 完整 26 条路径汇总

以下是系统检测的全部 26 条套利路径（每个三角组合的正向和反向，考虑不同池子组合）:

### 稳定币三角 (DAI-USDC-USDT)
| # | 路径 | 起始池 | 第二池 | 第三池 |
|---|------|--------|--------|--------|
| 1 | DAI → USDC → USDT → DAI | DAI/USDC | USDC/USDT | DAI/USDT |
| 2 | DAI → USDT → USDC → DAI | DAI/USDT | USDC/USDT | DAI/USDC |
| 3 | USDC → DAI → USDT → USDC | DAI/USDC | DAI/USDT | USDC/USDT |
| 4 | USDC → USDT → DAI → USDC | USDC/USDT | DAI/USDT | DAI/USDC |

### ETH-稳定币三角 (USDC-WETH-USDT)
| # | 路径 | 起始池 | 第二池 | 第三池 |
|---|------|--------|--------|--------|
| 5 | USDC → WETH → USDT → USDC | USDC/WETH | WETH/USDT | USDC/USDT |
| 6 | USDC → USDT → WETH → USDC | USDC/USDT | WETH/USDT | USDC/WETH |
| 7 | WETH → USDC → USDT → WETH | USDC/WETH | USDC/USDT | WETH/USDT |
| 8 | WETH → USDT → USDC → WETH | WETH/USDT | USDC/USDT | USDC/WETH |
| 9 | USDT → WETH → USDC → USDT | WETH/USDT | USDC/WETH | USDC/USDT |
| 10 | USDT → USDC → WETH → USDT | USDC/USDT | USDC/WETH | WETH/USDT |

### DAI-ETH-稳定币三角 (DAI-USDC-WETH)
| # | 路径 | 起始池 | 第二池 | 第三池 |
|---|------|--------|--------|--------|
| 11 | DAI → USDC → WETH → DAI | DAI/USDC | USDC/WETH | DAI/WETH |
| 12 | DAI → WETH → USDC → DAI | DAI/WETH | USDC/WETH | DAI/USDC |
| 13 | USDC → DAI → WETH → USDC | DAI/USDC | DAI/WETH | USDC/WETH |
| 14 | USDC → WETH → DAI → USDC | USDC/WETH | DAI/WETH | DAI/USDC |
| 15 | WETH → DAI → USDC → WETH | DAI/WETH | DAI/USDC | USDC/WETH |
| 16 | WETH → USDC → DAI → WETH | USDC/WETH | DAI/USDC | DAI/WETH |

### BTC-稳定币三角 (WBTC-USDC-USDT)
| # | 路径 | 起始池 | 第二池 | 第三池 |
|---|------|--------|--------|--------|
| 17 | WBTC → USDC → USDT → WBTC | WBTC/USDC | USDC/USDT | WBTC/USDT |
| 18 | WBTC → USDT → USDC → WBTC | WBTC/USDT | USDC/USDT | WBTC/USDC |

### BTC-ETH-USDC三角 (WBTC-WETH-USDC)
| # | 路径 | 起始池 | 第二池 | 第三池 |
|---|------|--------|--------|--------|
| 19 | WBTC → WETH → USDC → WBTC | WBTC/WETH | USDC/WETH | WBTC/USDC |
| 20 | WBTC → USDC → WETH → WBTC | WBTC/USDC | USDC/WETH | WBTC/WETH |
| 21 | WETH → WBTC → USDC → WETH | WBTC/WETH | WBTC/USDC | USDC/WETH |
| 22 | WETH → USDC → WBTC → WETH | USDC/WETH | WBTC/USDC | WBTC/WETH |

### BTC-ETH-USDT三角 (WBTC-WETH-USDT)
| # | 路径 | 起始池 | 第二池 | 第三池 |
|---|------|--------|--------|--------|
| 23 | WBTC → WETH → USDT → WBTC | WBTC/WETH | WETH/USDT | WBTC/USDT |
| 24 | WBTC → USDT → WETH → WBTC | WBTC/USDT | WETH/USDT | WBTC/WETH |
| 25 | WETH → WBTC → USDT → WETH | WBTC/WETH | WBTC/USDT | WETH/USDT |
| 26 | WETH → USDT → WBTC → WETH | WETH/USDT | WBTC/USDT | WBTC/WETH |

---

## 套利触发机制

当任一监控池子发生 Swap 事件时，系统会：

1. **更新池子状态**: 更新该池子的 sqrtPriceX96、tick、liquidity 等信息
2. **筛选三角路径**: 只检测配置的 6 个三角组合（通过 `is_valid_triangle` 验证）
3. **本地快速估算**: 使用缓存的价格数据进行本地利润估算
4. **链上精确报价**: 对有潜力的路径调用 QuoterV2 获取真实报价
5. **计算净利润**: 扣除 Gas 成本后的净利润

## 过滤逻辑说明

系统会自动过滤掉：
- **不在配置中的三角组合**: 如 SHIB-WETH-USDC（SHIB 只有一个边，无法形成闭环）
- **小额交易**: 交易金额 < $100 的 Swap 事件
- **高手续费路径**: 总手续费 > 1% 的路径组合
- **本地估算亏损**: 本地快速估算无利润的路径

---

## 配置文件位置

- **代币配置**: `arbitrage_tokens` 数据库表
- **三角组合**: `arbitrage_triangles` 数据库表
- **池子配置**: `arbitrage_pools` 数据库表
- **初始化代码**: `backend_rust/crates/services/src/database.rs`
