use anyhow::{anyhow, Result};
use async_trait::async_trait;
use ethers::prelude::*;
use ethers::types::{Address, Bytes, U256, I256, H256};
use ethers::abi::{encode, Token};
use models::{DexType, Pool, PoolState, UniswapV4PoolState, UniswapV4PoolKey};
use std::sync::Arc;

use crate::common::DexProtocol;
use super::contracts::v4_addresses;

// V4 StateView 用于读取池子状态
abigen!(
    UniswapV4StateView,
    r#"[
        function getSlot0(bytes32 poolId) external view returns (uint160 sqrtPriceX96, int24 tick, uint24 protocolFee, uint24 lpFee)
        function getLiquidity(bytes32 poolId) external view returns (uint128 liquidity)
        function getPositionInfo(bytes32 poolId, bytes32 positionId) external view returns (uint128 liquidity, uint256 feeGrowthInside0LastX128, uint256 feeGrowthInside1LastX128)
        function getFeeGrowthGlobals(bytes32 poolId) external view returns (uint256 feeGrowthGlobal0X128, uint256 feeGrowthGlobal1X128)
    ]"#
);

// PoolManager 基础函数 ABI
abigen!(
    UniswapV4PoolManagerBase,
    r#"[
        function settle() external payable returns (uint256)
        function take(address currency, address to, uint256 amount) external
        function sync(address currency) external
        function mint(address to, uint256 id, uint256 amount) external
        function burn(address from, uint256 id, uint256 amount) external
        function unlock(bytes calldata data) external returns (bytes memory)
        function extsload(bytes32 slot) external view returns (bytes32)
        event Initialize(bytes32 indexed id, address indexed currency0, address indexed currency1, uint24 fee, int24 tickSpacing, address hooks, uint160 sqrtPriceX96, int24 tick)
        event Swap(bytes32 indexed id, address indexed sender, int128 amount0, int128 amount1, uint160 sqrtPriceX96, uint128 liquidity, int24 tick, uint24 fee)
    ]"#
);

/// V4 PoolKey 用于标识池子
#[derive(Debug, Clone)]
pub struct PoolKey {
    pub currency0: Address,
    pub currency1: Address,
    pub fee: u32,
    pub tick_spacing: i32,
    pub hooks: Address,
}

impl PoolKey {
    pub fn new(
        currency0: Address,
        currency1: Address,
        fee: u32,
        tick_spacing: i32,
        hooks: Address,
    ) -> Self {
        // 确保 currency0 < currency1
        let (c0, c1) = if currency0 < currency1 {
            (currency0, currency1)
        } else {
            (currency1, currency0)
        };

        Self {
            currency0: c0,
            currency1: c1,
            fee,
            tick_spacing,
            hooks,
        }
    }

    /// 计算 Pool ID (keccak256 of encoded PoolKey)
    pub fn to_id(&self) -> [u8; 32] {
        v4_addresses::compute_pool_id(
            self.currency0,
            self.currency1,
            self.fee,
            self.tick_spacing,
            self.hooks,
        )
    }

    /// 编码为 ABI Token
    pub fn to_abi_token(&self) -> Token {
        Token::Tuple(vec![
            Token::Address(self.currency0),
            Token::Address(self.currency1),
            Token::Uint(self.fee.into()),
            Token::Int(self.tick_spacing.into()),
            Token::Address(self.hooks),
        ])
    }
}

/// V4 Swap 参数
#[derive(Debug, Clone)]
pub struct SwapParams {
    pub zero_for_one: bool,
    pub amount_specified: I256,
    pub sqrt_price_limit_x96: U256,
}

impl SwapParams {
    /// 创建 ExactIn swap 参数
    pub fn exact_input(zero_for_one: bool, amount_in: U256) -> Self {
        // 负数表示 exact input
        let amount_specified = I256::from_raw(amount_in).checked_neg().unwrap_or(I256::zero());

        // sqrt price limit
        let sqrt_price_limit_x96 = if zero_for_one {
            // 向下交换，设置最小价格
            U256::from(4295128739u64) // MIN_SQRT_RATIO + 1
        } else {
            // 向上交换，设置最大价格
            U256::from_dec_str("1461446703485210103287273052203988822378723970342").unwrap() // MAX_SQRT_RATIO - 1
        };

        Self {
            zero_for_one,
            amount_specified,
            sqrt_price_limit_x96,
        }
    }

