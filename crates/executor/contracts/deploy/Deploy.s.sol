// SPDX-License-Identifier: MIT
pragma solidity ^0.8.19;

import "forge-std/Script.sol";
import "../FlashArbitrage.sol";

contract DeployFlashArbitrage is Script {
    // Uniswap V3 SwapRouter 地址
    // Sepolia: 0x3bFA4769FB09eefC5a80d6E87c3B9C650f7Ae48E (可能需要确认)
    // Ethereum Mainnet: 0xE592427A0AEce92De3Edee1F18E0157C05861564
    // BSC (PancakeSwap V3): 0x1b81D678ffb9C0263b24A97847620C99d213eB14

    function run() external {
        // 从环境变量读取配置
        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");
        address swapRouter = vm.envAddress("SWAP_ROUTER");

        vm.startBroadcast(deployerPrivateKey);

        // 部署合约
        FlashArbitrage arbitrage = new FlashArbitrage(swapRouter);

        console.log("FlashArbitrage deployed at:", address(arbitrage));
        console.log("Owner:", arbitrage.owner());
        console.log("SwapRouter:", address(arbitrage.swapRouter()));

        vm.stopBroadcast();
    }
}
