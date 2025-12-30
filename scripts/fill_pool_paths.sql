-- 填充 arbitrage_pool_paths 表的 pool1, pool2, pool3 字段
-- 策略：选择费率最低的池子

-- 创建临时表存储 token pair 到最低费率池子的映射
DROP TEMPORARY TABLE IF EXISTS token_pair_pools;
CREATE TEMPORARY TABLE token_pair_pools AS
SELECT
    LOWER(token0) as token_a,
    LOWER(token1) as token_b,
    address as pool_address,
    fee,
    ROW_NUMBER() OVER (PARTITION BY LOWER(token0), LOWER(token1) ORDER BY fee ASC) as rn
FROM arbitrage_pools
WHERE chain_id = 1;

-- 插入反向映射
INSERT INTO token_pair_pools (token_a, token_b, pool_address, fee, rn)
SELECT
    LOWER(token1) as token_a,
    LOWER(token0) as token_b,
    address as pool_address,
    fee,
    ROW_NUMBER() OVER (PARTITION BY LOWER(token1), LOWER(token0) ORDER BY fee ASC) as rn
FROM arbitrage_pools
WHERE chain_id = 1;

-- 查看当前状态
SELECT 'Before update:' as status;
SELECT path_name, pool1, pool2, pool3 FROM arbitrage_pool_paths WHERE chain_id=1 LIMIT 5;

-- 更新 pool1 (token_a -> token_b)
UPDATE arbitrage_pool_paths p
JOIN token_pair_pools t ON (
    LOWER(p.token_a) = t.token_a AND
    LOWER(p.token_b) = t.token_b AND
    t.rn = 1
)
SET p.pool1 = t.pool_address
WHERE p.chain_id = 1;

-- 更新 pool2 (token_b -> token_c)
UPDATE arbitrage_pool_paths p
JOIN token_pair_pools t ON (
    LOWER(p.token_b) = t.token_a AND
    LOWER(p.token_c) = t.token_b AND
    t.rn = 1
)
SET p.pool2 = t.pool_address
WHERE p.chain_id = 1;

-- 更新 pool3 (token_c -> token_a)
UPDATE arbitrage_pool_paths p
JOIN token_pair_pools t ON (
    LOWER(p.token_c) = t.token_a AND
    LOWER(p.token_a) = t.token_b AND
    t.rn = 1
)
SET p.pool3 = t.pool_address
WHERE p.chain_id = 1;

-- 查看更新后的结果
SELECT 'After update:' as status;
SELECT path_name, pool1, pool2, pool3 FROM arbitrage_pool_paths WHERE chain_id=1;

-- 检查哪些路径还有空的池子字段
SELECT 'Paths with missing pools:' as status;
SELECT path_name, token_a, token_b, token_c, pool1, pool2, pool3
FROM arbitrage_pool_paths
WHERE chain_id=1 AND (pool1 IS NULL OR pool1 = '' OR pool2 IS NULL OR pool2 = '' OR pool3 IS NULL OR pool3 = '');

DROP TEMPORARY TABLE IF EXISTS token_pair_pools;