    /// 创建 ExactOut swap 参数
    pub fn exact_output(zero_for_one: bool, amount_out: U256) -> Self {
        // 正数表示 exact output
        let amount_specified = I256::from_raw(amount_out);

        let sqrt_price_limit_x96 = if zero_for_one {
            U256::from(4295128739u64)
        } else {
            U256::from_dec_str("1461446703485210103287273052203988822378723970342").unwrap()
        };

        Self {
            zero_for_one,
            amount_specified,
            sqrt_price_limit_x96,
        }
    }

    /// 编码为 ABI Token
    pub fn to_abi_token(&self) -> Token {
        Token::Tuple(vec![
            Token::Bool(self.zero_for_one),
            Token::Int(self.amount_specified.into_raw()),
            Token::Uint(self.sqrt_price_limit_x96),
        ])
    }
}

/// Uniswap V4 Flash Accounting 模块
/// 支持零成本闪电贷和多跳套利
pub struct FlashAccounting<M: Middleware> {
    provider: Arc<M>,
    pool_manager: Address,
}

impl<M: Middleware + 'static> FlashAccounting<M> {
    pub fn new(provider: Arc<M>) -> Self {
        Self {
            provider,
            pool_manager: *v4_addresses::POOL_MANAGER,
        }
    }

    /// V4 Flash Accounting 允许在一个交易中：
    /// 1. unlock() - 开启会计模式
    /// 2. 执行多个 swap - 累积 delta
    /// 3. settle/take - 结算所有 delta
    ///
    /// 关键优势：不需要实际转移代币直到最后结算
    pub fn encode_flash_accounting_swap(
        &self,
        swaps: Vec<(PoolKey, SwapParams, Vec<u8>)>,
    ) -> Result<Bytes> {
        // 编码多个 swap 操作
        let mut swap_calls = Vec::new();
        for (pool_key, params, hook_data) in swaps {
            swap_calls.push(Token::Tuple(vec![
                pool_key.to_abi_token(),
                params.to_abi_token(),
                Token::Bytes(hook_data),
            ]));
        }

        let encoded = encode(&[Token::Array(swap_calls)]);
        Ok(Bytes::from(encoded))
    }

    /// 获取 PoolManager 基础合约
    pub fn get_pool_manager(&self) -> UniswapV4PoolManagerBase<M> {
        UniswapV4PoolManagerBase::new(self.pool_manager, self.provider.clone())
    }
}

/// Uniswap V4 协议实现
pub struct UniswapV4Protocol<M: Middleware> {
    provider: Arc<M>,
    pool_manager: Address,
    state_view: Address,
    quoter: Address,
    chain_id: u64,
}

