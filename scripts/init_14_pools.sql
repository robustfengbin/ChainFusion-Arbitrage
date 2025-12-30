-- 初始化 14 个池子和 26 条套利路径
-- 基于 triangular_arbitrage_paths.md 配置

-- 清空表
TRUNCATE TABLE arbitrage_pool_paths;
TRUNCATE TABLE arbitrage_pools;

-- =====================================================
-- 插入 14 个池子 (chain_id = 1 为 Ethereum Mainnet)
-- =====================================================

-- 池子 1: DAI/USDC (0.01%)
INSERT INTO arbitrage_pools (chain_id, address, dex_type, token0, token0_symbol, token1, token1_symbol, fee, enabled)
VALUES (1, '0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168', 'uniswap_v3',
        '0x6B175474E89094C44Da98b954EedeAC495271d0F', 'DAI',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', 'USDC', 100, 1);

-- 池子 2: DAI/USDC (0.05%)
INSERT INTO arbitrage_pools (chain_id, address, dex_type, token0, token0_symbol, token1, token1_symbol, fee, enabled)
VALUES (1, '0x6c6Bc977E13Df9b0de53b251522280BB72383700', 'uniswap_v3',
        '0x6B175474E89094C44Da98b954EedeAC495271d0F', 'DAI',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', 'USDC', 500, 1);

-- 池子 3: DAI/USDT (0.01%)
INSERT INTO arbitrage_pools (chain_id, address, dex_type, token0, token0_symbol, token1, token1_symbol, fee, enabled)
VALUES (1, '0x6f48ECa74B38d2936B02ab603FF4e36A6C0E3A77', 'uniswap_v3',
        '0x6B175474E89094C44Da98b954EedeAC495271d0F', 'DAI',
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', 'USDT', 100, 1);

-- 池子 4: USDC/USDT (0.01%)
INSERT INTO arbitrage_pools (chain_id, address, dex_type, token0, token0_symbol, token1, token1_symbol, fee, enabled)
VALUES (1, '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6', 'uniswap_v3',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', 'USDC',
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', 'USDT', 100, 1);

-- 池子 5: USDC/WETH (0.05%) - 主力池
INSERT INTO arbitrage_pools (chain_id, address, dex_type, token0, token0_symbol, token1, token1_symbol, fee, enabled)
VALUES (1, '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640', 'uniswap_v3',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', 'USDC',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', 'WETH', 500, 1);

-- 池子 6: USDC/WETH (0.30%)
INSERT INTO arbitrage_pools (chain_id, address, dex_type, token0, token0_symbol, token1, token1_symbol, fee, enabled)
VALUES (1, '0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8', 'uniswap_v3',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', 'USDC',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', 'WETH', 3000, 1);

-- 池子 7: WETH/USDT (0.05%)
INSERT INTO arbitrage_pools (chain_id, address, dex_type, token0, token0_symbol, token1, token1_symbol, fee, enabled)
VALUES (1, '0x11b815efB8f581194ae79006d24E0d814B7697F6', 'uniswap_v3',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', 'WETH',
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', 'USDT', 500, 1);

-- 池子 8: WETH/USDT (0.30%)
INSERT INTO arbitrage_pools (chain_id, address, dex_type, token0, token0_symbol, token1, token1_symbol, fee, enabled)
VALUES (1, '0x4e68Ccd3E89f51C3074ca5072bBaC773960dFa36', 'uniswap_v3',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', 'WETH',
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', 'USDT', 3000, 1);

-- 池子 9: DAI/WETH (0.05%)
INSERT INTO arbitrage_pools (chain_id, address, dex_type, token0, token0_symbol, token1, token1_symbol, fee, enabled)
VALUES (1, '0x60594a405d53811d3BC4766596EFD80fd545A270', 'uniswap_v3',
        '0x6B175474E89094C44Da98b954EedeAC495271d0F', 'DAI',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', 'WETH', 500, 1);

-- 池子 10: DAI/WETH (0.30%)
INSERT INTO arbitrage_pools (chain_id, address, dex_type, token0, token0_symbol, token1, token1_symbol, fee, enabled)
VALUES (1, '0xC2e9F25Be6257c210d7Adf0D4Cd6E3E881ba25f8', 'uniswap_v3',
        '0x6B175474E89094C44Da98b954EedeAC495271d0F', 'DAI',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', 'WETH', 3000, 1);

-- 池子 11: WBTC/USDC (0.30%)
INSERT INTO arbitrage_pools (chain_id, address, dex_type, token0, token0_symbol, token1, token1_symbol, fee, enabled)
VALUES (1, '0x99ac8cA7087fA4A2A1FB6357269965A2014ABc35', 'uniswap_v3',
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599', 'WBTC',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', 'USDC', 3000, 1);

-- 池子 12: WBTC/USDT (0.30%)
INSERT INTO arbitrage_pools (chain_id, address, dex_type, token0, token0_symbol, token1, token1_symbol, fee, enabled)
VALUES (1, '0x9Db9e0e53058C89e5B94e29621a205198648425B', 'uniswap_v3',
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599', 'WBTC',
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', 'USDT', 3000, 1);

-- 池子 13: WBTC/WETH (0.05%) - 主力池
INSERT INTO arbitrage_pools (chain_id, address, dex_type, token0, token0_symbol, token1, token1_symbol, fee, enabled)
VALUES (1, '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0', 'uniswap_v3',
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599', 'WBTC',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', 'WETH', 500, 1);

-- 池子 14: WBTC/WETH (0.30%)
INSERT INTO arbitrage_pools (chain_id, address, dex_type, token0, token0_symbol, token1, token1_symbol, fee, enabled)
VALUES (1, '0xCBCdF9626bC03E24f779434178A73a0B4bad62eD', 'uniswap_v3',
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599', 'WBTC',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', 'WETH', 3000, 1);

-- =====================================================
-- 插入 26 条套利路径
-- =====================================================

-- 代币地址变量
-- DAI:  0x6B175474E89094C44Da98b954EedeAC495271d0F
-- USDC: 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48
-- USDT: 0xdAC17F958D2ee523a2206206994597C13D831ec7
-- WETH: 0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2
-- WBTC: 0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599

-- =====================================================
-- 稳定币三角 (DAI-USDC-USDT) - 优先级 10
-- =====================================================

-- 路径 1: DAI → USDC → USDT → DAI (触发: DAI/USDC 0.01%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168',
        'DAI→USDC→USDT→DAI', 'DAI-USDC-USDT',
        '0x6B175474E89094C44Da98b954EedeAC495271d0F', -- DAI
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', -- USDT
        '0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168', -- DAI/USDC 0.01%
        '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6', -- USDC/USDT 0.01%
        '0x6f48ECa74B38d2936B02ab603FF4e36A6C0E3A77', -- DAI/USDT 0.01%
        10, 1);

-- 路径 2: DAI → USDC → USDT → DAI (触发: DAI/USDC 0.05%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x6c6Bc977E13Df9b0de53b251522280BB72383700',
        'DAI→USDC→USDT→DAI', 'DAI-USDC-USDT',
        '0x6B175474E89094C44Da98b954EedeAC495271d0F',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48',
        '0xdAC17F958D2ee523a2206206994597C13D831ec7',
        '0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168',
        '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6',
        '0x6f48ECa74B38d2936B02ab603FF4e36A6C0E3A77',
        10, 1);

-- 路径 3: DAI → USDT → USDC → DAI (触发: DAI/USDT 0.01%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x6f48ECa74B38d2936B02ab603FF4e36A6C0E3A77',
        'DAI→USDT→USDC→DAI', 'DAI-USDC-USDT',
        '0x6B175474E89094C44Da98b954EedeAC495271d0F', -- DAI
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', -- USDT
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0x6f48ECa74B38d2936B02ab603FF4e36A6C0E3A77', -- DAI/USDT 0.01%
        '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6', -- USDC/USDT 0.01%
        '0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168', -- DAI/USDC 0.01%
        10, 1);

-- 路径 4: USDC → USDT → DAI → USDC (触发: USDC/USDT 0.01%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6',
        'USDC→USDT→DAI→USDC', 'DAI-USDC-USDT',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', -- USDT
        '0x6B175474E89094C44Da98b954EedeAC495271d0F', -- DAI
        '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6', -- USDC/USDT 0.01%
        '0x6f48ECa74B38d2936B02ab603FF4e36A6C0E3A77', -- DAI/USDT 0.01%
        '0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168', -- DAI/USDC 0.01%
        10, 1);

-- =====================================================
-- ETH-稳定币三角 (USDC-WETH-USDT) - 优先级 20
-- =====================================================

-- 路径 5: USDC → WETH → USDT → USDC (触发: USDC/WETH 0.05%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640',
        'USDC→WETH→USDT→USDC', 'USDC-WETH-USDT',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', -- WETH
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', -- USDT
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640', -- USDC/WETH 0.05%
        '0x11b815efB8f581194ae79006d24E0d814B7697F6', -- WETH/USDT 0.05%
        '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6', -- USDC/USDT 0.01%
        20, 1);

-- 路径 6: USDC → WETH → USDT → USDC (触发: USDC/WETH 0.30%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8',
        'USDC→WETH→USDT→USDC', 'USDC-WETH-USDT',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2',
        '0xdAC17F958D2ee523a2206206994597C13D831ec7',
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640',
        '0x11b815efB8f581194ae79006d24E0d814B7697F6',
        '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6',
        20, 1);

-- 路径 7: WETH → USDC → USDT → WETH (触发: USDC/WETH 0.05%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640',
        'WETH→USDC→USDT→WETH', 'USDC-WETH-USDT',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', -- WETH
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', -- USDT
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640', -- USDC/WETH 0.05%
        '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6', -- USDC/USDT 0.01%
        '0x11b815efB8f581194ae79006d24E0d814B7697F6', -- WETH/USDT 0.05%
        20, 1);

-- 路径 8: WETH → USDC → USDT → WETH (触发: USDC/WETH 0.30%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8',
        'WETH→USDC→USDT→WETH', 'USDC-WETH-USDT',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48',
        '0xdAC17F958D2ee523a2206206994597C13D831ec7',
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640',
        '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6',
        '0x11b815efB8f581194ae79006d24E0d814B7697F6',
        20, 1);

-- 路径 9: WETH → USDT → USDC → WETH (触发: WETH/USDT 0.05%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x11b815efB8f581194ae79006d24E0d814B7697F6',
        'WETH→USDT→USDC→WETH', 'USDC-WETH-USDT',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', -- WETH
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', -- USDT
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0x11b815efB8f581194ae79006d24E0d814B7697F6', -- WETH/USDT 0.05%
        '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6', -- USDC/USDT 0.01%
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640', -- USDC/WETH 0.05%
        20, 1);

-- 路径 10: WETH → USDT → USDC → WETH (触发: WETH/USDT 0.30%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x4e68Ccd3E89f51C3074ca5072bBaC773960dFa36',
        'WETH→USDT→USDC→WETH', 'USDC-WETH-USDT',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2',
        '0xdAC17F958D2ee523a2206206994597C13D831ec7',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48',
        '0x11b815efB8f581194ae79006d24E0d814B7697F6',
        '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6',
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640',
        20, 1);

-- 路径 11: USDC → USDT → WETH → USDC (触发: USDC/USDT 0.01%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6',
        'USDC→USDT→WETH→USDC', 'USDC-WETH-USDT',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', -- USDT
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', -- WETH
        '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6', -- USDC/USDT 0.01%
        '0x11b815efB8f581194ae79006d24E0d814B7697F6', -- WETH/USDT 0.05%
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640', -- USDC/WETH 0.05%
        20, 1);

-- 路径 12: USDT → WETH → USDC → USDT (触发: WETH/USDT 0.05%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x11b815efB8f581194ae79006d24E0d814B7697F6',
        'USDT→WETH→USDC→USDT', 'USDC-WETH-USDT',
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', -- USDT
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', -- WETH
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0x11b815efB8f581194ae79006d24E0d814B7697F6', -- WETH/USDT 0.05%
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640', -- USDC/WETH 0.05%
        '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6', -- USDC/USDT 0.01%
        20, 1);

-- 路径 13: USDT → WETH → USDC → USDT (触发: WETH/USDT 0.30%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x4e68Ccd3E89f51C3074ca5072bBaC773960dFa36',
        'USDT→WETH→USDC→USDT', 'USDC-WETH-USDT',
        '0xdAC17F958D2ee523a2206206994597C13D831ec7',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48',
        '0x11b815efB8f581194ae79006d24E0d814B7697F6',
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640',
        '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6',
        20, 1);

-- =====================================================
-- DAI-ETH-稳定币三角 (DAI-USDC-WETH) - 优先级 30
-- =====================================================

-- 路径 14: DAI → USDC → WETH → DAI (触发: DAI/USDC 0.01%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168',
        'DAI→USDC→WETH→DAI', 'DAI-USDC-WETH',
        '0x6B175474E89094C44Da98b954EedeAC495271d0F', -- DAI
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', -- WETH
        '0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168', -- DAI/USDC 0.01%
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640', -- USDC/WETH 0.05%
        '0x60594a405d53811d3BC4766596EFD80fd545A270', -- DAI/WETH 0.05%
        30, 1);

-- 路径 15: DAI → USDC → WETH → DAI (触发: DAI/USDC 0.05%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x6c6Bc977E13Df9b0de53b251522280BB72383700',
        'DAI→USDC→WETH→DAI', 'DAI-USDC-WETH',
        '0x6B175474E89094C44Da98b954EedeAC495271d0F',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2',
        '0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168',
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640',
        '0x60594a405d53811d3BC4766596EFD80fd545A270',
        30, 1);

-- 路径 16: DAI → WETH → USDC → DAI (触发: DAI/WETH 0.05%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x60594a405d53811d3BC4766596EFD80fd545A270',
        'DAI→WETH→USDC→DAI', 'DAI-USDC-WETH',
        '0x6B175474E89094C44Da98b954EedeAC495271d0F', -- DAI
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', -- WETH
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0x60594a405d53811d3BC4766596EFD80fd545A270', -- DAI/WETH 0.05%
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640', -- USDC/WETH 0.05%
        '0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168', -- DAI/USDC 0.01%
        30, 1);

-- 路径 17: DAI → WETH → USDC → DAI (触发: DAI/WETH 0.30%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0xC2e9F25Be6257c210d7Adf0D4Cd6E3E881ba25f8',
        'DAI→WETH→USDC→DAI', 'DAI-USDC-WETH',
        '0x6B175474E89094C44Da98b954EedeAC495271d0F',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48',
        '0x60594a405d53811d3BC4766596EFD80fd545A270',
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640',
        '0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168',
        30, 1);

-- 路径 18: USDC → WETH → DAI → USDC (触发: USDC/WETH 0.05%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640',
        'USDC→WETH→DAI→USDC', 'DAI-USDC-WETH',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', -- WETH
        '0x6B175474E89094C44Da98b954EedeAC495271d0F', -- DAI
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640', -- USDC/WETH 0.05%
        '0x60594a405d53811d3BC4766596EFD80fd545A270', -- DAI/WETH 0.05%
        '0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168', -- DAI/USDC 0.01%
        30, 1);

-- 路径 19: USDC → WETH → DAI → USDC (触发: USDC/WETH 0.30%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8',
        'USDC→WETH→DAI→USDC', 'DAI-USDC-WETH',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2',
        '0x6B175474E89094C44Da98b954EedeAC495271d0F',
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640',
        '0x60594a405d53811d3BC4766596EFD80fd545A270',
        '0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168',
        30, 1);

-- 路径 20: WETH → DAI → USDC → WETH (触发: DAI/WETH 0.05%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x60594a405d53811d3BC4766596EFD80fd545A270',
        'WETH→DAI→USDC→WETH', 'DAI-USDC-WETH',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', -- WETH
        '0x6B175474E89094C44Da98b954EedeAC495271d0F', -- DAI
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0x60594a405d53811d3BC4766596EFD80fd545A270', -- DAI/WETH 0.05%
        '0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168', -- DAI/USDC 0.01%
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640', -- USDC/WETH 0.05%
        30, 1);

-- 路径 21: WETH → DAI → USDC → WETH (触发: DAI/WETH 0.30%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0xC2e9F25Be6257c210d7Adf0D4Cd6E3E881ba25f8',
        'WETH→DAI→USDC→WETH', 'DAI-USDC-WETH',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2',
        '0x6B175474E89094C44Da98b954EedeAC495271d0F',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48',
        '0x60594a405d53811d3BC4766596EFD80fd545A270',
        '0x5777d92f208679DB4b9778590Fa3CAB3aC9e2168',
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640',
        30, 1);

-- =====================================================
-- BTC-稳定币三角 (WBTC-USDC-USDT) - 优先级 40
-- =====================================================

-- 路径 22: WBTC → USDC → USDT → WBTC (触发: WBTC/USDC 0.30%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x99ac8cA7087fA4A2A1FB6357269965A2014ABc35',
        'WBTC→USDC→USDT→WBTC', 'WBTC-USDC-USDT',
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599', -- WBTC
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', -- USDT
        '0x99ac8cA7087fA4A2A1FB6357269965A2014ABc35', -- WBTC/USDC 0.30%
        '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6', -- USDC/USDT 0.01%
        '0x9Db9e0e53058C89e5B94e29621a205198648425B', -- WBTC/USDT 0.30%
        40, 1);

-- 路径 23: WBTC → USDT → USDC → WBTC (触发: WBTC/USDT 0.30%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x9Db9e0e53058C89e5B94e29621a205198648425B',
        'WBTC→USDT→USDC→WBTC', 'WBTC-USDC-USDT',
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599', -- WBTC
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', -- USDT
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0x9Db9e0e53058C89e5B94e29621a205198648425B', -- WBTC/USDT 0.30%
        '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6', -- USDC/USDT 0.01%
        '0x99ac8cA7087fA4A2A1FB6357269965A2014ABc35', -- WBTC/USDC 0.30%
        40, 1);

-- 路径 24: USDC → USDT → WBTC → USDC (触发: USDC/USDT 0.01%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6',
        'USDC→USDT→WBTC→USDC', 'WBTC-USDC-USDT',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', -- USDT
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599', -- WBTC
        '0x3416cF6C708Da44DB2624D63ea0AAef7113527C6', -- USDC/USDT 0.01%
        '0x9Db9e0e53058C89e5B94e29621a205198648425B', -- WBTC/USDT 0.30%
        '0x99ac8cA7087fA4A2A1FB6357269965A2014ABc35', -- WBTC/USDC 0.30%
        40, 1);

-- =====================================================
-- BTC-ETH-USDC三角 (WBTC-WETH-USDC) - 优先级 50
-- =====================================================

-- 路径 25: WBTC → WETH → USDC → WBTC (触发: WBTC/WETH 0.05%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0',
        'WBTC→WETH→USDC→WBTC', 'WBTC-WETH-USDC',
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599', -- WBTC
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', -- WETH
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0', -- WBTC/WETH 0.05%
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640', -- USDC/WETH 0.05%
        '0x99ac8cA7087fA4A2A1FB6357269965A2014ABc35', -- WBTC/USDC 0.30%
        50, 1);

-- 路径 26: WBTC → WETH → USDC → WBTC (触发: WBTC/WETH 0.30%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0xCBCdF9626bC03E24f779434178A73a0B4bad62eD',
        'WBTC→WETH→USDC→WBTC', 'WBTC-WETH-USDC',
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48',
        '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0',
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640',
        '0x99ac8cA7087fA4A2A1FB6357269965A2014ABc35',
        50, 1);

-- 路径 27: WBTC → USDC → WETH → WBTC (触发: WBTC/USDC 0.30%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x99ac8cA7087fA4A2A1FB6357269965A2014ABc35',
        'WBTC→USDC→WETH→WBTC', 'WBTC-WETH-USDC',
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599', -- WBTC
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', -- WETH
        '0x99ac8cA7087fA4A2A1FB6357269965A2014ABc35', -- WBTC/USDC 0.30%
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640', -- USDC/WETH 0.05%
        '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0', -- WBTC/WETH 0.05%
        50, 1);

-- 路径 28: USDC → WETH → WBTC → USDC (触发: USDC/WETH 0.05%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640',
        'USDC→WETH→WBTC→USDC', 'WBTC-WETH-USDC',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', -- WETH
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599', -- WBTC
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640', -- USDC/WETH 0.05%
        '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0', -- WBTC/WETH 0.05%
        '0x99ac8cA7087fA4A2A1FB6357269965A2014ABc35', -- WBTC/USDC 0.30%
        50, 1);

-- 路径 29: USDC → WETH → WBTC → USDC (触发: USDC/WETH 0.30%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8',
        'USDC→WETH→WBTC→USDC', 'WBTC-WETH-USDC',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2',
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599',
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640',
        '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0',
        '0x99ac8cA7087fA4A2A1FB6357269965A2014ABc35',
        50, 1);

-- 路径 30: WETH → WBTC → USDC → WETH (触发: WBTC/WETH 0.05%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0',
        'WETH→WBTC→USDC→WETH', 'WBTC-WETH-USDC',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', -- WETH
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599', -- WBTC
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', -- USDC
        '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0', -- WBTC/WETH 0.05%
        '0x99ac8cA7087fA4A2A1FB6357269965A2014ABc35', -- WBTC/USDC 0.30%
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640', -- USDC/WETH 0.05%
        50, 1);

-- 路径 31: WETH → WBTC → USDC → WETH (触发: WBTC/WETH 0.30%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0xCBCdF9626bC03E24f779434178A73a0B4bad62eD',
        'WETH→WBTC→USDC→WETH', 'WBTC-WETH-USDC',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2',
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599',
        '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48',
        '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0',
        '0x99ac8cA7087fA4A2A1FB6357269965A2014ABc35',
        '0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640',
        50, 1);

-- =====================================================
-- BTC-ETH-USDT三角 (WBTC-WETH-USDT) - 优先级 60
-- =====================================================

-- 路径 32: WBTC → WETH → USDT → WBTC (触发: WBTC/WETH 0.05%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0',
        'WBTC→WETH→USDT→WBTC', 'WBTC-WETH-USDT',
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599', -- WBTC
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', -- WETH
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', -- USDT
        '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0', -- WBTC/WETH 0.05%
        '0x11b815efB8f581194ae79006d24E0d814B7697F6', -- WETH/USDT 0.05%
        '0x9Db9e0e53058C89e5B94e29621a205198648425B', -- WBTC/USDT 0.30%
        60, 1);

-- 路径 33: WBTC → WETH → USDT → WBTC (触发: WBTC/WETH 0.30%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0xCBCdF9626bC03E24f779434178A73a0B4bad62eD',
        'WBTC→WETH→USDT→WBTC', 'WBTC-WETH-USDT',
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2',
        '0xdAC17F958D2ee523a2206206994597C13D831ec7',
        '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0',
        '0x11b815efB8f581194ae79006d24E0d814B7697F6',
        '0x9Db9e0e53058C89e5B94e29621a205198648425B',
        60, 1);

-- 路径 34: WBTC → USDT → WETH → WBTC (触发: WBTC/USDT 0.30%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x9Db9e0e53058C89e5B94e29621a205198648425B',
        'WBTC→USDT→WETH→WBTC', 'WBTC-WETH-USDT',
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599', -- WBTC
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', -- USDT
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', -- WETH
        '0x9Db9e0e53058C89e5B94e29621a205198648425B', -- WBTC/USDT 0.30%
        '0x11b815efB8f581194ae79006d24E0d814B7697F6', -- WETH/USDT 0.05%
        '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0', -- WBTC/WETH 0.05%
        60, 1);

-- 路径 35: WETH → WBTC → USDT → WETH (触发: WBTC/WETH 0.05%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0',
        'WETH→WBTC→USDT→WETH', 'WBTC-WETH-USDT',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', -- WETH
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599', -- WBTC
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', -- USDT
        '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0', -- WBTC/WETH 0.05%
        '0x9Db9e0e53058C89e5B94e29621a205198648425B', -- WBTC/USDT 0.30%
        '0x11b815efB8f581194ae79006d24E0d814B7697F6', -- WETH/USDT 0.05%
        60, 1);

-- 路径 36: WETH → WBTC → USDT → WETH (触发: WBTC/WETH 0.30%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0xCBCdF9626bC03E24f779434178A73a0B4bad62eD',
        'WETH→WBTC→USDT→WETH', 'WBTC-WETH-USDT',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2',
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599',
        '0xdAC17F958D2ee523a2206206994597C13D831ec7',
        '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0',
        '0x9Db9e0e53058C89e5B94e29621a205198648425B',
        '0x11b815efB8f581194ae79006d24E0d814B7697F6',
        60, 1);

-- 路径 37: WETH → USDT → WBTC → WETH (触发: WETH/USDT 0.05%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x11b815efB8f581194ae79006d24E0d814B7697F6',
        'WETH→USDT→WBTC→WETH', 'WBTC-WETH-USDT',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', -- WETH
        '0xdAC17F958D2ee523a2206206994597C13D831ec7', -- USDT
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599', -- WBTC
        '0x11b815efB8f581194ae79006d24E0d814B7697F6', -- WETH/USDT 0.05%
        '0x9Db9e0e53058C89e5B94e29621a205198648425B', -- WBTC/USDT 0.30%
        '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0', -- WBTC/WETH 0.05%
        60, 1);

-- 路径 38: WETH → USDT → WBTC → WETH (触发: WETH/USDT 0.30%)
INSERT INTO arbitrage_pool_paths (chain_id, trigger_pool, path_name, triangle_name, token_a, token_b, token_c, pool1, pool2, pool3, priority, enabled)
VALUES (1, '0x4e68Ccd3E89f51C3074ca5072bBaC773960dFa36',
        'WETH→USDT→WBTC→WETH', 'WBTC-WETH-USDT',
        '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2',
        '0xdAC17F958D2ee523a2206206994597C13D831ec7',
        '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599',
        '0x11b815efB8f581194ae79006d24E0d814B7697F6',
        '0x9Db9e0e53058C89e5B94e29621a205198648425B',
        '0x4585FE77225b41b697C938B018E2Ac67Ac5a20c0',
        60, 1);

-- =====================================================
-- 验证结果
-- =====================================================
SELECT '===== 池子统计 =====' AS info;
SELECT COUNT(*) AS pool_count FROM arbitrage_pools;
SELECT address, CONCAT(token0_symbol, '/', token1_symbol) AS pair, fee/10000.0 AS fee_percent FROM arbitrage_pools ORDER BY id;

SELECT '===== 路径统计 =====' AS info;
SELECT COUNT(*) AS path_count FROM arbitrage_pool_paths;
SELECT triangle_name, COUNT(*) AS paths FROM arbitrage_pool_paths GROUP BY triangle_name ORDER BY priority;
