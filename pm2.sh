#!/usr/bin/env bash
set -euo pipefail

APP_NAME="chainfusion_arbitrage"

echo "[1/4] 编译 Rust 项目 (debug 模式)..."
cargo build

echo "[2/4] 停止已有 pm2 实例..."
pm2 delete "$APP_NAME" || true

echo "[3/4] 输入钱包私钥..."
# 检查是否已通过环境变量传入
if [ -z "${PRIVATE_KEY:-}" ]; then
    # 交互输入（隐藏输入内容）
    read -s -p "请输入钱包私钥 (0x开头): " PRIVATE_KEY
    echo ""  # 换行

    # 验证格式
    if [[ ! "$PRIVATE_KEY" =~ ^0x[a-fA-F0-9]{64}$ ]]; then
        echo "❌ 私钥格式错误！应为 0x 开头的 66 位十六进制字符串"
        exit 1
    fi
    echo "✅ 私钥格式验证通过"
else
    echo "✅ 使用环境变量中的私钥"
fi

echo "[4/4] 启动 pm2 实例..."
# 通过环境变量传递私钥给 pm2 进程
PRIVATE_KEY="$PRIVATE_KEY" pm2 start target/debug/chainfusion_arbitrage \
  --name "$APP_NAME" \
  --cwd "$(pwd)"

# 清除 shell 中的私钥变量
unset PRIVATE_KEY

echo ""
echo "✅ 启动完成！"
echo "   私钥已安全传递给进程，不会保存到磁盘"
echo ""
echo "查看日志: pm2 logs $APP_NAME"
echo "查看状态: pm2 status"