impl<M: Middleware + 'static> UniswapV4Protocol<M> {
    pub fn new(provider: Arc<M>, chain_id: u64) -> Self {
        Self {
            provider,
            pool_manager: *v4_addresses::POOL_MANAGER,
            state_view: *v4_addresses::STATE_VIEW,
            quoter: *v4_addresses::QUOTER,
            chain_id,
        }
    }

    pub fn with_addresses(
        provider: Arc<M>,
        pool_manager: Address,
        state_view: Address,
        quoter: Address,
        chain_id: u64,
    ) -> Self {
        Self {
            provider,
            pool_manager,
            state_view,
            quoter,
            chain_id,
        }
    }

    /// 获取 PoolManager 合约
    fn get_pool_manager(&self) -> UniswapV4PoolManagerBase<M> {
        UniswapV4PoolManagerBase::new(self.pool_manager, self.provider.clone())
    }

    /// 获取 StateView 合约
    fn get_state_view(&self) -> UniswapV4StateView<M> {
        UniswapV4StateView::new(self.state_view, self.provider.clone())
    }

    /// 获取池子 slot0 数据
    pub async fn get_slot0(&self, pool_id: [u8; 32]) -> Result<(U256, i32, u32, u32)> {
        let state_view = self.get_state_view();
        let pool_id_h256 = H256::from(pool_id);

        let result = state_view.get_slot_0(pool_id_h256.into()).call().await?;

        Ok((
            U256::from(result.0), // sqrtPriceX96
            result.1,              // tick
            result.2.into(),       // protocolFee
            result.3.into(),       // lpFee
        ))
    }

    /// 获取池子流动性
    pub async fn get_liquidity(&self, pool_id: [u8; 32]) -> Result<u128> {
        let state_view = self.get_state_view();
        let pool_id_h256 = H256::from(pool_id);

        let liquidity = state_view.get_liquidity(pool_id_h256.into()).call().await?;
        Ok(liquidity)
    }

    /// 获取费用增长全局值
    pub async fn get_fee_growth_globals(&self, pool_id: [u8; 32]) -> Result<(U256, U256)> {
        let state_view = self.get_state_view();
        let pool_id_h256 = H256::from(pool_id);

        let result = state_view.get_fee_growth_globals(pool_id_h256.into()).call().await?;
        Ok((result.0, result.1))
    }

    /// 使用原始 eth_call 获取精确输入报价
    /// 由于 V4 Quoter 使用复杂的 tuple 参数，这里使用低级 ABI 编码
    pub async fn quote_exact_input_single(
        &self,
        pool_key: &PoolKey,
        zero_for_one: bool,
        amount_in: U256,
        _hook_data: Vec<u8>,
    ) -> Result<U256> {
        // 编码函数调用
        // function quoteExactInputSingle(PoolKey poolKey, bool zeroForOne, uint256 exactAmount, bytes hookData)
        let selector = ethers::utils::keccak256(
            "quoteExactInputSingle((address,address,uint24,int24,address),bool,uint256,bytes)"
        );

        let params = encode(&[
            pool_key.to_abi_token(),
            Token::Bool(zero_for_one),
            Token::Uint(amount_in),
            Token::Bytes(vec![]),
        ]);

        let mut calldata = selector[..4].to_vec();
        calldata.extend(params);

        // 执行 eth_call
        let tx = TransactionRequest::new()
            .to(self.quoter)
            .data(calldata);

        let result = self.provider.call(&tx.into(), None).await?;

        // 解码结果: (int128[] deltaAmounts, uint160 sqrtPriceX96After, uint32 initializedTicksLoaded)
        // deltaAmounts[1] 是输出代币的 delta (负值表示输出)
        if result.len() >= 64 {
            // 简单解析：跳过动态数组偏移，获取第一个元素
            let output_index = if zero_for_one { 1 } else { 0 };
            let offset = 32 + output_index * 32; // 跳过数组长度
            if result.len() >= offset + 32 {
                let delta_bytes: [u8; 32] = result[offset..offset+32].try_into().unwrap_or([0u8; 32]);
                let delta = I256::from_raw(U256::from_big_endian(&delta_bytes));

                // 取绝对值
                let amount_out = if delta < I256::zero() {
                    U256::from((-delta.as_i128()) as u128)
                } else {
                    U256::from(delta.as_i128() as u128)
                };
                return Ok(amount_out);
            }
        }

        Err(anyhow!("Failed to decode quoter response"))
    }

    /// 使用原始 eth_call 获取精确输出报价
    pub async fn quote_exact_output_single(
        &self,
        pool_key: &PoolKey,
        zero_for_one: bool,
        amount_out: U256,
        _hook_data: Vec<u8>,
    ) -> Result<U256> {
        let selector = ethers::utils::keccak256(
            "quoteExactOutputSingle((address,address,uint24,int24,address),bool,uint256,bytes)"
        );

        let params = encode(&[
            pool_key.to_abi_token(),
            Token::Bool(zero_for_one),
            Token::Uint(amount_out),
            Token::Bytes(vec![]),
        ]);

        let mut calldata = selector[..4].to_vec();
        calldata.extend(params);

        let tx = TransactionRequest::new()
            .to(self.quoter)
            .data(calldata);

        let result = self.provider.call(&tx.into(), None).await?;

        if result.len() >= 64 {
            let input_index = if zero_for_one { 0 } else { 1 };
            let offset = 32 + input_index * 32;
            if result.len() >= offset + 32 {
                let delta_bytes: [u8; 32] = result[offset..offset+32].try_into().unwrap_or([0u8; 32]);
                let amount_in = U256::from_big_endian(&delta_bytes);
                return Ok(amount_in);
            }
        }

        Err(anyhow!("Failed to decode quoter response"))
    }

    /// 从 sqrtPriceX96 计算价格
    pub fn sqrt_price_x96_to_price(sqrt_price_x96: U256, decimals0: u8, decimals1: u8) -> f64 {
        let sqrt_price = sqrt_price_x96.as_u128() as f64 / (2_u128.pow(96) as f64);
        let price = sqrt_price * sqrt_price;
        let decimal_adjustment = 10_f64.powi(decimals0 as i32 - decimals1 as i32);
        price * decimal_adjustment
    }

    /// 获取 Flash Accounting 模块
    pub fn flash_accounting(&self) -> FlashAccounting<M> {
        FlashAccounting::new(self.provider.clone())
    }

    /// 编码 unlock 调用数据
    pub fn encode_unlock_callback(&self, callback_data: Vec<u8>) -> Result<Bytes> {
        let encoded = encode(&[Token::Bytes(callback_data)]);
        Ok(Bytes::from(encoded))
    }

    /// 从 extsload 读取存储槽
    pub async fn read_storage_slot(&self, slot: H256) -> Result<H256> {
        let pool_manager = self.get_pool_manager();
        let result = pool_manager.extsload(slot.into()).call().await?;
        Ok(H256::from(result))
    }

    /// 获取完整的池子状态
    pub async fn get_pool_state_by_key(&self, pool_key: &PoolKey) -> Result<UniswapV4PoolState> {
        let pool_id = pool_key.to_id();

        let slot0 = self.get_slot0(pool_id).await?;
        let liquidity = self.get_liquidity(pool_id).await?;
        let fee_growth = self.get_fee_growth_globals(pool_id).await?;

        let pool = Pool {
            address: self.pool_manager, // V4 使用单例合约
            dex_type: DexType::UniswapV4,
            token0: pool_key.currency0,
            token1: pool_key.currency1,
            fee: pool_key.fee,
            chain_id: self.chain_id,
        };

        let v4_pool_key = UniswapV4PoolKey {
            currency0: pool_key.currency0,
            currency1: pool_key.currency1,
            fee: pool_key.fee,
            tick_spacing: pool_key.tick_spacing,
            hooks: pool_key.hooks,
        };

        Ok(UniswapV4PoolState {
            pool,
            pool_key: v4_pool_key,
            sqrt_price_x96: slot0.0,
            tick: slot0.1,
            liquidity,
            fee_growth_global0_x128: fee_growth.0,
            fee_growth_global1_x128: fee_growth.1,
            protocol_fee: slot0.2,
        })
    }

    /// 编码 swap 调用数据
    pub fn encode_swap(&self, pool_key: &PoolKey, params: &SwapParams, hook_data: Vec<u8>) -> Bytes {
        // function swap(PoolKey key, SwapParams params, bytes hookData)
        let selector = ethers::utils::keccak256(
            "swap((address,address,uint24,int24,address),(bool,int256,uint160),bytes)"
        );

        let encoded_params = encode(&[
            pool_key.to_abi_token(),
            params.to_abi_token(),
            Token::Bytes(hook_data),
        ]);

        let mut calldata = selector[..4].to_vec();
        calldata.extend(encoded_params);

        Bytes::from(calldata)
    }

    /// 编码 initialize 调用数据
    pub fn encode_initialize(&self, pool_key: &PoolKey, sqrt_price_x96: U256) -> Bytes {
        let selector = ethers::utils::keccak256(
            "initialize((address,address,uint24,int24,address),uint160)"
        );

        let encoded_params = encode(&[
            pool_key.to_abi_token(),
            Token::Uint(sqrt_price_x96),
        ]);

        let mut calldata = selector[..4].to_vec();
        calldata.extend(encoded_params);

        Bytes::from(calldata)
    }
}

