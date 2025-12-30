#!/usr/bin/env python3
"""验证三角套利计算的正确性"""

import math

def sqrt_price_x96_to_price(sqrt_price_x96: int, decimals0: int, decimals1: int) -> float:
    """将 sqrtPriceX96 转换为价格 (token1/token0)"""
    q96 = 2 ** 96
    sqrt_price = sqrt_price_x96 / q96
    raw_price = sqrt_price ** 2
    decimal_adjustment = 10 ** (decimals0 - decimals1)
    return raw_price * decimal_adjustment


def tick_to_price(tick: int, decimals0: int, decimals1: int) -> float:
    """将 tick 转换为价格"""
    raw_price = 1.0001 ** tick
    decimal_adjustment = 10 ** (decimals0 - decimals1)
    return raw_price * decimal_adjustment


# 区块 23962988 的数据 - 最大利润机会
print("=" * 80)
print("区块 23962988 三角套利验证")
print("路径: USDC → WETH → USDT → USDC")
print("=" * 80)

# 池子数据 (从数据库查询结果)
# 1. WETH-USDT 3000费率池 (0x4e68...)
weth_usdt_sqrt_price = 4451917912263315629049748
weth_usdt_tick = -195745
weth_usdt_fee = 0.003  # 0.3%

# 2. USDC-USDT 100费率池 (0x3416...)
usdc_usdt_sqrt_price = 79216510988341235694979128691
usdc_usdt_tick = -3
usdc_usdt_fee = 0.0001  # 0.01%

# 3. USDC-WETH 500费率池 (0x88e6...)
usdc_weth_sqrt_price = 1416227891581353475536807466141954
usdc_weth_tick = 195833
usdc_weth_fee = 0.0005  # 0.05%

# 小数位数
WETH_DECIMALS = 18
USDT_DECIMALS = 6
USDC_DECIMALS = 6

print("\n1. 各池子价格计算")
print("-" * 60)

# WETH-USDT 池: token0=WETH, token1=USDT
# price = USDT/WETH
weth_usdt_price = sqrt_price_x96_to_price(weth_usdt_sqrt_price, WETH_DECIMALS, USDT_DECIMALS)
weth_usdt_price_tick = tick_to_price(weth_usdt_tick, WETH_DECIMALS, USDT_DECIMALS)
print(f"WETH-USDT 池 (3000费率):")
print(f"  sqrtPriceX96 = {weth_usdt_sqrt_price}")
print(f"  从 sqrtPriceX96: 1 WETH = {weth_usdt_price:.2f} USDT")
print(f"  从 tick (-195745): 1 WETH = {weth_usdt_price_tick:.2f} USDT")

# USDC-USDT 池: token0=USDC, token1=USDT
# price = USDT/USDC
usdc_usdt_price = sqrt_price_x96_to_price(usdc_usdt_sqrt_price, USDC_DECIMALS, USDT_DECIMALS)
usdc_usdt_price_tick = tick_to_price(usdc_usdt_tick, USDC_DECIMALS, USDT_DECIMALS)
print(f"\nUSDC-USDT 池 (100费率):")
print(f"  sqrtPriceX96 = {usdc_usdt_sqrt_price}")
print(f"  从 sqrtPriceX96: 1 USDC = {usdc_usdt_price:.6f} USDT")
print(f"  从 tick (-3): 1 USDC = {usdc_usdt_price_tick:.6f} USDT")

# USDC-WETH 池: token0=USDC, token1=WETH
# price = WETH/USDC
usdc_weth_price = sqrt_price_x96_to_price(usdc_weth_sqrt_price, USDC_DECIMALS, WETH_DECIMALS)
usdc_weth_price_tick = tick_to_price(usdc_weth_tick, USDC_DECIMALS, WETH_DECIMALS)
print(f"\nUSDC-WETH 池 (500费率):")
print(f"  sqrtPriceX96 = {usdc_weth_sqrt_price}")
print(f"  从 sqrtPriceX96: 1 USDC = {usdc_weth_price:.10f} WETH")
print(f"  从 tick (195833): 1 USDC = {usdc_weth_price_tick:.10f} WETH")
print(f"  即: 1 WETH = {1/usdc_weth_price:.2f} USDC")

print("\n" + "=" * 80)
print("2. 三跳套利计算 (WETH → USDT → USDC → WETH)")
print("=" * 80)

