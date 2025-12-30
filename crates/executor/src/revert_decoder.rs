//! Solidity Revert 错误解码器
//!
//! 用于解析合约 revert 时返回的错误信息，提供可读的错误原因

use ethers::abi::{self, Token};
use ethers::types::{I256, U256};
use std::collections::HashMap;
use tracing::{debug, warn};

// 自定义错误选择器常量
const SELECTOR_ERROR_STRING: [u8; 4] = [0x08, 0xc3, 0x79, 0xa0];
const SELECTOR_PANIC: [u8; 4] = [0x4e, 0x48, 0x7b, 0x71];
// ArbitrageFailed_Detailed(string,address,address,address,uint256,uint256,uint256,uint256,uint256,int256)
const SELECTOR_ARBITRAGE_FAILED_DETAILED: [u8; 4] = [0x38, 0x4f, 0xd5, 0x83];
// ProfitBelowMinimum(uint256,uint256,uint256,uint256)
const SELECTOR_PROFIT_BELOW_MINIMUM: [u8; 4] = [0xcc, 0x9c, 0x44, 0x04];

// 已知的错误签名映射
lazy_static::lazy_static! {
    static ref ERROR_SIGNATURES: HashMap<[u8; 4], &'static str> = {
        let mut m = HashMap::new();
        // 标准 Error(string) 选择器: 0x08c379a0
        m.insert(SELECTOR_ERROR_STRING, "Error(string)");
        // Panic(uint256) 选择器: 0x4e487b71
        m.insert(SELECTOR_PANIC, "Panic(uint256)");
        // 自定义套利错误
        m.insert(SELECTOR_ARBITRAGE_FAILED_DETAILED, "ArbitrageFailed_Detailed(string,address,address,address,uint256,uint256,uint256,uint256,uint256,int256)");
        m.insert(SELECTOR_PROFIT_BELOW_MINIMUM, "ProfitBelowMinimum(uint256,uint256,uint256,uint256)");
        m
    };

    // Panic 错误代码映射
    static ref PANIC_CODES: HashMap<u64, &'static str> = {
        let mut m = HashMap::new();
        m.insert(0x00, "通用/未定义错误");
        m.insert(0x01, "断言失败 (assert)");
        m.insert(0x11, "算术溢出/下溢");
        m.insert(0x12, "除以零");
        m.insert(0x21, "无效的枚举值");
        m.insert(0x22, "存储字节数组编码错误");
        m.insert(0x31, "空数组 pop");
        m.insert(0x32, "数组越界");
        m.insert(0x41, "内存分配过大");
        m.insert(0x51, "调用了未初始化的内部函数");
        m
    };
}

/// 解码后的错误信息
#[derive(Debug, Clone)]
pub struct DecodedRevertError {
    /// 错误类型
    pub error_type: RevertErrorType,
    /// 可读的错误消息
    pub message: String,
    /// 原始错误数据 (hex)
    pub raw_data: String,
    /// 详细分析
    pub analysis: Option<ErrorAnalysis>,
}

/// 错误类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevertErrorType {
    /// 标准 Error(string) revert
    ErrorString,
    /// Panic 错误
    Panic,
    /// 自定义错误
    CustomError,
    /// 无数据的 revert
    EmptyRevert,
    /// 未知格式
    Unknown,
}

/// 错误分析 - 针对套利特定错误的详细分析
#[derive(Debug, Clone)]
pub struct ErrorAnalysis {
    /// 可能的原因
    pub possible_causes: Vec<String>,
    /// 建议的修复措施
    pub suggestions: Vec<String>,
    /// 是否可重试
    pub is_retryable: bool,
}

/// Revert 错误解码器
pub struct RevertDecoder;

impl RevertDecoder {
    /// 从错误字符串中提取并解码 revert 数据
    ///
    /// 支持多种格式：
    /// - 直接的 hex 字符串
    /// - ethers 错误消息中嵌入的 revert 数据
    /// - ContractError 格式
    pub fn decode_from_error_string(error: &str) -> DecodedRevertError {
        debug!("解码错误字符串: {}", error);

        // 尝试从错误消息中提取 hex 数据
        if let Some(hex_data) = Self::extract_hex_from_error(error) {
            return Self::decode_revert_data(&hex_data);
        }

        // 如果无法提取 hex 数据，尝试直接解析错误消息
        Self::parse_error_message(error)
    }

