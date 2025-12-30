// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

// Uniswap V3 接口定义 (避免版本冲突)
interface IUniswapV3Pool {
    function token0() external view returns (address);
    function token1() external view returns (address);
    function flash(
        address recipient,
        uint256 amount0,
        uint256 amount1,
        bytes calldata data
    ) external;
}

interface IUniswapV3FlashCallback {
    function uniswapV3FlashCallback(
        uint256 fee0,
        uint256 fee1,
        bytes calldata data
    ) external;
}

interface ISwapRouter {
    struct ExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint24 fee;
        address recipient;
        uint256 deadline;
        uint256 amountIn;
        uint256 amountOutMinimum;
        uint160 sqrtPriceLimitX96;
    }

    function exactInputSingle(ExactInputSingleParams calldata params)
        external
        payable
        returns (uint256 amountOut);
}

/// @title FlashArbitrage - 闪电贷三角套利合约
/// @notice 使用 Uniswap V3 闪电贷执行三角套利
/// @dev 支持 A -> B -> C -> A 的套利路径
contract FlashArbitrage is IUniswapV3FlashCallback, Ownable, ReentrancyGuard {
    using SafeERC20 for IERC20;

    /// @notice Uniswap V3 SwapRouter 地址
    ISwapRouter public immutable SWAP_ROUTER;

    /// @notice 最小利润阈值 (wei)
    uint256 public minProfitThreshold;

    /// @notice 套利执行事件
    event ArbitrageExecuted(
        address indexed tokenA,
        address indexed tokenB,
        address indexed tokenC,
        uint256 amountIn,
        uint256 amountOut,
        uint256 profit
    );

    /// @notice 套利失败事件
    event ArbitrageFailed(
        address indexed tokenA,
        uint256 amountIn,
        string reason
    );

    /// @notice 利润提取事件
    event ProfitWithdrawn(
        address indexed token,
        address indexed to,
        uint256 amount
    );

    /// @notice 利润转换事件
    event ProfitConverted(
        address indexed fromToken,
        address indexed toToken,
        uint256 amountIn,
        uint256 amountOut
    );

    /// @notice 每步交换执行事件 (用于追踪每一步的输入输出)
    event SwapStepExecuted(
        uint8 indexed step,        // 步骤: 1, 2, 3
        address tokenIn,           // 输入代币
        address tokenOut,          // 输出代币
        uint256 amountIn,          // 输入数量
        uint256 amountOut          // 输出数量
    );

    /// @notice 套利详细结果事件 (包含每步数据和盈亏分析)
    event ArbitrageResult(
        uint256 inputAmount,       // 初始输入
        uint256 step1Out,          // 第一步输出
        uint256 step2Out,          // 第二步输出
        uint256 step3Out,          // 第三步输出 (最终输出)
        uint256 flashFee,          // 闪电贷手续费
        int256 profitOrLoss        // 盈亏 (负数表示亏损)
    );

    /// @notice 套利失败详细错误 (携带每步执行数据，方便分析)
    error ArbitrageFailed_Detailed(
        string reason,           // 失败原因
        address tokenA,          // 起始代币 (借入)
        address tokenB,          // 中间代币1
        address tokenC,          // 中间代币2
        uint256 inputAmount,     // 初始输入 (tokenA)
        uint256 step1Out,        // 第一步输出 (A->B, tokenB)
        uint256 step2Out,        // 第二步输出 (B->C, tokenC)
        uint256 step3Out,        // 第三步输出 (C->A, tokenA)
        uint256 amountOwed,      // 需要归还的金额 (tokenA)
        int256 profitOrLoss      // 盈亏 (tokenA)
    );

    /// @notice 利润不足错误
    error ProfitBelowMinimum(
        uint256 actualProfit,    // 实际利润
        uint256 minRequired,     // 最低要求
        uint256 inputAmount,     // 输入金额
        uint256 outputAmount     // 输出金额
    );

    /// @notice 闪电贷回调数据结构
    struct FlashCallbackData {
        address tokenA;      // 起始代币 (借入并最终归还)
        address tokenB;      // 中间代币 1
        address tokenC;      // 中间代币 2
        uint24 fee1;         // A -> B 池子费率
        uint24 fee2;         // B -> C 池子费率
        uint24 fee3;         // C -> A 池子费率
        uint256 amount0;     // 借入的 token0 数量
        uint256 amount1;     // 借入的 token1 数量
        address flashPool;   // 闪电贷来源池
    }

    /// @notice 套利参数结构
    struct ArbitrageParams {
        address flashPool;   // 用于闪电贷的池子
        address tokenA;      // 起始代币
        address tokenB;      // 中间代币 1
        address tokenC;      // 中间代币 2
        uint24 fee1;         // A -> B 费率
        uint24 fee2;         // B -> C 费率
        uint24 fee3;         // C -> A 费率
        uint256 amountIn;    // 输入金额
        uint256 minProfit;   // 最小利润要求
        address profitToken; // 利润结算代币 (address(0) 表示不转换，保留原始代币)
        uint24 profitConvertFee; // 利润转换池费率 (tokenA -> profitToken)
    }

    constructor(address _swapRouter) Ownable(msg.sender) {
        require(_swapRouter != address(0), "Invalid router");
        SWAP_ROUTER = ISwapRouter(_swapRouter);
        minProfitThreshold = 0; // 默认无最小利润限制，由调用者指定
    }

    /// @notice 执行三角套利
    /// @param params 套利参数
    /// @return profit 套利利润
    function executeArbitrage(ArbitrageParams calldata params)
        external
        onlyOwner
        nonReentrant
        returns (uint256 profit)
    {
        require(params.amountIn > 0, "Amount must be > 0");
        require(params.flashPool != address(0), "Invalid flash pool");

        IUniswapV3Pool pool = IUniswapV3Pool(params.flashPool);
        address token0 = pool.token0();
        address token1 = pool.token1();

        // 确定借入哪个代币
        uint256 amount0;
        uint256 amount1;

        if (params.tokenA == token0) {
            amount0 = params.amountIn;
            amount1 = 0;
        } else if (params.tokenA == token1) {
            amount0 = 0;
            amount1 = params.amountIn;
        } else {
            revert("Token A not in flash pool");
        }

        // 记录套利前余额
        uint256 balanceBefore = IERC20(params.tokenA).balanceOf(address(this));

        // 编码回调数据
        bytes memory data = abi.encode(FlashCallbackData({
            tokenA: params.tokenA,
            tokenB: params.tokenB,
            tokenC: params.tokenC,
            fee1: params.fee1,
            fee2: params.fee2,
            fee3: params.fee3,
            amount0: amount0,
            amount1: amount1,
            flashPool: params.flashPool
        }));

        // 发起闪电贷
        pool.flash(address(this), amount0, amount1, data);

        // 计算利润
        uint256 balanceAfter = IERC20(params.tokenA).balanceOf(address(this));

        // 检查是否亏损
        if (balanceAfter < balanceBefore) {
            revert ProfitBelowMinimum(
                0,                    // 实际利润为 0 (亏损)
                params.minProfit,     // 最低要求
                params.amountIn,      // 输入金额
                balanceAfter          // 输出金额
            );
        }

        profit = balanceAfter - balanceBefore;

        // 检查利润是否达到最低要求
        if (profit < params.minProfit) {
            revert ProfitBelowMinimum(
                profit,               // 实际利润
                params.minProfit,     // 最低要求
                params.amountIn,      // 输入金额
                balanceAfter          // 输出金额
            );
        }

        // 检查全局利润阈值
        if (profit < minProfitThreshold) {
            revert ProfitBelowMinimum(
                profit,               // 实际利润
                minProfitThreshold,   // 全局阈值
                params.amountIn,      // 输入金额
                balanceAfter          // 输出金额
            );
        }

        emit ArbitrageExecuted(
            params.tokenA,
            params.tokenB,
            params.tokenC,
            params.amountIn,
            balanceAfter,
            profit
        );

        // 如果指定了利润结算代币且与起始代币不同，则转换利润
        if (params.profitToken != address(0) && params.profitToken != params.tokenA && profit > 0) {
            uint256 convertedProfit = _convertProfit(
                params.tokenA,
                params.profitToken,
                params.profitConvertFee,
                profit
            );

            emit ProfitConverted(params.tokenA, params.profitToken, profit, convertedProfit);

            // 返回转换后的利润金额
            return convertedProfit;
        }

        return profit;
    }

    /// @notice Uniswap V3 闪电贷回调
    /// @dev 在这里执行实际的套利交换
    function uniswapV3FlashCallback(
        uint256 fee0,
        uint256 fee1,
        bytes calldata data
    ) external override {
        FlashCallbackData memory decoded = abi.decode(data, (FlashCallbackData));

        // 验证回调来源
        require(msg.sender == decoded.flashPool, "Invalid callback sender");

        // 计算需要归还的金额 (借入金额 + 手续费)
        uint256 amountBorrowed = decoded.amount0 > 0 ? decoded.amount0 : decoded.amount1;
        uint256 fee = decoded.amount0 > 0 ? fee0 : fee1;
        uint256 amountOwed = amountBorrowed + fee;

        // 执行三角套利: A -> B -> C -> A (返回每步结果)
        (uint256 amountOut, uint256 step1Out, uint256 step2Out) = _executeTriangularSwapWithDetails(
            decoded.tokenA,
            decoded.tokenB,
            decoded.tokenC,
            decoded.fee1,
            decoded.fee2,
            decoded.fee3,
            amountBorrowed
        );

        // 计算盈亏 (可能为负)
        // casting to 'int256' is safe because amountOut and amountOwed are token amounts
        // which are always far below int256.max (~2^255)
        // forge-lint: disable-next-line(unsafe-typecast)
        int256 profitOrLoss = int256(amountOut) - int256(amountOwed);

        // 发出详细结果事件 (即使失败也会在 error 中携带这些信息)
        emit ArbitrageResult(
            amountBorrowed,
            step1Out,
            step2Out,
            amountOut,
            fee,
            profitOrLoss
        );

        // 确保有足够的代币归还闪电贷 (使用自定义 error 携带详细信息)
        if (amountOut < amountOwed) {
            revert ArbitrageFailed_Detailed(
                "Insufficient output for repayment",
                decoded.tokenA,
                decoded.tokenB,
                decoded.tokenC,
                amountBorrowed,
                step1Out,
                step2Out,
                amountOut,
                amountOwed,
                profitOrLoss
            );
        }

        // 归还闪电贷
        IERC20(decoded.tokenA).safeTransfer(msg.sender, amountOwed);
    }

    /// @notice 执行三角交换并返回每步详情 A -> B -> C -> A
    function _executeTriangularSwapWithDetails(
        address tokenA,
        address tokenB,
        address tokenC,
        uint24 fee1,
        uint24 fee2,
        uint24 fee3,
        uint256 amountIn
    ) internal returns (uint256 amountOut, uint256 step1Out, uint256 step2Out) {
        // 授权 SwapRouter
        IERC20(tokenA).forceApprove(address(SWAP_ROUTER), amountIn);

        // Step 1: A -> B
        step1Out = SWAP_ROUTER.exactInputSingle(
            ISwapRouter.ExactInputSingleParams({
                tokenIn: tokenA,
                tokenOut: tokenB,
                fee: fee1,
                recipient: address(this),
                deadline: block.timestamp,
                amountIn: amountIn,
                amountOutMinimum: 0,
                sqrtPriceLimitX96: 0
            })
        );
        emit SwapStepExecuted(1, tokenA, tokenB, amountIn, step1Out);

        // Step 2: B -> C
        IERC20(tokenB).forceApprove(address(SWAP_ROUTER), step1Out);
        step2Out = SWAP_ROUTER.exactInputSingle(
            ISwapRouter.ExactInputSingleParams({
                tokenIn: tokenB,
                tokenOut: tokenC,
                fee: fee2,
                recipient: address(this),
                deadline: block.timestamp,
                amountIn: step1Out,
                amountOutMinimum: 0,
                sqrtPriceLimitX96: 0
            })
        );
        emit SwapStepExecuted(2, tokenB, tokenC, step1Out, step2Out);

        // Step 3: C -> A
        IERC20(tokenC).forceApprove(address(SWAP_ROUTER), step2Out);
        amountOut = SWAP_ROUTER.exactInputSingle(
            ISwapRouter.ExactInputSingleParams({
                tokenIn: tokenC,
                tokenOut: tokenA,
                fee: fee3,
                recipient: address(this),
                deadline: block.timestamp,
                amountIn: step2Out,
                amountOutMinimum: 0,
                sqrtPriceLimitX96: 0
            })
        );
        emit SwapStepExecuted(3, tokenC, tokenA, step2Out, amountOut);

        return (amountOut, step1Out, step2Out);
    }

    /// @notice 将利润从一种代币转换为另一种代币
    /// @param fromToken 源代币
    /// @param toToken 目标代币
    /// @param fee 交换池费率
    /// @param amount 转换金额
    /// @return amountOut 转换后的金额
    function _convertProfit(
        address fromToken,
        address toToken,
        uint24 fee,
        uint256 amount
    ) internal returns (uint256 amountOut) {
        IERC20(fromToken).forceApprove(address(SWAP_ROUTER), amount);

        amountOut = SWAP_ROUTER.exactInputSingle(
            ISwapRouter.ExactInputSingleParams({
                tokenIn: fromToken,
                tokenOut: toToken,
                fee: fee,
                recipient: address(this),
                deadline: block.timestamp,
                amountIn: amount,
                amountOutMinimum: 0, // 利润转换不设滑点保护，因为已经是赚到的利润
                sqrtPriceLimitX96: 0
            })
        );

        return amountOut;
    }

    /// @notice 提取合约中的利润
    /// @param token 代币地址
    /// @param to 接收地址
    /// @param amount 提取金额
    function withdrawProfit(address token, address to, uint256 amount)
        external
        onlyOwner
    {
        require(to != address(0), "Invalid recipient");
        uint256 balance = IERC20(token).balanceOf(address(this));
        require(balance >= amount, "Insufficient balance");

        IERC20(token).safeTransfer(to, amount);

        emit ProfitWithdrawn(token, to, amount);
    }

    /// @notice 提取所有利润
    /// @param token 代币地址
    /// @param to 接收地址
    function withdrawAllProfit(address token, address to) external onlyOwner {
        require(to != address(0), "Invalid recipient");
        uint256 balance = IERC20(token).balanceOf(address(this));
        require(balance > 0, "No balance to withdraw");

        IERC20(token).safeTransfer(to, balance);

        emit ProfitWithdrawn(token, to, balance);
    }

    /// @notice 设置最小利润阈值
    /// @param threshold 新的阈值
    function setMinProfitThreshold(uint256 threshold) external onlyOwner {
        minProfitThreshold = threshold;
    }

    /// @notice 紧急提取 (用于恢复误转入的代币)
    /// @param token 代币地址
    function emergencyWithdraw(address token) external onlyOwner {
        uint256 balance = IERC20(token).balanceOf(address(this));
        if (balance > 0) {
            IERC20(token).safeTransfer(owner(), balance);
        }
    }

    /// @notice 紧急提取 ETH
    function emergencyWithdrawEth() external onlyOwner {
        uint256 balance = address(this).balance;
        if (balance > 0) {
            payable(owner()).transfer(balance);
        }
    }

    /// @notice 接收 ETH
    receive() external payable {}
}