#[async_trait]
impl<M: Middleware + 'static> DexProtocol for UniswapV4Protocol<M> {
    fn dex_type(&self) -> DexType {
        DexType::UniswapV4
    }

    async fn get_pool_state(&self, _pool_address: Address) -> Result<PoolState> {
        // V4 不使用单独的池子地址，需要使用 PoolKey
        // 这里返回错误，实际使用时应该用 get_pool_state_by_key
        Err(anyhow!("V4 requires PoolKey, use get_pool_state_by_key instead"))
    }

    async fn get_amount_out(
        &self,
        _pool_address: Address,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> Result<U256> {
        // 创建默认的 PoolKey (无 hooks, 标准费率)
        let pool_key = PoolKey::new(
            token_in,
            token_out,
            3000, // 0.3% fee
            60,   // 标准 tick spacing
            Address::zero(),
        );

        let zero_for_one = token_in < token_out;
        self.quote_exact_input_single(&pool_key, zero_for_one, amount_in, vec![]).await
    }

    async fn get_amount_in(
        &self,
        _pool_address: Address,
        token_in: Address,
        token_out: Address,
        amount_out: U256,
    ) -> Result<U256> {
        let pool_key = PoolKey::new(
            token_in,
            token_out,
            3000,
            60,
            Address::zero(),
        );

        let zero_for_one = token_in < token_out;
        self.quote_exact_output_single(&pool_key, zero_for_one, amount_out, vec![]).await
    }

    async fn get_pool_tokens(&self, _pool_address: Address) -> Result<(Address, Address)> {
        // V4 不使用单独的池子地址
        Err(anyhow!("V4 requires PoolKey to identify pool tokens"))
    }

    async fn get_pool_fee(&self, _pool_address: Address) -> Result<u32> {
        // V4 费率在 PoolKey 中定义
        Err(anyhow!("V4 requires PoolKey to get fee"))
    }
}