    /// 解码 revert 数据 (bytes)
    pub fn decode_revert_data(data: &[u8]) -> DecodedRevertError {
        if data.is_empty() {
            return DecodedRevertError {
                error_type: RevertErrorType::EmptyRevert,
                message: "空 revert (无错误消息)".to_string(),
                raw_data: "0x".to_string(),
                analysis: Some(ErrorAnalysis {
                    possible_causes: vec![
                        "require() 条件失败但没有提供消息".to_string(),
                        "revert() 被调用但没有参数".to_string(),
                    ],
                    suggestions: vec![
                        "检查合约中的 require 语句".to_string(),
                    ],
                    is_retryable: false,
                }),
            };
        }

        let raw_hex = format!("0x{}", hex::encode(data));

        // 检查是否有函数选择器 (至少 4 字节)
        if data.len() < 4 {
            return DecodedRevertError {
                error_type: RevertErrorType::Unknown,
                message: format!("数据太短，无法解析: {}", raw_hex),
                raw_data: raw_hex,
                analysis: None,
            };
        }

        let selector: [u8; 4] = data[0..4].try_into().unwrap();
        let payload = &data[4..];

        // 检查是否是标准 Error(string)
        if selector == SELECTOR_ERROR_STRING {
            return Self::decode_error_string(payload, raw_hex);
        }

        // 检查是否是 Panic(uint256)
        if selector == SELECTOR_PANIC {
            return Self::decode_panic(payload, raw_hex);
        }

        // 检查是否是 ArbitrageFailed_Detailed
        if selector == SELECTOR_ARBITRAGE_FAILED_DETAILED {
            return Self::decode_arbitrage_failed_detailed(payload, raw_hex);
        }

        // 检查是否是 ProfitBelowMinimum
        if selector == SELECTOR_PROFIT_BELOW_MINIMUM {
            return Self::decode_profit_below_minimum(payload, raw_hex);
        }

        // 未知的自定义错误
        DecodedRevertError {
            error_type: RevertErrorType::CustomError,
            message: format!("自定义错误 (选择器: 0x{})", hex::encode(selector)),
            raw_data: raw_hex,
            analysis: Some(ErrorAnalysis {
                possible_causes: vec![
                    "合约使用了自定义 error 类型".to_string(),
                ],
                suggestions: vec![
                    "查看合约 ABI 以解码此错误".to_string(),
                ],
                is_retryable: false,
            }),
        }
    }

    /// 解码标准 Error(string)
    fn decode_error_string(payload: &[u8], raw_hex: String) -> DecodedRevertError {
        // ABI 解码 string
        match abi::decode(&[abi::ParamType::String], payload) {
            Ok(tokens) => {
                if let Some(Token::String(msg)) = tokens.first() {
                    let analysis = Self::analyze_arbitrage_error(msg);
                    return DecodedRevertError {
                        error_type: RevertErrorType::ErrorString,
                        message: msg.clone(),
                        raw_data: raw_hex,
                        analysis: Some(analysis),
                    };
                }
            }
            Err(e) => {
                warn!("解码 Error(string) 失败: {:?}", e);
            }
        }

        // 尝试直接从 payload 提取 UTF-8 字符串
        if let Some(msg) = Self::try_extract_utf8(payload) {
            let analysis = Self::analyze_arbitrage_error(&msg);
            return DecodedRevertError {
                error_type: RevertErrorType::ErrorString,
                message: msg,
                raw_data: raw_hex,
                analysis: Some(analysis),
            };
        }

        DecodedRevertError {
            error_type: RevertErrorType::ErrorString,
            message: "Error(string) 但无法解码消息".to_string(),
            raw_data: raw_hex,
            analysis: None,
        }
    }

