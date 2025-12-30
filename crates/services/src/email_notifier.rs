//! 套利邮件通知模块
//!
//! 在执行合约套利时发送邮件通知，包括:
//! - 套利前钱包余额
//! - 套利完成后钱包余额
//! - 套利执行详情

use anyhow::Result;
use chrono::Utc;
use chrono_tz::Asia::Shanghai;
use lettre::message::{header::ContentType, Mailbox};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

/// 邮件通知配置
#[derive(Clone, Debug)]
pub struct EmailConfig {
    pub enabled: bool,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: String,
    pub smtp_password: String,
    pub from_email: String,
    pub from_name: String,
    pub to_emails: Vec<String>,
    pub use_tls: bool,
}

impl EmailConfig {
    /// 从环境变量创建配置
    pub fn from_env() -> Self {
        let enabled = std::env::var("EMAIL_ENABLED")
            .unwrap_or_else(|_| "false".to_string())
            .parse::<bool>()
            .unwrap_or(false);

        let smtp_host = std::env::var("EMAIL_SMTP_HOST")
            .unwrap_or_else(|_| "smtp.qq.com".to_string());

        let smtp_port = std::env::var("EMAIL_SMTP_PORT")
            .unwrap_or_else(|_| "587".to_string())
            .parse()
            .unwrap_or(587);

        let smtp_username = std::env::var("EMAIL_SMTP_USERNAME")
            .unwrap_or_else(|_| "".to_string());

        let smtp_password = std::env::var("EMAIL_SMTP_PASSWORD")
            .unwrap_or_else(|_| "".to_string());

        let from_email = std::env::var("EMAIL_FROM_ADDRESS")
            .unwrap_or_else(|_| smtp_username.clone());

        let from_name = std::env::var("EMAIL_FROM_NAME")
            .unwrap_or_else(|_| "ChainFusion Arbitrage System".to_string());

        let to_emails_str = std::env::var("EMAIL_TO_ADDRESSES")
            .unwrap_or_else(|e| {
                warn!("Failed to read EMAIL_TO_ADDRESSES: {}", e);
                "".to_string()
            });

        let to_emails: Vec<String> = if to_emails_str.is_empty() {
            warn!("EMAIL_TO_ADDRESSES is empty");
            Vec::new()
        } else {
            let emails: Vec<String> = to_emails_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            info!("Parsed email recipients: {:?}", emails);
            emails
        };

        let use_tls = std::env::var("EMAIL_USE_TLS")
            .unwrap_or_else(|_| "true".to_string())
            .parse::<bool>()
            .unwrap_or(true);

        Self {
            enabled,
            smtp_host,
            smtp_port,
            smtp_username,
            smtp_password,
            from_email,
            from_name,
            to_emails,
            use_tls,
        }
    }
}

/// 钱包余额信息
#[derive(Clone, Debug)]
pub struct WalletBalance {
    /// 代币符号
    pub symbol: String,
    /// 代币地址
    pub token_address: String,
    /// 余额数量
    pub balance: String,
    /// USD 价值
    pub usd_value: Decimal,
}

/// 套利执行信息
#[derive(Clone, Debug)]
pub struct ArbitrageExecutionInfo {
    /// 链名称
    pub chain_name: String,
    /// 机会 ID
    pub opportunity_id: String,
    /// 套利路径描述
    pub path_description: String,
    /// 输入代币符号
    pub input_token: String,
    /// 输入金额
    pub input_amount: String,
    /// 预期利润 (USD)
    pub expected_profit_usd: Decimal,
    /// 实际利润 (USD)
    pub actual_profit_usd: Option<Decimal>,
    /// Gas 费用 (USD)
    pub gas_cost_usd: Decimal,
    /// 交易哈希
    pub tx_hash: Option<String>,
    /// 执行状态
    pub status: String,
    /// 区块号
    pub block_number: u64,
    /// 错误信息 (如果失败)
    pub error_message: Option<String>,
}