# 输入金额 (USD)
input_amount_usd = 2147672.71
print(f"\n输入: ${input_amount_usd:,.2f} (假设为 WETH 价值)")

# 第一跳: WETH → USDT
# WETH-USDT 池: 1 WETH = ~3959 USDT
# 输入 $2,147,672.71 WETH
weth_price_usd = weth_usdt_price  # WETH/USD 约等于 WETH/USDT (因为 USDT ≈ $1)
input_weth = input_amount_usd / weth_price_usd
print(f"\n第一跳: WETH → USDT (WETH-USDT 3000费率池)")
print(f"  输入: {input_weth:.6f} WETH (价值 ${input_amount_usd:,.2f})")
print(f"  池子价格: 1 WETH = {weth_usdt_price:.2f} USDT")
print(f"  费率: {weth_usdt_fee * 100:.2f}%")

# 计算输出 USDT
# WETH → USDT: 输入 token0, 输出 token1
# output = input * price * (1 - fee)
usdt_output = input_weth * weth_usdt_price * (1 - weth_usdt_fee)
print(f"  输出: {usdt_output:,.2f} USDT (扣除 {weth_usdt_fee*100:.2f}% 费用)")

# 第二跳: USDT → USDC
print(f"\n第二跳: USDT → USDC (USDC-USDT 100费率池)")
print(f"  输入: {usdt_output:,.2f} USDT")
print(f"  池子价格: 1 USDC = {usdc_usdt_price:.6f} USDT")
print(f"  费率: {usdc_usdt_fee * 100:.4f}%")

# USDT → USDC: 输入 token1, 输出 token0
# output = input / price * (1 - fee)
usdc_output = usdt_output / usdc_usdt_price * (1 - usdc_usdt_fee)
print(f"  输出: {usdc_output:,.2f} USDC")

# 第三跳: USDC → WETH
print(f"\n第三跳: USDC → WETH (USDC-WETH 500费率池)")
print(f"  输入: {usdc_output:,.2f} USDC")
print(f"  池子价格: 1 USDC = {usdc_weth_price:.10f} WETH")
print(f"  费率: {usdc_weth_fee * 100:.2f}%")

# USDC → WETH: 输入 token0, 输出 token1
# output = input * price * (1 - fee)
weth_output = usdc_output * usdc_weth_price * (1 - usdc_weth_fee)
print(f"  输出: {weth_output:.6f} WETH")

# 计算利润
output_value_usd = weth_output * weth_price_usd  # WETH 价值
gross_profit = output_value_usd - input_amount_usd
gas_cost = 27.02  # Gas 成本
net_profit = gross_profit - gas_cost

print("\n" + "=" * 80)
print("3. 利润计算")
print("=" * 80)
print(f"  输入: ${input_amount_usd:,.2f}")
print(f"  输出: {weth_output:.6f} WETH = ${output_value_usd:,.2f}")
print(f"  毛利润: ${gross_profit:,.2f}")
print(f"  Gas 成本: ${gas_cost:.2f}")
print(f"  净利润: ${net_profit:,.2f}")

print("\n" + "=" * 80)
print("4. 与回测报告对比")
print("=" * 80)
print(f"  回测报告净利润: $4,817.93")
print(f"  手动计算净利润: ${net_profit:,.2f}")
print(f"  差异: ${abs(4817.93 - net_profit):,.2f}")

# 检查问题
print("\n" + "=" * 80)
print("5. 问题分析")
print("=" * 80)

# 检查价格比例
eth_usdt_price = weth_usdt_price
eth_usdc_price = 1 / usdc_weth_price  # 反转得到 WETH/USDC
usdt_usdc_price = usdc_usdt_price

print(f"ETH/USDT 价格: {eth_usdt_price:.2f}")
print(f"ETH/USDC 价格: {eth_usdc_price:.2f}")
print(f"USDT/USDC 价格: {usdt_usdc_price:.6f}")

# 理论上无套利时: ETH/USDT = ETH/USDC * USDC/USDT
theoretical_eth_usdt = eth_usdc_price * usdt_usdc_price
print(f"\n理论 ETH/USDT (从 ETH/USDC * USDC/USDT): {theoretical_eth_usdt:.2f}")
print(f"实际 ETH/USDT: {eth_usdt_price:.2f}")
print(f"价格偏差: {(eth_usdt_price - theoretical_eth_usdt) / theoretical_eth_usdt * 100:.4f}%")