    /// 解码 Panic(uint256)
    fn decode_panic(payload: &[u8], raw_hex: String) -> DecodedRevertError {
        match abi::decode(&[abi::ParamType::Uint(256)], payload) {
            Ok(tokens) => {
                if let Some(Token::Uint(code)) = tokens.first() {
                    let code_u64 = code.as_u64();
                    let description = PANIC_CODES
                        .get(&code_u64)
                        .unwrap_or(&"未知 Panic 代码");

                    return DecodedRevertError {
                        error_type: RevertErrorType::Panic,
                        message: format!("Panic(0x{:02x}): {}", code_u64, description),
                        raw_data: raw_hex,
                        analysis: Some(ErrorAnalysis {
                            possible_causes: vec![
                                format!("Solidity Panic 代码 0x{:02x}", code_u64),
                                description.to_string(),
                            ],
                            suggestions: vec![
                                "这通常是合约内部逻辑错误".to_string(),
                                "检查是否有溢出/下溢或数组越界".to_string(),
                            ],
                            is_retryable: false,
                        }),
                    };
                }
            }
            Err(e) => {
                warn!("解码 Panic(uint256) 失败: {:?}", e);
            }
        }

        DecodedRevertError {
            error_type: RevertErrorType::Panic,
            message: "Panic 但无法解码代码".to_string(),
            raw_data: raw_hex,
            analysis: None,
        }
    }