/// 邮件通知器
pub struct EmailNotifier {
    config: EmailConfig,
    mailer: Arc<RwLock<Option<AsyncSmtpTransport<Tokio1Executor>>>>,
}

impl EmailNotifier {
    /// 创建新的邮件通知器
    pub fn new(config: EmailConfig) -> Self {
        let mailer = if config.enabled && !config.smtp_username.is_empty() {
            match Self::create_mailer(&config) {
                Ok(m) => Some(m),
                Err(e) => {
                    error!("Failed to create email sender: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Self {
            config,
            mailer: Arc::new(RwLock::new(mailer)),
        }
    }

    /// 创建 SMTP 邮件发送器
    fn create_mailer(config: &EmailConfig) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
        let creds = Credentials::new(
            config.smtp_username.clone(),
            config.smtp_password.clone(),
        );

        let mailer = if config.use_tls {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.smtp_host)?
                .port(config.smtp_port)
                .credentials(creds)
                .build()
        } else {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp_host)?
                .port(config.smtp_port)
                .credentials(creds)
                .build()
        };

        Ok(mailer)
    }

    /// 发送邮件给指定收件人
    async fn send_email_to(
        &self,
        mailer: &AsyncSmtpTransport<Tokio1Executor>,
        to_email: &str,
        subject: &str,
        body: &str,
    ) -> Result<()> {
        let from_mailbox: Mailbox = format!("{} <{}>", self.config.from_name, self.config.from_email)
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid sender address: {}", e))?;

        let to_mailbox: Mailbox = to_email.parse()
            .map_err(|e| anyhow::anyhow!("Invalid recipient address: {}", e))?;

        let email = Message::builder()
            .from(from_mailbox)
            .to(to_mailbox)
            .subject(subject)
            .header(ContentType::TEXT_HTML)
            .body(body.to_string())?;

        mailer.send(email).await?;

        Ok(())
    }

    /// 发送通用通知邮件
    pub async fn send_notification(
        &self,
        subject: &str,
        body: &str,
    ) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        if self.config.to_emails.is_empty() {
            warn!("Email notification is enabled but no recipients configured");
            return Ok(());
        }

        let mailer_guard = self.mailer.read().await;
        let mailer = match mailer_guard.as_ref() {
            Some(m) => m,
            None => {
                warn!("Email sender not initialized");
                return Ok(());
            }
        };

        // 异步发送给所有收件人
        for to_email in &self.config.to_emails {
            let mailer_clone = mailer.clone();
            let to_email_clone = to_email.clone();
            let subject_clone = subject.to_string();
            let body_clone = body.to_string();
            let self_clone = self.clone();

            tokio::spawn(async move {
                match self_clone.send_email_to(&mailer_clone, &to_email_clone, &subject_clone, &body_clone).await {
                    Ok(_) => info!("Notification email sent to: {}", to_email_clone),
                    Err(e) => error!("Failed to send email ({}): {}", to_email_clone, e),
                }
            });
        }

