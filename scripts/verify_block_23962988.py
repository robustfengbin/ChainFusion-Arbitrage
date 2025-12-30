#!/usr/bin/env python3
"""验证区块 23962988 的三角套利计算"""

def sqrt_price_x96_to_price(sqrt_price_x96: int, decimals0: int, decimals1: int) -> float:
    """将 sqrtPriceX96 转换为价格 (token1/token0)"""
    q96 = 2 ** 96
    sqrt_price = sqrt_price_x96 / q96
    raw_price = sqrt_price ** 2
    decimal_adjustment = 10 ** (decimals0 - decimals1)
    return raw_price * decimal_adjustment


print("=" * 80)
print("区块 23962988 三角套利验证")
print("路径: USDC → WETH → USDT → USDC")
print("=" * 80)

# 小数位数
USDC_DECIMALS = 6
WETH_DECIMALS = 18
USDT_DECIMALS = 6

# 池子数据 (从数据库查询)
# 1. USDC-WETH 500费率池 (0x88e6...)
usdc_weth_sqrt_price = 1415751629892847547683532427111224
usdc_weth_tick = 195826
usdc_weth_fee = 0.0005  # 0.05%

# 2. WETH-USDT 500费率池 (0x11b8...)
weth_usdt_sqrt_price = 4441666137082552305574556
weth_usdt_tick = -195791
weth_usdt_fee = 0.0005  # 0.05%

# 3. USDC-USDT 100费率池 (0x3416...)
usdc_usdt_sqrt_price = 79217445829088751497460178370
usdc_usdt_tick = -3
usdc_usdt_fee = 0.0001  # 0.01%

print("\n1. 各池子价格计算")
print("-" * 60)

# USDC-WETH 池: token0=USDC, token1=WETH
# price = WETH/USDC
usdc_weth_price = sqrt_price_x96_to_price(usdc_weth_sqrt_price, USDC_DECIMALS, WETH_DECIMALS)
print(f"USDC-WETH 池 (500费率):")
print(f"  sqrtPriceX96 = {usdc_weth_sqrt_price}")
print(f"  price(WETH/USDC) = {usdc_weth_price:.10f}")
print(f"  即: 1 USDC = {usdc_weth_price:.10f} WETH")
print(f"  即: 1 WETH = {1/usdc_weth_price:.2f} USDC")

# WETH-USDT 池: token0=WETH, token1=USDT
# price = USDT/WETH
weth_usdt_price = sqrt_price_x96_to_price(weth_usdt_sqrt_price, WETH_DECIMALS, USDT_DECIMALS)
print(f"\nWETH-USDT 池 (500费率):")
print(f"  sqrtPriceX96 = {weth_usdt_sqrt_price}")
print(f"  price(USDT/WETH) = {weth_usdt_price:.2f}")
print(f"  即: 1 WETH = {weth_usdt_price:.2f} USDT")

# USDC-USDT 池: token0=USDC, token1=USDT
# price = USDT/USDC
usdc_usdt_price = sqrt_price_x96_to_price(usdc_usdt_sqrt_price, USDC_DECIMALS, USDT_DECIMALS)
print(f"\nUSDC-USDT 池 (100费率):")
print(f"  sqrtPriceX96 = {usdc_usdt_sqrt_price}")
print(f"  price(USDT/USDC) = {usdc_usdt_price:.6f}")

print("\n" + "=" * 80)
print("2. 三跳套利计算 (USDC → WETH → USDT → USDC)")
print("=" * 80)

# 回测报告显示的输入金额
input_amount_usd = 235073.45
input_usdc = input_amount_usd  # 1 USDC ≈ $1

print(f"\n输入: {input_usdc:,.2f} USDC (约 ${input_amount_usd:,.2f})")

# 第一跳: USDC → WETH (USDC-WETH 500费率池)
print(f"\n第一跳: USDC → WETH (USDC-WETH 500费率池)")
print(f"  输入: {input_usdc:,.2f} USDC")
print(f"  池子价格: 1 USDC = {usdc_weth_price:.10f} WETH")
print(f"  费率: {usdc_weth_fee * 100:.2f}%")

