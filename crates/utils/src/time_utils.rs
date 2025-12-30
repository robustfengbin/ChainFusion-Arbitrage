//! 时间工具模块
//!
//! 提供统一的时间格式化和时区转换功能

use chrono::{DateTime, Local, Utc};
use chrono_tz::Asia::Shanghai;

/// 上海时区 (UTC+8)
pub const SHANGHAI_OFFSET_SECONDS: i32 = 8 * 3600;

/// 获取当前上海时间
pub fn now_shanghai() -> DateTime<chrono_tz::Tz> {
    Utc::now().with_timezone(&Shanghai)
}

/// 获取当前上海时间的格式化字符串 (默认格式: YYYY-MM-DD HH:MM:SS)
pub fn now_shanghai_str() -> String {
    now_shanghai().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 将 UTC 时间转换为上海时间
pub fn utc_to_shanghai(utc_time: DateTime<Utc>) -> DateTime<chrono_tz::Tz> {
    utc_time.with_timezone(&Shanghai)
}

/// 将 UTC 时间转换为上海时间的格式化字符串
pub fn utc_to_shanghai_str(utc_time: DateTime<Utc>) -> String {
    utc_to_shanghai(utc_time).format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 将 UTC 时间转换为上海时间，使用自定义格式
pub fn utc_to_shanghai_format(utc_time: DateTime<Utc>, format: &str) -> String {
    utc_to_shanghai(utc_time).format(format).to_string()
}

/// 获取当前本地时间（系统时区）
pub fn now_local() -> DateTime<Local> {
    Local::now()
}

/// 获取当前本地时间的格式化字符串
pub fn now_local_str() -> String {
    now_local().format("%Y-%m-%d %H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_now_shanghai() {
        let shanghai_time = now_shanghai();
        println!("上海时间: {}", shanghai_time);
        assert!(shanghai_time.to_string().contains("+08:00"));
    }

    #[test]
    fn test_utc_to_shanghai() {
        let utc_time = Utc::now();
        let shanghai_time = utc_to_shanghai(utc_time);

        // 上海时间比 UTC 快 8 小时
        let utc_hour = utc_time.hour();
        let shanghai_hour = shanghai_time.hour();
        let expected_hour = (utc_hour + 8) % 24;
        assert_eq!(shanghai_hour, expected_hour);
    }

    #[test]
    fn test_format() {
        let utc_time = Utc::now();
        let formatted = utc_to_shanghai_format(utc_time, "%Y年%m月%d日 %H:%M:%S");
        println!("自定义格式: {}", formatted);
        assert!(formatted.contains("年"));
    }
}