    /// 解码 ArbitrageFailed_Detailed(string,address,address,address,uint256,uint256,uint256,uint256,uint256,int256)
    fn decode_arbitrage_failed_detailed(payload: &[u8], raw_hex: String) -> DecodedRevertError {
        use ethers::types::Address;

        // ABI 解码参数
        let param_types = vec![
            abi::ParamType::String,     // reason
            abi::ParamType::Address,    // tokenA
            abi::ParamType::Address,    // tokenB
            abi::ParamType::Address,    // tokenC
            abi::ParamType::Uint(256),  // inputAmount
            abi::ParamType::Uint(256),  // step1Out
            abi::ParamType::Uint(256),  // step2Out
            abi::ParamType::Uint(256),  // step3Out
            abi::ParamType::Uint(256),  // amountOwed
            abi::ParamType::Int(256),   // profitOrLoss
        ];

        match abi::decode(&param_types, payload) {
            Ok(tokens) => {
                let reason = tokens.get(0).and_then(|t| {
                    if let Token::String(s) = t { Some(s.clone()) } else { None }
                }).unwrap_or_else(|| "未知原因".to_string());

                let token_a = tokens.get(1).and_then(|t| {
                    if let Token::Address(a) = t { Some(*a) } else { None }
                }).unwrap_or(Address::zero());

                let token_b = tokens.get(2).and_then(|t| {
                    if let Token::Address(a) = t { Some(*a) } else { None }
                }).unwrap_or(Address::zero());

                let token_c = tokens.get(3).and_then(|t| {
                    if let Token::Address(a) = t { Some(*a) } else { None }
                }).unwrap_or(Address::zero());

                let input_amount = tokens.get(4).and_then(|t| {
                    if let Token::Uint(v) = t { Some(*v) } else { None }
                }).unwrap_or_default();

                let step1_out = tokens.get(5).and_then(|t| {
                    if let Token::Uint(v) = t { Some(*v) } else { None }
                }).unwrap_or_default();

                let step2_out = tokens.get(6).and_then(|t| {
                    if let Token::Uint(v) = t { Some(*v) } else { None }
                }).unwrap_or_default();

                let step3_out = tokens.get(7).and_then(|t| {
                    if let Token::Uint(v) = t { Some(*v) } else { None }
                }).unwrap_or_default();

                let amount_owed = tokens.get(8).and_then(|t| {
                    if let Token::Uint(v) = t { Some(*v) } else { None }
                }).unwrap_or_default();

                let profit_or_loss: I256 = tokens.get(9).and_then(|t| {
                    if let Token::Int(v) = t { Some(I256::from_raw(*v)) } else { None }
                }).unwrap_or(I256::zero());

                // 将 I256 转换为 i128 用于显示
                let profit_i128: i128 = profit_or_loss.low_i128();

                // 根据代币地址获取符号和精度
                let get_token_info = |addr: Address| -> (&'static str, u8) {
                    // 常见代币地址映射 (ETH Mainnet)
                    let addr_lower = format!("{:?}", addr).to_lowercase();
                    match addr_lower.as_str() {
                        "0xdac17f958d2ee523a2206206994597c13d831ec7" => ("USDT", 6),
                        "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48" => ("USDC", 6),
                        "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2" => ("WETH", 18),
                        "0x6b175474e89094c44da98b954eedeac495271d0f" => ("DAI", 18),
                        "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599" => ("WBTC", 8),
                        _ => ("UNKNOWN", 18), // 默认 18 位精度
                    }
                };

                let (symbol_a, decimals_a) = get_token_info(token_a);
                let (symbol_b, decimals_b) = get_token_info(token_b);
                let (symbol_c, decimals_c) = get_token_info(token_c);

                // 格式化金额
                let format_amount = |amount: U256, decimals: u8| -> String {
                    let divisor = 10_u128.pow(decimals as u32) as f64;
                    let formatted = amount.as_u128() as f64 / divisor;
                    format!("{:.4}", formatted)
                };

                let format_signed = |amount: i128, decimals: u8| -> String {
                    let divisor = 10_u128.pow(decimals as u32) as f64;
                    let abs_amount = amount.unsigned_abs() as f64 / divisor;
                    let sign = if amount < 0 { "-" } else { "" };
                    format!("{}{:.4}", sign, abs_amount)
                };

                let message = format!(
                    "套利失败: {}\n\
                     ├─ 代币路径: {} → {} → {} → {}\n\
                     ├─ 借入数量: {} {}\n\
                     ├─ Step1 输出 ({}→{}): {} {}\n\
                     ├─ Step2 输出 ({}→{}): {} {}\n\
                     ├─ Step3 输出 ({}→{}): {} {}\n\
                     ├─ 需归还数量: {} {}\n\
                     └─ 盈亏: {} {}",
                    reason,
                    symbol_a, symbol_b, symbol_c, symbol_a,
                    format_amount(input_amount, decimals_a), symbol_a,
                    symbol_a, symbol_b, format_amount(step1_out, decimals_b), symbol_b,
                    symbol_b, symbol_c, format_amount(step2_out, decimals_c), symbol_c,
                    symbol_c, symbol_a, format_amount(step3_out, decimals_a), symbol_a,
                    format_amount(amount_owed, decimals_a), symbol_a,
                    format_signed(profit_i128, decimals_a), symbol_a
                );

                // 计算缺口金额
                let shortfall = if amount_owed > step3_out {
                    let diff = (amount_owed - step3_out).as_u128() as f64;
                    diff / 10_u128.pow(decimals_a as u32) as f64
                } else {
                    0.0
                };

                DecodedRevertError {
                    error_type: RevertErrorType::CustomError,
                    message,
                    raw_data: raw_hex,
                    analysis: Some(ErrorAnalysis {
                        possible_causes: vec![
                            format!("失败原因: {}", reason),
                            format!("输出不足: 需要 {} {} 但只有 {} {}，缺口 {:.4} {}",
                                format_amount(amount_owed, decimals_a), symbol_a,
                                format_amount(step3_out, decimals_a), symbol_a,
                                shortfall, symbol_a),
                            format!("亏损数量: {} {}", format_signed(profit_i128, decimals_a), symbol_a),
                        ],
                        suggestions: vec![
                            "价格可能在执行期间变化，导致输出减少".to_string(),
                            "可能被其他套利者抢先 (frontrun)".to_string(),
                            "增加利润阈值以确保足够的安全边际".to_string(),
                        ],
                        is_retryable: true,
                    }),
                }
            }
            Err(e) => {
                warn!("解码 ArbitrageFailed_Detailed 失败: {:?}", e);
                DecodedRevertError {
                    error_type: RevertErrorType::CustomError,
                    message: "ArbitrageFailed_Detailed 但无法解码参数".to_string(),
                    raw_data: raw_hex,
                    analysis: None,
                }
            }
        }
    }

