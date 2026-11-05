#![allow(dead_code)]

pub mod backend;
pub mod config;
pub mod deps;
#[macro_use]
pub mod i18n;
pub mod logger;
pub mod paths;
pub mod protocol;
pub mod vaults;

pub const PROJECT_NAME: &str = "gocryptfs-ui";
/// 历史命名：配置/数据/状态目录与显示名都用它
pub const APP_NAME: &str = "gocryptfs-tui";
pub const CLI_NAME: &str = "gocryptfs-cli";
pub const APP_VERSION: &str = env!("APP_VERSION");
pub const APP_COMMIT: &str = env!("APP_COMMIT");
pub const APP_BUILD_TIME: &str = env!("APP_BUILD_TIME");

pub fn version_string() -> String {
    format!("{} ({}, {})", APP_VERSION, APP_COMMIT, APP_BUILD_TIME)
}

/// 版本串（写进内嵌后端的 VERSION 文件，供 `gocryptfs-cli --version` 使用）
pub fn version_line() -> String {
    version_string()
}

pub fn short_version() -> String {
    APP_VERSION.to_string()
}
