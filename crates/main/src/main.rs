mod bootstrap;

use anyhow::Result;
use tracing::info;
use utils::LoggerManager;

use crate::bootstrap::{setup_panic_hook, Application};

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志系统
    let _logger = LoggerManager::init();

    // 设置 panic hook
    setup_panic_hook();

    info!("========================================");
    info!("  DEX 套利机器人系统启动");
    info!("========================================");

    // 启动应用
    let app = Application::start().await?;

    // 运行 API 服务器（阻塞）
    app.run_server().await;

    // 关闭应用
    app.shutdown().await?;

    Ok(())
}