    /// 解码 ProfitBelowMinimum(uint256,uint256,uint256,uint256)
    fn decode_profit_below_minimum(payload: &[u8], raw_hex: String) -> DecodedRevertError {
        // ABI 解码参数: (uint256, uint256, uint256, uint256)
        let param_types = vec![
            abi::ParamType::Uint(256), // actualProfit
            abi::ParamType::Uint(256), // minRequired
            abi::ParamType::Uint(256), // inputAmount
            abi::ParamType::Uint(256), // outputAmount
        ];

        match abi::decode(&param_types, payload) {
            Ok(tokens) => {
                let actual_profit = tokens.get(0).and_then(|t| {
                    if let Token::Uint(v) = t { Some(*v) } else { None }
                }).unwrap_or_default();

                let min_required = tokens.get(1).and_then(|t| {
                    if let Token::Uint(v) = t { Some(*v) } else { None }
                }).unwrap_or_default();

                let input_amount = tokens.get(2).and_then(|t| {
                    if let Token::Uint(v) = t { Some(*v) } else { None }
                }).unwrap_or_default();

                let output_amount = tokens.get(3).and_then(|t| {
                    if let Token::Uint(v) = t { Some(*v) } else { None }
                }).unwrap_or_default();

                // 格式化金额 (假设 6 位精度)
                let format_u256 = |amount: U256| -> String {
                    let dec6 = amount.as_u128() as f64 / 1_000_000.0;
                    format!("{:.4}", dec6)
                };

                let message = format!(
                    "利润不足\n\
                     ├─ 实际利润: {} | {} (6位精度)\n\
                     ├─ 最低要求: {} | {} (6位精度)\n\
                     ├─ 输入数量: {} | {} (6位精度)\n\
                     └─ 输出数量: {} | {} (6位精度)",
                    actual_profit, format_u256(actual_profit),
                    min_required, format_u256(min_required),
                    input_amount, format_u256(input_amount),
                    output_amount, format_u256(output_amount)
                );

                DecodedRevertError {
                    error_type: RevertErrorType::CustomError,
                    message,
                    raw_data: raw_hex,
                    analysis: Some(ErrorAnalysis {
                        possible_causes: vec![
                            format!("利润 {} 低于最低要求 {}", format_u256(actual_profit), format_u256(min_required)),
                            "可能原因: 价格变动导致利润减少".to_string(),
                            "可能原因: gas 成本或闪电贷费用侵蚀了利润".to_string(),
                        ],
                        suggestions: vec![
                            "调整最小利润阈值".to_string(),
                            "选择费率更低的闪电贷池".to_string(),
                            "提高利润筛选门槛以避免边际套利".to_string(),
                        ],
                        is_retryable: false,
                    }),
                }
            }
            Err(e) => {
                warn!("解码 ProfitBelowMinimum 失败: {:?}", e);
                DecodedRevertError {
                    error_type: RevertErrorType::CustomError,
                    message: "ProfitBelowMinimum 但无法解码参数".to_string(),
                    raw_data: raw_hex,
                    analysis: None,
                }
            }
        }
    }