/// V4 Hooks 配置
#[derive(Debug, Clone, Default)]
pub struct HooksConfig {
    /// 是否在 swap 前调用 beforeSwap
    pub before_swap: bool,
    /// 是否在 swap 后调用 afterSwap
    pub after_swap: bool,
    /// 是否在添加流动性前调用
    pub before_add_liquidity: bool,
    /// 是否在添加流动性后调用
    pub after_add_liquidity: bool,
    /// 是否在移除流动性前调用
    pub before_remove_liquidity: bool,
    /// 是否在移除流动性后调用
    pub after_remove_liquidity: bool,
    /// 是否在初始化前调用
    pub before_initialize: bool,
    /// 是否在初始化后调用
    pub after_initialize: bool,
    /// 是否在 donate 前调用
    pub before_donate: bool,
    /// 是否在 donate 后调用
    pub after_donate: bool,
}

impl HooksConfig {
    /// 从 hooks 地址解析配置
    /// V4 的 hooks 地址编码了启用的 hooks 类型
    pub fn from_address(hooks: Address) -> Self {
        let flags = hooks.as_bytes()[0] as u16 | ((hooks.as_bytes()[1] as u16) << 8);

        Self {
            before_swap: (flags & (1 << 7)) != 0,
            after_swap: (flags & (1 << 6)) != 0,
            before_add_liquidity: (flags & (1 << 11)) != 0,
            after_add_liquidity: (flags & (1 << 10)) != 0,
            before_remove_liquidity: (flags & (1 << 9)) != 0,
            after_remove_liquidity: (flags & (1 << 8)) != 0,
            before_initialize: (flags & (1 << 13)) != 0,
            after_initialize: (flags & (1 << 12)) != 0,
            before_donate: (flags & (1 << 5)) != 0,
            after_donate: (flags & (1 << 4)) != 0,
        }
    }