        Ok(())
    }

    /// 发送 HTML 格式通知
    pub async fn send_html_notification(
        &self,
        title: &str,
        content_html: &str,
    ) -> Result<()> {
        let shanghai_time = Utc::now().with_timezone(&Shanghai);
        let time_str = shanghai_time.format("%Y-%m-%d %H:%M:%S CST").to_string();

        let body = format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <style>
        body {{ font-family: 'Segoe UI', Arial, sans-serif; background: #f5f5f5; margin: 0; padding: 20px; }}
        .container {{ max-width: 700px; margin: 0 auto; background: white; border-radius: 12px; overflow: hidden; box-shadow: 0 4px 6px rgba(0,0,0,0.1); }}
        .header {{ background: linear-gradient(135deg, #667eea 0%, #764ba2 100%); color: white; padding: 30px; text-align: center; }}
        .header h2 {{ margin: 0 0 10px 0; font-size: 24px; }}
        .content {{ padding: 30px; }}
        .footer {{ text-align: center; color: #999; font-size: 12px; padding: 20px; border-top: 1px solid #e0e0e0; }}
        .info-box {{ background: #f8f9fa; padding: 15px; border-radius: 8px; margin: 15px 0; border-left: 4px solid #667eea; }}
        .success-box {{ background: #d4edda; padding: 15px; border-radius: 8px; margin: 15px 0; border-left: 4px solid #28a745; }}
        .error-box {{ background: #f8d7da; padding: 15px; border-radius: 8px; margin: 15px 0; border-left: 4px solid #dc3545; }}
        .balance-table {{ width: 100%; border-collapse: collapse; margin: 15px 0; }}
        .balance-table th, .balance-table td {{ padding: 10px; text-align: left; border-bottom: 1px solid #e0e0e0; }}
        .balance-table th {{ background: #f8f9fa; }}
        .profit {{ color: #28a745; font-weight: bold; }}
        .loss {{ color: #dc3545; font-weight: bold; }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h2>{}</h2>
            <p>{}</p>
        </div>
        <div class="content">
            {}
        </div>
        <div class="footer">
            <p>ChainFusion Arbitrage System - Automated Trading</p>
            <p>This email is sent automatically, please do not reply</p>
        </div>
    </div>
</body>
</html>"#,
            title,
            time_str,
            content_html,
        );

        self.send_notification(title, &body).await
    }

    /// 发送套利执行通知
    /// 包括套利前后的钱包余额和执行详情
    pub async fn send_arbitrage_notification(
        &self,
        execution_info: &ArbitrageExecutionInfo,
        balances_before: &[WalletBalance],
        balances_after: &[WalletBalance],
    ) -> Result<()> {
        let is_success = execution_info.status == "Confirmed" || execution_info.status == "Success";
        let status_emoji = if is_success { "✅" } else { "❌" };
        let status_class = if is_success { "success-box" } else { "error-box" };

        // 构建余额前后对比表格
        let mut balance_rows = String::new();
        for before in balances_before {
            let after = balances_after.iter()
                .find(|b| b.symbol == before.symbol)
                .cloned()
                .unwrap_or(before.clone());

            let change = after.usd_value - before.usd_value;
            let change_class = if change >= Decimal::ZERO { "profit" } else { "loss" };
            let change_sign = if change >= Decimal::ZERO { "+" } else { "" };

            balance_rows.push_str(&format!(
                r#"<tr>
                    <td>{}</td>
                    <td>{}</td>
                    <td>{}</td>
                    <td class="{}">{}{:.4}</td>
                </tr>"#,
                before.symbol,
                before.balance,
                after.balance,
                change_class,
                change_sign,
                change
            ));
        }

        // 计算总 USD 变化
        let total_before: Decimal = balances_before.iter().map(|b| b.usd_value).sum();
        let total_after: Decimal = balances_after.iter().map(|b| b.usd_value).sum();
        let total_change = total_after - total_before;
        let total_change_class = if total_change >= Decimal::ZERO { "profit" } else { "loss" };
        let total_change_sign = if total_change >= Decimal::ZERO { "+" } else { "" };

        let actual_profit_html = if let Some(profit) = execution_info.actual_profit_usd {
            format!("<p><strong>实际利润:</strong> <span class=\"profit\">${:.4}</span></p>", profit)
        } else {
            String::new()
        };

        let error_html = if let Some(ref err) = execution_info.error_message {
            format!(
                r#"<div class="error-box">
                    <strong>错误信息:</strong><br>
                    <code>{}</code>
                </div>"#,
                err
            )
        } else {
            String::new()
        };

        let tx_hash_html = if let Some(ref hash) = execution_info.tx_hash {
            format!("<p><strong>交易哈希:</strong> <code>{}</code></p>", hash)
        } else {
            String::new()
        };

        let content = format!(
            r#"
            <div class="{}">
                <h3>{} 套利执行{}</h3>
            </div>

            <div class="info-box">
                <h4>执行信息</h4>
                <p><strong>链:</strong> {}</p>
                <p><strong>机会 ID:</strong> {}</p>
                <p><strong>区块:</strong> {}</p>
                <p><strong>路径:</strong> {}</p>
                <p><strong>输入:</strong> {} {}</p>
                <p><strong>预期利润:</strong> ${:.4}</p>
                {}
                <p><strong>Gas 费用:</strong> ${:.4}</p>
                {}
                {}
            </div>

            <h4>钱包余额变化</h4>
            <table class="balance-table">
                <thead>
                    <tr>
                        <th>代币</th>
                        <th>套利前</th>
                        <th>套利后</th>
                        <th>变化 (USD)</th>
                    </tr>
                </thead>
                <tbody>
                    {}
                    <tr style="font-weight: bold; background: #f8f9fa;">
                        <td colspan="3">总计 (USD)</td>
                        <td class="{}">{}{:.4}</td>
                    </tr>
                </tbody>
            </table>
            "#,
            status_class,
            status_emoji,
            if is_success { "成功" } else { "失败" },
            execution_info.chain_name,
            execution_info.opportunity_id,
            execution_info.block_number,
            execution_info.path_description,
            execution_info.input_amount,
            execution_info.input_token,
            execution_info.expected_profit_usd,
            actual_profit_html,
            execution_info.gas_cost_usd,
            tx_hash_html,
            error_html,
            balance_rows,
            total_change_class,
            total_change_sign,
            total_change
        );

        let title = format!(
            "{} {} 套利 - {}",
            status_emoji,
            execution_info.chain_name,
            if is_success { "成功" } else { "失败" }
        );

        self.send_html_notification(&title, &content).await
    }

    /// 发送错误通知
    pub async fn send_error_notification(
        &self,
        error_title: &str,
        error_message: &str,
        details: Option<&str>,
    ) -> Result<()> {
        let details_html = if let Some(d) = details {
            format!(
                r#"<div style="background: #f8f9fa; padding: 15px; border-radius: 8px; margin: 20px 0;">
                    <strong>Error Details:</strong><br>
                    <code style="display: block; margin-top: 10px; padding: 10px; background: #fff; border-radius: 4px; word-break: break-word;">
                        {}
                    </code>
                </div>"#,
                d
            )
        } else {
            String::new()
        };

        let content = format!(
            r#"<div style="background: #fff3cd; padding: 20px; margin: 20px 0; border-radius: 8px; border-left: 4px solid #ffc107;">
                <div style="color: #dc3545; font-weight: bold; font-size: 16px; margin-bottom: 10px;">
                    {}
                </div>
            </div>
            {}"#,
            error_message,
            details_html
        );

        self.send_html_notification(&format!("⚠️ {}", error_title), &content).await
    }

    /// 发送成功通知
    pub async fn send_success_notification(
        &self,
        title: &str,
        message: &str,
    ) -> Result<()> {
        let content = format!(
            r#"<div style="background: #d4edda; padding: 20px; margin: 20px 0; border-radius: 8px; border-left: 4px solid #28a745;">
                <div style="color: #28a745; font-weight: bold; font-size: 16px;">
                    {}
                </div>
            </div>"#,
            message
        );

        self.send_html_notification(&format!("✅ {}", title), &content).await
    }
}

impl Clone for EmailNotifier {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            mailer: Arc::clone(&self.mailer),
        }
    }
}

use std::sync::OnceLock;

/// 全局邮件通知器实例
static EMAIL_NOTIFIER: OnceLock<Option<Arc<EmailNotifier>>> = OnceLock::new();

/// 获取全局邮件通知器
pub fn get_email_notifier() -> Option<Arc<EmailNotifier>> {
    EMAIL_NOTIFIER.get_or_init(|| {
        let config = EmailConfig::from_env();
        if config.enabled {
            info!("Initializing email notifier: SMTP={}:{}", config.smtp_host, config.smtp_port);
            Some(Arc::new(EmailNotifier::new(config)))
        } else {
            info!("Email notification not enabled");
            None
        }
    }).clone()
}