    /// 分析套利相关错误
    fn analyze_arbitrage_error(message: &str) -> ErrorAnalysis {
        let msg_lower = message.to_lowercase();

        // 输出不足以偿还闪电贷
        if msg_lower.contains("insufficient output") || msg_lower.contains("repayment") {
            return ErrorAnalysis {
                possible_causes: vec![
                    "三角套利输出不足以偿还闪电贷本金+手续费".to_string(),
                    "可能原因1: 价格在发现机会和执行之间发生了变化".to_string(),
                    "可能原因2: 被其他套利者抢先执行 (frontrun)".to_string(),
                    "可能原因3: 滑点导致实际输出低于预期".to_string(),
                    "可能原因4: 预估利润计算不准确".to_string(),
                ],
                suggestions: vec![
                    "检查执行时的实时价格与发现时的价格差异".to_string(),
                    "增加利润阈值以确保足够的安全边际".to_string(),
                    "考虑使用 Flashbots 防止被 frontrun".to_string(),
                    "减少执行延迟，更快地提交交易".to_string(),
                ],
                is_retryable: true,
            };
        }

        // 利润不足
        if msg_lower.contains("profit below") || msg_lower.contains("minimum") {
            return ErrorAnalysis {
                possible_causes: vec![
                    "套利利润低于设定的最小阈值".to_string(),
                    "可能是 gas 成本或闪电贷费用侵蚀了利润".to_string(),
                ],
                suggestions: vec![
                    "调整最小利润阈值".to_string(),
                    "选择费率更低的闪电贷池".to_string(),
                ],
                is_retryable: false,
            };
        }

        // Token 不在闪电贷池中
        if msg_lower.contains("not in flash pool") {
            return ErrorAnalysis {
                possible_causes: vec![
                    "选择的闪电贷池不包含起始代币".to_string(),
                ],
                suggestions: vec![
                    "检查闪电贷池选择逻辑".to_string(),
                    "确保使用正确的池子进行闪电贷".to_string(),
                ],
                is_retryable: false,
            };
        }

        // 滑点/输出不足
        if msg_lower.contains("slippage") || msg_lower.contains("too little received")
            || msg_lower.contains("insufficient output amount") {
            return ErrorAnalysis {
                possible_causes: vec![
                    "交易滑点超出预期".to_string(),
                    "流动性池深度不足".to_string(),
                ],
                suggestions: vec![
                    "减少交易金额".to_string(),
                    "增加滑点容忍度".to_string(),
                    "检查池子流动性".to_string(),
                ],
                is_retryable: true,
            };
        }

        // 过期
        if msg_lower.contains("expired") || msg_lower.contains("deadline") {
            return ErrorAnalysis {
                possible_causes: vec![
                    "交易截止时间已过".to_string(),
                ],
                suggestions: vec![
                    "增加截止时间偏移量".to_string(),
                    "优化执行速度".to_string(),
                ],
                is_retryable: true,
            };
        }

        // 流动性不足
        if msg_lower.contains("insufficient liquidity") || msg_lower.contains("not enough") {
            return ErrorAnalysis {
                possible_causes: vec![
                    "池子流动性不足以完成交易".to_string(),
                ],
                suggestions: vec![
                    "减少交易金额".to_string(),
                    "等待流动性恢复".to_string(),
                ],
                is_retryable: true,
            };
        }

        // 默认分析
        ErrorAnalysis {
            possible_causes: vec![
                format!("合约返回错误: {}", message),
            ],
            suggestions: vec![
                "查看合约代码以了解此错误的具体含义".to_string(),
            ],
            is_retryable: false,
        }
    }