    /// 检查是否有任何 hook 启用
    pub fn has_any_hook(&self) -> bool {
        self.before_swap
            || self.after_swap
            || self.before_add_liquidity
            || self.after_add_liquidity
            || self.before_remove_liquidity
            || self.after_remove_liquidity
            || self.before_initialize
            || self.after_initialize
            || self.before_donate
            || self.after_donate
    }
}

/// V4 套利路径构建器
pub struct V4ArbitragePathBuilder {
    swaps: Vec<(PoolKey, SwapParams, Vec<u8>)>,
}

impl V4ArbitragePathBuilder {
    pub fn new() -> Self {
        Self { swaps: Vec::new() }
    }

    /// 添加一个 swap 步骤
    pub fn add_swap(
        mut self,
        currency0: Address,
        currency1: Address,
        fee: u32,
        tick_spacing: i32,
        hooks: Address,
        zero_for_one: bool,
        amount: U256,
        is_exact_input: bool,
    ) -> Self {
        let pool_key = PoolKey::new(currency0, currency1, fee, tick_spacing, hooks);

        let params = if is_exact_input {
            SwapParams::exact_input(zero_for_one, amount)
        } else {
            SwapParams::exact_output(zero_for_one, amount)
        };

        self.swaps.push((pool_key, params, vec![]));
        self
    }

    /// 添加带有 hook data 的 swap
    pub fn add_swap_with_hook_data(
        mut self,
        pool_key: PoolKey,
        params: SwapParams,
        hook_data: Vec<u8>,
    ) -> Self {
        self.swaps.push((pool_key, params, hook_data));
        self
    }

    /// 构建最终的 swap 序列
    pub fn build(self) -> Vec<(PoolKey, SwapParams, Vec<u8>)> {
        self.swaps
    }

    /// 创建三角套利路径
    /// 例如: USDT -> ETH -> DAI -> USDT
    pub fn triangular_arbitrage(
        token_a: Address,
        token_b: Address,
        token_c: Address,
        amount: U256,
        fee: u32,
        tick_spacing: i32,
    ) -> Vec<(PoolKey, SwapParams, Vec<u8>)> {
        let hooks = Address::zero();

        Self::new()
            .add_swap(token_a, token_b, fee, tick_spacing, hooks, token_a < token_b, amount, true)
            .add_swap(token_b, token_c, fee, tick_spacing, hooks, token_b < token_c, U256::zero(), true)
            .add_swap(token_c, token_a, fee, tick_spacing, hooks, token_c < token_a, U256::zero(), true)
            .build()
    }
}

impl Default for V4ArbitragePathBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_pool_key_ordering() {
        let addr1 = Address::from_str("0x0000000000000000000000000000000000000001").unwrap();
        let addr2 = Address::from_str("0x0000000000000000000000000000000000000002").unwrap();

        // 无论输入顺序如何，currency0 总是 < currency1
        let key1 = PoolKey::new(addr1, addr2, 3000, 60, Address::zero());
        let key2 = PoolKey::new(addr2, addr1, 3000, 60, Address::zero());

        assert_eq!(key1.currency0, key2.currency0);
        assert_eq!(key1.currency1, key2.currency1);
        assert!(key1.currency0 < key1.currency1);
    }

    #[test]
    fn test_pool_id_calculation() {
        let addr1 = Address::from_str("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48").unwrap(); // USDC
        let addr2 = Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap(); // WETH

        let key = PoolKey::new(addr1, addr2, 3000, 60, Address::zero());
        let pool_id = key.to_id();

        assert_eq!(pool_id.len(), 32);
    }

    #[test]
    fn test_hooks_config_parsing() {
        let no_hooks = HooksConfig::from_address(Address::zero());
        assert!(!no_hooks.has_any_hook());
    }

    #[test]
    fn test_swap_params_exact_input() {
        let params = SwapParams::exact_input(true, U256::from(1000));
        assert!(params.zero_for_one);
        assert!(params.amount_specified < I256::zero()); // 负数表示 exact input
    }

    #[test]
    fn test_swap_params_exact_output() {
        let params = SwapParams::exact_output(false, U256::from(1000));
        assert!(!params.zero_for_one);
        assert!(params.amount_specified > I256::zero()); // 正数表示 exact output
    }
}
