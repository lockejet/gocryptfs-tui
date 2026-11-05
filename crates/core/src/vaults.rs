// vaults.rs — 卷列表与历史记录的数据模型及加载（TUI / Web UI 共用）

use std::path::Path;

use serde::Deserialize;

use crate::{backend, paths};

fn default_valid() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct Vault {
    pub id: u32,
    pub name: String,
    pub path: String,
    pub mount_point: String,
    pub mounted: bool,
    pub locked: bool,
    #[serde(default = "default_valid")]
    pub valid: bool,
}

#[derive(Debug, Deserialize)]
pub struct CliListResponse {
    pub status: String,
    pub data: CliListData,
}

#[derive(Debug, Deserialize)]
pub struct CliListData {
    pub vaults: Vec<Vault>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HistoryEntry {
    pub ts: String,
    #[serde(default)]
    pub src: String,
    #[serde(default)]
    pub action: String,
    #[serde(default, alias = "name")]
    pub target: String,
    #[serde(default, alias = "status")]
    pub result: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

pub fn load_vaults(config: &str) -> Result<Vec<Vault>, String> {
    if config.is_empty() {
        return Err(crate::t!("cli.error.config_empty").to_string());
    }
    let out = backend::cli_command(&paths::cli_path())
        .args(["-c", config, "list", "--json"])
        .output()
        .map_err(|e| crate::t!("cli.error.exec_failed", e))?;
    if !out.status.success() {
        return Err(crate::t!(
            "cli.error.cli_failed",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let resp: CliListResponse =
        serde_json::from_slice(&out.stdout).map_err(|e| crate::t!("cli.error.json_parse", e))?;
    if resp.status != "ok" {
        return Err(crate::t!("cli.error.bad_status").to_string());
    }
    Ok(resp.data.vaults)
}

pub fn load_history(log_file: &Path) -> Vec<HistoryEntry> {
    let content = match std::fs::read_to_string(log_file) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<HistoryEntry>(line).ok())
        .collect()
}