    /// 从错误消息中提取 hex 数据
    fn extract_hex_from_error(error: &str) -> Option<Vec<u8>> {
        // 匹配多种格式
        let patterns = [
            // Revert(Bytes(0x...))
            r"Bytes\((0x[0-9a-fA-F]+)\)",
            // revert data: 0x...
            r"revert data[:\s]+(0x[0-9a-fA-F]+)",
            // execution reverted: 0x...
            r"reverted[:\s]+(0x[0-9a-fA-F]+)",
            // 直接的 0x... 格式
            r"(0x[0-9a-fA-F]{8,})",
        ];

        for pattern in &patterns {
            if let Ok(re) = regex::Regex::new(pattern) {
                if let Some(caps) = re.captures(error) {
                    if let Some(hex_match) = caps.get(1) {
                        let hex_str = hex_match.as_str();
                        if hex_str.starts_with("0x") || hex_str.starts_with("0X") {
                            if let Ok(bytes) = hex::decode(&hex_str[2..]) {
                                return Some(bytes);
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// 尝试从 payload 中提取 UTF-8 字符串
    fn try_extract_utf8(data: &[u8]) -> Option<String> {
        // ABI 编码的 string 格式:
        // - 前 32 字节: 偏移量 (通常是 0x20)
        // - 接下来 32 字节: 字符串长度
        // - 后面是实际字符串数据

        if data.len() < 64 {
            return None;
        }

        // 读取偏移量
        let offset = U256::from_big_endian(&data[0..32]).as_usize();
        if offset >= data.len() || offset < 32 {
            return None;
        }

        // 读取长度
        let len_start = offset;
        if len_start + 32 > data.len() {
            return None;
        }
        let length = U256::from_big_endian(&data[len_start..len_start + 32]).as_usize();

        // 读取字符串
        let str_start = len_start + 32;
        if str_start + length > data.len() {
            return None;
        }

        String::from_utf8(data[str_start..str_start + length].to_vec()).ok()
    }

    /// 直接解析错误消息 (当无法提取 hex 数据时)
    fn parse_error_message(error: &str) -> DecodedRevertError {
        // 常见的错误模式
        let error_lower = error.to_lowercase();

        if error_lower.contains("insufficient output for repayment") {
            return DecodedRevertError {
                error_type: RevertErrorType::ErrorString,
                message: "Insufficient output for repayment".to_string(),
                raw_data: error.to_string(),
                analysis: Some(Self::analyze_arbitrage_error("insufficient output for repayment")),
            };
        }

        if error_lower.contains("execution reverted") {
            let analysis = Self::analyze_arbitrage_error(error);
            return DecodedRevertError {
                error_type: RevertErrorType::ErrorString,
                message: error.to_string(),
                raw_data: error.to_string(),
                analysis: Some(analysis),
            };
        }

        DecodedRevertError {
            error_type: RevertErrorType::Unknown,
            message: error.to_string(),
            raw_data: error.to_string(),
            analysis: None,
        }
    }
}

impl std::fmt::Display for DecodedRevertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "═══════════════════════════════════════════════════════════")?;
        writeln!(f, "🔴 合约执行失败 - 错误解析")?;
        writeln!(f, "═══════════════════════════════════════════════════════════")?;
        writeln!(f, "📋 错误类型: {:?}", self.error_type)?;
        writeln!(f, "📝 错误消息: {}", self.message)?;
        writeln!(f, "🔢 原始数据: {}", self.raw_data)?;

        if let Some(ref analysis) = self.analysis {
            writeln!(f, "───────────────────────────────────────────────────────────")?;
            writeln!(f, "🔍 可能的原因:")?;
            for (i, cause) in analysis.possible_causes.iter().enumerate() {
                writeln!(f, "   {}. {}", i + 1, cause)?;
            }
            writeln!(f, "───────────────────────────────────────────────────────────")?;
            writeln!(f, "💡 建议措施:")?;
            for (i, suggestion) in analysis.suggestions.iter().enumerate() {
                writeln!(f, "   {}. {}", i + 1, suggestion)?;
            }
            writeln!(f, "───────────────────────────────────────────────────────────")?;
            writeln!(f, "🔄 是否可重试: {}", if analysis.is_retryable { "是" } else { "否" })?;
        }
        writeln!(f, "═══════════════════════════════════════════════════════════")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_insufficient_output() {
        // 这是你遇到的实际错误数据
        let data = hex::decode(
            "08c379a0\
             0000000000000000000000000000000000000000000000000000000000000020\
             0000000000000000000000000000000000000000000000000000000000000021\
             496e73756666696369656e74206f757470757420666f722072657061796d656e74\
             00000000000000000000000000000000000000000000000000000000000000"
        ).unwrap();

        let decoded = RevertDecoder::decode_revert_data(&data);
        assert_eq!(decoded.error_type, RevertErrorType::ErrorString);
        assert_eq!(decoded.message, "Insufficient output for repayment");
        assert!(decoded.analysis.is_some());
    }

    #[test]
    fn test_decode_from_error_string() {
        let error = r#"ContractError("Revert(Bytes(0x08c379a000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000021496e73756666696369656e74206f757470757420666f722072657061796d656e7400000000000000000000000000000000000000000000000000000000000000))")"#;

        let decoded = RevertDecoder::decode_from_error_string(error);
        assert_eq!(decoded.message, "Insufficient output for repayment");
    }

    #[test]
    fn test_decode_panic() {
        // Panic(0x11) - 算术溢出
        let data = hex::decode(
            "4e487b71\
             0000000000000000000000000000000000000000000000000000000000000011"
        ).unwrap();

        let decoded = RevertDecoder::decode_revert_data(&data);
        assert_eq!(decoded.error_type, RevertErrorType::Panic);
        assert!(decoded.message.contains("0x11"));
        assert!(decoded.message.contains("溢出"));
    }

    #[test]
    fn test_decode_arbitrage_failed_detailed() {
        // ArbitrageFailed_Detailed(string,address,address,address,uint256,uint256,uint256,uint256,uint256,int256)
        // 选择器: 0x384fd583
        // 测试数据:
        //   reason: "Insufficient output for repayment"
        //   tokenA: USDT (0xdAC17F958D2ee523a2206206994597C13D831ec7)
        //   tokenB: WETH (0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2)
        //   tokenC: USDC (0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48)
        //   inputAmount: 2393919900 (2393.9199 USDT)
        //   step1Out: 812421356303377902 (0.8124 WETH)
        //   step2Out: 2390855445 (2390.8554 USDC)
        //   step3Out: 2390523032 (2390.5230 USDT)
        //   amountOwed: 2395116860 (2395.1169 USDT)
        //   profitOrLoss: -4593828 (-4.5938 USDT)
        use ethers::abi::encode;
        use ethers::abi::Token;
        use ethers::types::Address;
        use std::str::FromStr;

        let reason = "Insufficient output for repayment";
        let usdt = Address::from_str("0xdAC17F958D2ee523a2206206994597C13D831ec7").unwrap();
        let weth = Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap();
        let usdc = Address::from_str("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48").unwrap();

        let tokens = vec![
            Token::String(reason.to_string()),
            Token::Address(usdt),                           // tokenA
            Token::Address(weth),                           // tokenB
            Token::Address(usdc),                           // tokenC
            Token::Uint(U256::from(2393919900u64)),         // inputAmount
            Token::Uint(U256::from(812421356303377902u64)), // step1Out
            Token::Uint(U256::from(2390855445u64)),         // step2Out
            Token::Uint(U256::from(2390523032u64)),         // step3Out
            Token::Uint(U256::from(2395116860u64)),         // amountOwed
            Token::Int(I256::from(-4593828i64).into_raw()), // profitOrLoss (负数)
        ];
        let encoded = encode(&tokens);

        // 添加选择器 0x384fd583
        let mut data = vec![0x38, 0x4f, 0xd5, 0x83];
        data.extend(encoded);

        let decoded = RevertDecoder::decode_revert_data(&data);
        assert_eq!(decoded.error_type, RevertErrorType::CustomError);
        assert!(decoded.message.contains("套利失败"));
        assert!(decoded.message.contains("USDT"));
        assert!(decoded.message.contains("WETH"));
        assert!(decoded.message.contains("USDC"));
        assert!(decoded.message.contains("2393.9199"));
        assert!(decoded.message.contains("-4.5938"));
        assert!(decoded.analysis.is_some());
    }

    #[test]
    fn test_decode_profit_below_minimum() {
        // ProfitBelowMinimum(uint256,uint256,uint256,uint256)
        // 选择器: 0xcc9c4404
        use ethers::abi::encode;
        use ethers::abi::Token;

        let tokens = vec![
            Token::Uint(U256::from(50000u64)),      // actualProfit
            Token::Uint(U256::from(100000u64)),     // minRequired
            Token::Uint(U256::from(895333167u64)),  // inputAmount
            Token::Uint(U256::from(895383167u64)),  // outputAmount
        ];
        let encoded = encode(&tokens);

        // 添加选择器
        let mut data = vec![0xcc, 0x9c, 0x44, 0x04];
        data.extend(encoded);

        let decoded = RevertDecoder::decode_revert_data(&data);
        assert_eq!(decoded.error_type, RevertErrorType::CustomError);
        assert!(decoded.message.contains("利润不足"));
        assert!(decoded.message.contains("50000"));
        assert!(decoded.message.contains("100000"));
        assert!(decoded.analysis.is_some());
    }
}
