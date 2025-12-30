// SPDX-License-Identifier: MIT
pragma solidity ^0.8.19;

import "forge-std/Script.sol";
import "../FlashArbitrage.sol";

/// @title 测试套利合约
/// @notice 用于在测试网验证合约基本功能
contract TestArbitrage is Script {
    function run() external {
        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");
        address contractAddress = vm.envAddress("ARBITRAGE_CONTRACT_ADDRESS");

        FlashArbitrage arbitrage = FlashArbitrage(payable(contractAddress));

        vm.startBroadcast(deployerPrivateKey);

        // 1. 检查合约状态
        console.log("=== Contract Status ===");
        console.log("Contract Address:", contractAddress);
        console.log("Owner:", arbitrage.owner());
        console.log("SwapRouter:", address(arbitrage.swapRouter()));
        console.log("MinProfitThreshold:", arbitrage.minProfitThreshold());

        // 2. 设置最小利润阈值 (测试 owner 权限)
        console.log("\n=== Setting MinProfitThreshold ===");
        uint256 newThreshold = 0.001 ether; // 0.001 ETH
        arbitrage.setMinProfitThreshold(newThreshold);
        console.log("New MinProfitThreshold:", arbitrage.minProfitThreshold());

        vm.stopBroadcast();

        console.log("\n=== Test Completed ===");
    }
}

/// @title 模拟套利测试 (仅模拟，不实际执行)
/// @notice 使用 forge script --dry-run 来模拟套利调用
contract SimulateArbitrage is Script {
    function run() external view {
        address contractAddress = vm.envAddress("ARBITRAGE_CONTRACT_ADDRESS");

        FlashArbitrage arbitrage = FlashArbitrage(payable(contractAddress));

        console.log("=== Simulate Arbitrage ===");
        console.log("Contract:", contractAddress);
        console.log("Owner:", arbitrage.owner());

        // Sepolia 测试网地址 (示例)
        // 注意: 这些地址需要根据实际测试网配置调整
        console.log("\nTo execute a real arbitrage, you need:");
        console.log("1. A valid flash pool address");
        console.log("2. Token addresses (A, B, C) that form a profitable loop");
        console.log("3. Correct fee tiers for each swap");
        console.log("4. Sufficient liquidity in all pools");

        console.log("\nExample call structure:");
        console.log("arbitrage.executeArbitrage(ArbitrageParams({");
        console.log("    flashPool: 0x...,");
        console.log("    tokenA: WETH,");
        console.log("    tokenB: USDC,");
        console.log("    tokenC: DAI,");
        console.log("    fee1: 3000,  // 0.3%");
        console.log("    fee2: 500,   // 0.05%");
        console.log("    fee3: 3000,  // 0.3%");
        console.log("    amountIn: 1 ether,");
        console.log("    minProfit: 0.01 ether");
        console.log("}));");
    }
}
