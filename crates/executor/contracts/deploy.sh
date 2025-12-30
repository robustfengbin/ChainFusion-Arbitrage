#!/bin/bash

# FlashArbitrage 合约部署脚本
# 使用方法: ./deploy.sh <network>
# 支持的网络: sepolia, bsc_testnet, mainnet

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 检查 Foundry 是否安装
check_foundry() {
    if ! command -v forge &> /dev/null; then
        echo -e "${YELLOW}Foundry 未安装，正在安装...${NC}"
        curl -L https://foundry.paradigm.xyz | bash
        source ~/.bashrc 2>/dev/null || source ~/.zshrc 2>/dev/null || true
        foundryup
    fi
    echo -e "${GREEN}Foundry 版本: $(forge --version)${NC}"
}

# 安装依赖
install_deps() {
    echo -e "${YELLOW}安装 OpenZeppelin 和 Uniswap 依赖...${NC}"

    # 初始化 (如果需要)
    if [ ! -d "lib" ]; then
        forge init --no-commit --no-git 2>/dev/null || true
    fi

    # 安装依赖
    forge install OpenZeppelin/openzeppelin-contracts@v5.0.0 --no-commit 2>/dev/null || true
    forge install Uniswap/v3-core --no-commit 2>/dev/null || true
    forge install Uniswap/v3-periphery --no-commit 2>/dev/null || true

    # 创建 remappings
    cat > remappings.txt << 'EOF'
@openzeppelin/contracts/=lib/openzeppelin-contracts/contracts/
@uniswap/v3-core/=lib/v3-core/
@uniswap/v3-periphery/=lib/v3-periphery/
forge-std/=lib/forge-std/src/
EOF

    echo -e "${GREEN}依赖安装完成${NC}"
}

# 网络配置
setup_network() {
    local network=$1

    case $network in
        sepolia)
            export RPC_URL="${SEPOLIA_RPC_URL:-https://eth-sepolia.g.alchemy.com/v2/demo}"
            # Uniswap V3 SwapRouter on Sepolia
            export SWAP_ROUTER="0x3bFA4769FB09eefC5a80d6E87c3B9C650f7Ae48E"
            export CHAIN_ID=11155111
            echo -e "${GREEN}网络: Sepolia Testnet${NC}"
            ;;
        bsc_testnet)
            export RPC_URL="${BSC_TESTNET_RPC_URL:-https://data-seed-prebsc-1-s1.binance.org:8545}"
            # PancakeSwap V3 SwapRouter on BSC Testnet
            export SWAP_ROUTER="0x1b81D678ffb9C0263b24A97847620C99d213eB14"
            export CHAIN_ID=97
            echo -e "${GREEN}网络: BSC Testnet${NC}"
            ;;
        mainnet)
            export RPC_URL="${ETH_RPC_URL}"
            # Uniswap V3 SwapRouter on Mainnet
            export SWAP_ROUTER="0xE592427A0AEce92De3Edee1F18E0157C05861564"
            export CHAIN_ID=1
            echo -e "${RED}警告: 正在部署到主网!${NC}"
            read -p "确认继续? (y/N): " confirm
            if [[ $confirm != "y" && $confirm != "Y" ]]; then
                echo "已取消"
                exit 1
            fi
            ;;
        *)
            echo -e "${RED}不支持的网络: $network${NC}"
            echo "支持的网络: sepolia, bsc_testnet, mainnet"
            exit 1
            ;;
    esac
}

# 部署合约
deploy() {
    local network=$1

    echo -e "${YELLOW}开始部署到 $network ...${NC}"

    # 检查私钥
    if [ -z "$PRIVATE_KEY" ]; then
        echo -e "${RED}错误: 请设置 PRIVATE_KEY 环境变量${NC}"
        echo "export PRIVATE_KEY=your_private_key_here"
        exit 1
    fi

    # 编译
    echo -e "${YELLOW}编译合约...${NC}"
    forge build

    # 部署
    echo -e "${YELLOW}部署合约...${NC}"
    forge script deploy/Deploy.s.sol:DeployFlashArbitrage \
        --rpc-url "$RPC_URL" \
        --broadcast \
        --verify \
        -vvvv

    echo -e "${GREEN}部署完成!${NC}"
    echo -e "${YELLOW}请保存合约地址并更新 .env 文件中的 ARBITRAGE_CONTRACT_ADDRESS${NC}"
}

# 仅编译 (不部署)
build_only() {
    echo -e "${YELLOW}编译合约...${NC}"
    forge build
    echo -e "${GREEN}编译完成!${NC}"
}

# 主函数
main() {
    local cmd=${1:-help}
    local network=${2:-sepolia}

    case $cmd in
        deploy)
            check_foundry
            install_deps
            setup_network "$network"
            deploy "$network"
            ;;
        build)
            check_foundry
            install_deps
            build_only
            ;;
        install)
            check_foundry
            install_deps
            ;;
        help|*)
            echo "FlashArbitrage 合约部署工具"
            echo ""
            echo "用法: $0 <command> [network]"
            echo ""
            echo "命令:"
            echo "  deploy <network>  部署合约到指定网络"
            echo "  build             仅编译合约"
            echo "  install           安装依赖"
            echo "  help              显示此帮助"
            echo ""
            echo "网络:"
            echo "  sepolia      Ethereum Sepolia 测试网 (默认)"
            echo "  bsc_testnet  BSC 测试网"
            echo "  mainnet      Ethereum 主网 (谨慎使用)"
            echo ""
            echo "环境变量:"
            echo "  PRIVATE_KEY          部署者私钥 (必需)"
            echo "  SEPOLIA_RPC_URL      Sepolia RPC URL"
            echo "  BSC_TESTNET_RPC_URL  BSC 测试网 RPC URL"
            echo "  ETH_RPC_URL          主网 RPC URL"
            echo ""
            echo "示例:"
            echo "  export PRIVATE_KEY=0x..."
            echo "  ./deploy.sh deploy sepolia"
            ;;
    esac
}

main "$@"