# USDC -> WETH: 输入 token0, 输出 token1
# output = input * price * (1 - fee)
usdc_after_fee = input_usdc * (1 - usdc_weth_fee)
weth_output = usdc_after_fee * usdc_weth_price
print(f"  扣费后: {usdc_after_fee:,.2f} USDC")
print(f"  输出: {weth_output:.6f} WETH")

# 第二跳: WETH → USDT (WETH-USDT 500费率池)
print(f"\n第二跳: WETH → USDT (WETH-USDT 500费率池)")
print(f"  输入: {weth_output:.6f} WETH")
print(f"  池子价格: 1 WETH = {weth_usdt_price:.2f} USDT")
print(f"  费率: {weth_usdt_fee * 100:.2f}%")

# WETH -> USDT: 输入 token0, 输出 token1
# output = input * price * (1 - fee)
weth_after_fee = weth_output * (1 - weth_usdt_fee)
usdt_output = weth_after_fee * weth_usdt_price
print(f"  扣费后: {weth_after_fee:.6f} WETH")
print(f"  输出: {usdt_output:,.2f} USDT")

# 第三跳: USDT → USDC (USDC-USDT 100费率池)
print(f"\n第三跳: USDT → USDC (USDC-USDT 100费率池)")
print(f"  输入: {usdt_output:,.2f} USDT")
print(f"  池子价格: 1 USDC = {usdc_usdt_price:.6f} USDT")
print(f"  费率: {usdc_usdt_fee * 100:.4f}%")

# USDT -> USDC: 输入 token1, 输出 token0
# output = input / price * (1 - fee)
usdt_after_fee = usdt_output * (1 - usdc_usdt_fee)
usdc_output = usdt_after_fee / usdc_usdt_price
print(f"  扣费后: {usdt_after_fee:,.2f} USDT")
print(f"  输出: {usdc_output:,.2f} USDC")

print("\n" + "=" * 80)
print("3. 利润计算")
print("=" * 80)

gross_profit_usdc = usdc_output - input_usdc
gross_profit_usd = gross_profit_usdc  # 1 USDC ≈ $1
gas_cost = 27.02
net_profit = gross_profit_usd - gas_cost

print(f"  输入: {input_usdc:,.2f} USDC")
print(f"  输出: {usdc_output:,.2f} USDC")
print(f"  毛利润: {gross_profit_usdc:,.2f} USDC (${gross_profit_usd:,.2f})")
print(f"  Gas 成本: ${gas_cost:.2f}")
print(f"  净利润: ${net_profit:,.2f}")

print("\n" + "=" * 80)
print("4. 与回测报告对比")
print("=" * 80)
print(f"  回测报告毛利润: $685.32")
print(f"  手动计算毛利润: ${gross_profit_usd:,.2f}")
print(f"  差异: ${abs(685.32 - gross_profit_usd):,.2f}")

# 分析价格差异
print("\n" + "=" * 80)
print("5. 价格关系分析")
print("=" * 80)

# 通过 USDC-WETH 池得到的 WETH/USD 价格
weth_usd_via_usdc = 1 / usdc_weth_price
print(f"WETH/USD (via USDC-WETH): ${weth_usd_via_usdc:.2f}")

# 通过 WETH-USDT 池得到的 WETH/USD 价格
weth_usd_via_usdt = weth_usdt_price
print(f"WETH/USD (via WETH-USDT): ${weth_usd_via_usdt:.2f}")

# 价格偏差
price_diff = weth_usd_via_usdt - weth_usd_via_usdc
price_diff_pct = price_diff / weth_usd_via_usdc * 100
print(f"价格差异: ${price_diff:.2f} ({price_diff_pct:.4f}%)")

# 理论最大利润率
total_fee_pct = (usdc_weth_fee + weth_usdt_fee + usdc_usdt_fee) * 100
print(f"总费率: {total_fee_pct:.2f}%")
print(f"如果价格偏差 > 总费率，则有套利机会")
