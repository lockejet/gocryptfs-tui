#![allow(dead_code)]

mod cli;
mod logger;

use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use logger::{LogEntry, LogKind, LogLevel, Logger};

const APP_NAME: &str = env!("CARGO_PKG_NAME");
const CLI_NAME: &str = "gocryptfs-cli";
const MAX_OUTPUT: usize = 500;
const MAX_HISTORY_DISPLAY: usize = 40;

const APP_VERSION: &str = env!("APP_VERSION");
const APP_COMMIT: &str = env!("APP_COMMIT");
const APP_BUILD_TIME: &str = env!("APP_BUILD_TIME");

fn version_string() -> String {
    format!("{} ({}, {})", APP_VERSION, APP_COMMIT, APP_BUILD_TIME)
}

fn short_version() -> String {
    APP_VERSION.to_string()
}

// ============================================================
// 配置文件读值
// ============================================================

fn read_setting(config: &str, key: &str, default: &str) -> String {
    if let Ok(content) = std::fs::read_to_string(config) {
        for line in content.lines() {
            let t = line.trim_start();
            if let Some(rest) = t.strip_prefix(&format!("{}:", key)) {
                let v = rest.trim().trim_matches('"').trim_matches('\'');
                if !v.is_empty() {
                    return v.to_string();
                }
            }
        }
    }
    default.to_string()
}

// ============================================================
// 数据模型
// ============================================================

fn default_valid() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
struct Vault {
    id: u32,
    name: String,
    path: String,
    mount_point: String,
    mounted: bool,
    locked: bool,
    #[serde(default = "default_valid")]
    valid: bool,
}

#[derive(Debug, Deserialize)]
struct CliListResponse {
    status: String,
    data: CliListData,
}

#[derive(Debug, Deserialize)]
struct CliListData {
    vaults: Vec<Vault>,
}

#[derive(Debug, Clone, Deserialize)]
struct HistoryEntry {
    ts: String,
    #[serde(default)]
    src: String,
    #[serde(default)]
    action: String,
    #[serde(default, alias = "name")]
    target: String,
    #[serde(default, alias = "status")]
    result: String,
    #[serde(default)]
    detail: String,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Default)]
struct HistoryFilter {
    src: Option<String>,
    result: Option<String>,
    action: Option<String>,
}

impl HistoryFilter {
    fn apply(&self, entries: &[HistoryEntry]) -> Vec<HistoryEntry> {
        entries
            .iter()
            .filter(|e| self.src.as_ref().is_none_or(|s| &e.src == s))
            .filter(|e| self.result.as_ref().is_none_or(|r| &e.result == r))
            .filter(|e| self.action.as_ref().is_none_or(|a| &e.action == a))
            .cloned()
            .collect()
    }

    fn cycle_src(&mut self) {
        self.src = match self.src.as_deref() {
            None => Some("cli".to_string()),
            Some("cli") => Some("tui".to_string()),
            _ => None,
        };
    }

    fn cycle_result(&mut self) {
        self.result = match self.result.as_deref() {
            None => Some("success".to_string()),
            Some("success") => Some("failed".to_string()),
            Some("failed") => Some("started".to_string()),
            Some("started") => Some("cancelled".to_string()),
            _ => None,
        };
    }

    fn cycle_action(&mut self) {
        self.action = match self.action.as_deref() {
            None => Some("mount".to_string()),
            Some("mount") => Some("umount".to_string()),
            Some("umount") => Some("create".to_string()),
            Some("create") => Some("remove".to_string()),
            _ => None,
        };
    }
}

// ============================================================
// CLI 交互
// ============================================================

fn cli_path() -> String {
    std::env::var("GOCRYPTFS_CLI").unwrap_or_else(|_| CLI_NAME.to_string())
}

fn get_startup_command() -> String {
    std::env::args().collect::<Vec<_>>().join(" ")
}

fn resolve_config(cli_opt: Option<PathBuf>) -> String {
    if let Some(p) = cli_opt {
        return p.to_string_lossy().to_string();
    }
    if let Ok(o) = Command::new(cli_path()).arg("config").output() {
        if o.status.success() {
            let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !p.is_empty() {
                return p;
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join(APP_NAME)
            .join("config.yaml")
            .to_string_lossy()
            .to_string();
    }
    if let Some(cfg) = dirs::config_dir() {
        return cfg
            .join(APP_NAME)
            .join("config.yaml")
            .to_string_lossy()
            .to_string();
    }
    String::new()
}

fn resolve_data_dir(cli_opt: Option<PathBuf>) -> PathBuf {
    if let Some(p) = cli_opt {
        return p;
    }
    if let Some(d) = dirs::data_local_dir() {
        return d.join(APP_NAME);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local/share").join(APP_NAME);
    }
    PathBuf::from("/tmp").join(APP_NAME)
}

fn load_vaults(config: &str) -> Result<Vec<Vault>, String> {
    if config.is_empty() {
        return Err("配置路径为空".to_string());
    }
    let out = Command::new(cli_path())
        .args(["-c", config, "list", "--json"])
        .output()
        .map_err(|e| format!("执行 CLI 失败: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "CLI 错误: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let resp: CliListResponse =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("JSON 解析失败: {}", e))?;
    if resp.status != "ok" {
        return Err("CLI 返回非 ok".to_string());
    }
    Ok(resp.data.vaults)
}

// 加载并（必要时）从旧 history.jsonl 迁移
fn load_history(log_file: &Path) -> Vec<HistoryEntry> {
    let content = match std::fs::read_to_string(log_file) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<HistoryEntry>(line).ok())
        .collect()
}

// 启动时把旧 history.jsonl 迁移到 app.log.jsonl，然后删除旧文件
fn migrate_old_history(data_dir: &Path) {
    let old = data_dir.join("history.jsonl");
    let new = data_dir.join("app.log.jsonl");

    if !old.exists() {
        return;
    }

    let old_content = match std::fs::read_to_string(&old) {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut new_lines = String::new();
    for line in old_content.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            let ts = v.get("ts").and_then(|x| x.as_str()).unwrap_or("");
            let action = v.get("action").and_then(|x| x.as_str()).unwrap_or("");
            let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("");
            let status = v.get("status").and_then(|x| x.as_str()).unwrap_or("");
            let detail = v.get("detail").and_then(|x| x.as_str()).unwrap_or("");

            let new_entry = serde_json::json!({
                "ts": ts,
                "src": "cli",
                "action": action,
                "target": name,
                "result": status,
                "detail": detail,
                "pid": null,
                "duration_ms": null,
            });

            if let Ok(s) = serde_json::to_string(&new_entry) {
                new_lines.push_str(&s);
                new_lines.push('\n');
            }
        }
    }

    use std::io::Write as _;
    if new.exists() {
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&new) {
            let _ = f.write_all(new_lines.as_bytes());
        }
    } else if std::fs::write(&new, &new_lines).is_err() {
        return;
    }

    let _ = std::fs::remove_file(&old);
}

#[derive(Debug)]
enum CliEvent {
    Pid(u32),
    Stdout(String),
    Stderr(String),
    Progress { pct: u8, done: u64, total: u64 },
    Check { key: String, value: String },
    Done(i32),
}

fn parse_protocol_line(line: &str) -> Option<CliEvent> {
    if let Some(rest) = line.strip_prefix("@@PROGRESS@@ ") {
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 3 {
            let pct = parts[0].parse::<u8>().ok()?;
            let done = parts[1].parse::<u64>().unwrap_or(0);
            let total = parts[2].parse::<u64>().unwrap_or(0);
            return Some(CliEvent::Progress { pct, done, total });
        }
    } else if let Some(rest) = line.strip_prefix("@@CHECK@@ ") {
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        if parts.len() == 2 {
            return Some(CliEvent::Check {
                key: parts[0].to_string(),
                value: parts[1].to_string(),
            });
        }
    }
    None
}

fn spawn_cli_task(
    args: Vec<String>,
    password: Option<String>,
    log_file: &Path,
    history_file: &Path,
) -> Receiver<CliEvent> {
    let (tx, rx) = mpsc::channel();
    let cli = cli_path();
    let log = log_file.to_path_buf();
    let hist = history_file.to_path_buf();
    thread::spawn(move || {
        let mut cmd = Command::new(&cli);
        cmd.args(&args)
            .env("LOG_FILE", &log)
            .env("HISTORY_FILE", &hist)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(CliEvent::Stderr(format!("spawn 失败: {}", e)));
                let _ = tx.send(CliEvent::Done(1));
                return;
            }
        };
        let _ = tx.send(CliEvent::Pid(child.id()));

        if let Some(pw) = password {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = writeln!(stdin, "{}", pw);
            }
        } else {
            drop(child.stdin.take());
        }
        let stderr = child.stderr.take().unwrap();
        let tx_err = tx.clone();
        let h_err = thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Some(evt) = parse_protocol_line(&line) {
                    if tx_err.send(evt).is_err() {
                        break;
                    }
                } else if !line.starts_with("@@") && tx_err.send(CliEvent::Stderr(line)).is_err() {
                    break;
                }
            }
        });
        let stdout = child.stdout.take().unwrap();
        let tx_out = tx.clone();
        let h_out = thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(evt) = parse_protocol_line(&line) {
                    if tx_out.send(evt).is_err() {
                        break;
                    }
                } else if !line.starts_with("@@") && tx_out.send(CliEvent::Stdout(line)).is_err() {
                    break;
                }
            }
        });
        let code = child.wait().map(|s| s.code().unwrap_or(1)).unwrap_or(1);
        let _ = h_err.join();
        let _ = h_out.join();
        let _ = tx.send(CliEvent::Done(code));
    });
    rx
}

fn sys_run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("执行 {} 失败: {}", cmd, e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("退出码 {}", out.status.code().unwrap_or(1))
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn kill_pid_tree(pid: u32) {
    let _ = Command::new("pkill")
        .arg("-TERM")
        .arg("-P")
        .arg(pid.to_string())
        .spawn();
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .spawn();
}

// ============================================================
// 状态定义
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq)]
enum Page {
    Mount,
    Create,
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Focus {
    List,
    Detail,
    Dir,
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SettingsTab {
    Gocryptfs,
    Rsync,
    Filters,
    Perm,
}

#[derive(Debug, Clone)]
enum Overlay {
    Help,
    Settings {
        tab: SettingsTab,
        scope_index: usize,
    },
    History,
    Message(String),
    ConfirmUmount {
        vault_index: usize,
    },
}

#[derive(Debug, Clone, Default)]
struct ScrollState {
    v: u16,
    h: u16,
}

struct PasswordInput {
    vault_name: String,
    buffer: String,
    error: Option<String>,
}

struct BackgroundTask {
    description: String,
    action: String,
    status: TaskStatus,
    pid: Option<u32>,
    rx: Receiver<CliEvent>,
    started_at: Instant,
}

#[derive(Debug, Clone, PartialEq)]
enum TaskStatus {
    Running,
    Done,
    Failed(i32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum WizardDirection {
    Create,
    Remove,
}

#[derive(Debug, Clone, PartialEq)]
enum WizardStep {
    ConfigPaths,
    EnterPassword,
    Running,
    ConfirmDelete,
    Done,
    Failed(String),
}

struct Wizard {
    direction: WizardDirection,
    step: WizardStep,
    source: String,
    name: String,
    cipher: String,
    tmp_mount: String,
    vault_name: String,
    dry_run: bool,
    keep_source: bool,
    keep_source_locked: bool,
    restore: bool,
    restore_locked: bool,
    delete_cipher: bool,
    delete_cipher_locked: bool,
    password: String,
    password_confirm: String,
    confirming: bool,
    delete_confirm_text: String,
    progress: Option<(u8, u64, u64)>,
    checks: HashMap<String, String>,
    error: Option<String>,
    selected: usize,
}

impl Wizard {
    fn option_count(&self) -> usize {
        match self.direction {
            WizardDirection::Create => 2,
            WizardDirection::Remove => 3,
        }
    }
}

#[derive(Clone)]
struct DirSection {
    title: String,
    lines: Vec<String>,
}

#[derive(Clone, Default)]
struct DirView {
    sections: Vec<DirSection>,
    scroll: u16,
    mode: String,
}

struct App {
    page: Page,
    focus: Focus,
    overlay: Option<Overlay>,
    wizard: Option<Wizard>,

    vaults: Vec<Vault>,
    vault_list_state: ListState,
    vault_confirmed: Option<usize>,
    pending: Vec<String>,
    pending_list_state: ListState,
    pending_confirmed: Option<usize>,

    dir_view: DirView,
    detail_scroll: ScrollState,
    dir_scroll: ScrollState,
    output_scroll: ScrollState,
    output: Vec<String>,

    status: String,
    config: String,
    data_dir: PathBuf,
    log_file: PathBuf,
    history_file: PathBuf,
    help_file: PathBuf,
    logger: Logger,
    startup_command: String,
    password_input: Option<PasswordInput>,
    task: Option<BackgroundTask>,
    should_quit: bool,
    editor_request: bool,
    scope_names: Vec<String>,
    history_entries: Vec<HistoryEntry>,
    history_selected: usize,
    history_filter: HistoryFilter,

    last_ctrl_d: Option<Instant>,
    ctrl_d_count: u8,
}

impl App {
    fn new(
        config: String,
        data_dir: PathBuf,
        log_file: PathBuf,
        history_file: PathBuf,
        help_file: PathBuf,
    ) -> Self {
        let startup_command = get_startup_command();

        let level_str = read_setting(&config, "logging.level", "operation");
        let max_size: u64 = read_setting(&config, "logging.max_size", "5242880")
            .parse()
            .unwrap_or(5 * 1024 * 1024);
        let max_files: usize = read_setting(&config, "logging.max_files", "3")
            .parse()
            .unwrap_or(3);
        let logger = Logger::new(
            log_file.clone(),
            LogLevel::parse(&level_str),
            max_size,
            max_files,
        );

        let mut app = App {
            page: Page::Mount,
            focus: Focus::List,
            overlay: None,
            wizard: None,
            vaults: Vec::new(),
            vault_list_state: ListState::default(),
            vault_confirmed: None,
            pending: Vec::new(),
            pending_list_state: ListState::default(),
            pending_confirmed: None,
            dir_view: DirView::default(),
            detail_scroll: ScrollState::default(),
            dir_scroll: ScrollState::default(),
            output_scroll: ScrollState::default(),
            output: Vec::new(),
            status: "就绪".to_string(),
            config: config.clone(),
            data_dir,
            log_file,
            history_file,
            help_file,
            logger,
            startup_command,
            password_input: None,
            task: None,
            should_quit: false,
            editor_request: false,
            scope_names: vec!["全局".to_string()],
            history_entries: Vec::new(),
            history_selected: 0,
            history_filter: HistoryFilter::default(),
            last_ctrl_d: None,
            ctrl_d_count: 0,
        };
        app.add_output(format!("{} {} 启动", APP_NAME, short_version()));
        app.add_output(format!("CLI:  {}", cli_path()));
        app.add_output(format!(
            "配置: {}",
            if app.config.is_empty() {
                "(未找到)"
            } else {
                &app.config
            }
        ));

        if !app.config.is_empty() {
            match load_vaults(&config) {
                Ok(vaults) => {
                    let n = vaults.len();
                    app.vaults = vaults;
                    app.add_output(format!("加载 {} 个卷", n));
                    if n > 0 {
                        app.vault_list_state.select(Some(0));
                    }
                }
                Err(e) => app.add_output(format!("加载失败: {}", e)),
            }
        }
        app.reload_pending();
        app.update_scope_names();

        app.logger.log(
            LogKind::Operation,
            LogEntry::new("tui.start", APP_NAME).detail(short_version()),
        );

        app
    }

    fn add_output(&mut self, line: String) {
        let ts = chrono::Local::now().format("%H:%M:%S").to_string();
        self.output.push(format!("[{}] {}", ts, line));
        if self.output.len() > MAX_OUTPUT {
            let excess = self.output.len() - MAX_OUTPUT;
            self.output.drain(0..excess);
        }
        let h = self.output.len().saturating_sub(7) as u16;
        self.output_scroll.v = h;
    }

    fn update_scope_names(&mut self) {
        let mut names = vec!["全局".to_string()];
        for v in &self.vaults {
            names.push(v.name.clone());
        }
        self.scope_names = names;
    }

    fn reload_vaults(&mut self) {
        if self.config.is_empty() {
            return;
        }
        match load_vaults(&self.config) {
            Ok(vaults) => {
                self.vaults = vaults;
                if !self.vaults.is_empty() {
                    let cur = self.vault_list_state.selected().unwrap_or(0);
                    let idx = cur.min(self.vaults.len() - 1);
                    self.vault_list_state.select(Some(idx));
                } else {
                    self.vault_list_state.select(None);
                }
                self.update_scope_names();
            }
            Err(e) => self.add_output(format!("刷新失败: {}", e)),
        }
    }

    fn reload_pending(&mut self) {
        if self.config.is_empty() {
            return;
        }
        if let Ok(content) = std::fs::read_to_string(&self.config) {
            let mut in_pending = false;
            let mut dirs = Vec::new();
            for line in content.lines() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("pending:") {
                    in_pending = true;
                    continue;
                }
                if in_pending {
                    if trimmed.starts_with("- source_dir:") {
                        let dir = trimmed
                            .trim_start_matches("- source_dir:")
                            .trim()
                            .trim_matches('"')
                            .trim_matches('\'')
                            .to_string();
                        if !dir.is_empty() {
                            dirs.push(dir);
                        }
                    } else if !trimmed.is_empty()
                        && !trimmed.starts_with('#')
                        && !line.starts_with(' ')
                        && !line.starts_with('\t')
                    {
                        in_pending = false;
                    }
                }
            }
            self.pending = dirs;
            if !self.pending.is_empty() {
                let cur = self.pending_list_state.selected().unwrap_or(0);
                let idx = cur.min(self.pending.len() - 1);
                self.pending_list_state.select(Some(idx));
            } else {
                self.pending_list_state.select(None);
            }
        }
    }

    fn selected_vault(&self) -> Option<&Vault> {
        self.vault_list_state
            .selected()
            .and_then(|i| self.vaults.get(i))
    }

    fn pending_created_vault(&self, source: &str) -> Option<&Vault> {
        self.vaults.iter().find(|v| v.mount_point == source)
    }

    fn filtered_history(&self) -> Vec<HistoryEntry> {
        self.history_filter.apply(&self.history_entries)
    }

    fn next_vault(&mut self) {
        if self.vaults.is_empty() {
            return;
        }
        let i = match self.vault_list_state.selected() {
            Some(i) => (i + 1) % self.vaults.len(),
            None => 0,
        };
        self.vault_list_state.select(Some(i));
        self.vault_confirmed = None;
        self.dir_view = DirView::default();
        self.dir_scroll = ScrollState::default();
        self.detail_scroll = ScrollState::default();
    }

    fn previous_vault(&mut self) {
        if self.vaults.is_empty() {
            return;
        }
        let i = match self.vault_list_state.selected() {
            Some(0) | None => self.vaults.len() - 1,
            Some(i) => i - 1,
        };
        self.vault_list_state.select(Some(i));
        self.vault_confirmed = None;
        self.dir_view = DirView::default();
        self.dir_scroll = ScrollState::default();
        self.detail_scroll = ScrollState::default();
    }

    fn next_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let i = match self.pending_list_state.selected() {
            Some(i) => (i + 1) % self.pending.len(),
            None => 0,
        };
        self.pending_list_state.select(Some(i));
        self.pending_confirmed = None;
        self.dir_view = DirView::default();
        self.dir_scroll = ScrollState::default();
        self.detail_scroll = ScrollState::default();
    }

    fn previous_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let i = match self.pending_list_state.selected() {
            Some(0) | None => self.pending.len() - 1,
            Some(i) => i - 1,
        };
        self.pending_list_state.select(Some(i));
        self.pending_confirmed = None;
        self.dir_view = DirView::default();
        self.dir_scroll = ScrollState::default();
        self.detail_scroll = ScrollState::default();
    }

    fn start_mount(&mut self) {
        let name = match self.selected_vault() {
            Some(v) => v.name.clone(),
            None => {
                self.status = "无卷可选".to_string();
                return;
            }
        };
        if let Some(v) = self.selected_vault() {
            if v.mounted {
                self.status = format!("{} 已挂载", name);
                return;
            }
        }
        self.password_input = Some(PasswordInput {
            vault_name: name,
            buffer: String::new(),
            error: None,
        });
        self.status = "输入密码后按 Enter".to_string();
    }

    fn trigger_umount_or_confirm(&mut self) {
        let idx = match self.vault_list_state.selected() {
            Some(i) => i,
            None => {
                self.status = "无卷可选".to_string();
                return;
            }
        };
        let vault = match self.vaults.get(idx) {
            Some(v) => v.clone(),
            None => return,
        };
        if !vault.valid {
            self.overlay = Some(Overlay::Message(format!(
                "「{}」不是有效的 gocryptfs 加密卷，无法卸载。\n\n加密目录: {}",
                vault.name, vault.path
            )));
            return;
        }
        if !vault.mounted {
            self.status = "卷未挂载".to_string();
            return;
        }
        self.overlay = Some(Overlay::ConfirmUmount { vault_index: idx });
    }

    fn enter_action(&mut self) {
        let idx = match self.vault_list_state.selected() {
            Some(i) => i,
            None => {
                self.status = "无卷可选".to_string();
                return;
            }
        };
        if self.vault_confirmed != Some(idx) {
            self.status = "请先按 Space 选中".to_string();
            return;
        }
        let vault = match self.vaults.get(idx) {
            Some(v) => v.clone(),
            None => return,
        };
        if !vault.valid {
            self.overlay = Some(Overlay::Message(format!(
                "「{}」不是有效的 gocryptfs 加密卷。\n\n加密目录: {}\n缺少 gocryptfs.conf。\n\n请在 [2] 创建加密页处理该条目。",
                vault.name, vault.path
            )));
            return;
        }
        if vault.mounted {
            self.overlay = Some(Overlay::ConfirmUmount { vault_index: idx });
        } else {
            self.start_mount();
        }
    }

    fn enter_create_wizard(&mut self) {
        let idx = match self.pending_list_state.selected() {
            Some(i) => i,
            None => {
                self.status = "无待处理目录".to_string();
                return;
            }
        };
        if self.pending_confirmed != Some(idx) {
            self.status = "请先按 Space 选中".to_string();
            return;
        }
        let source = self.pending[idx].clone();
        if let Some(v) = self.pending_created_vault(&source) {
            let name = v.name.clone();
            self.overlay = Some(Overlay::Message(format!(
                "源目录已被卷「{}」占用，无法重复创建。\n\n请先在 [3] 删除加密页删除该卷，或从配置中移除该 pending 条目。",
                name
            )));
            return;
        }
        self.start_create_wizard();
    }

    fn enter_remove_wizard(&mut self) {
        let idx = match self.vault_list_state.selected() {
            Some(i) => i,
            None => {
                self.status = "无卷可选".to_string();
                return;
            }
        };
        if self.vault_confirmed != Some(idx) {
            self.status = "请先按 Space 选中".to_string();
            return;
        }
        self.start_remove_wizard();
    }

    fn start_umount_confirmed(&mut self, idx: usize) {
        let name = match self.vaults.get(idx) {
            Some(v) => v.name.clone(),
            None => return,
        };
        self.vault_confirmed = None;
        let log = self.log_file.clone();
        let hist = self.history_file.clone();
        let args = vec![
            "-c".into(),
            self.config.clone(),
            "umount".into(),
            name.clone(),
        ];
        self.start_task("umount", format!("卸载 {}", name), args, None, log, hist);
    }

    fn load_dir_view(&mut self, use_tree: bool) {
        let mode = if use_tree { "tree" } else { "ls" };
        let cmd = if use_tree { "tree" } else { "ls" };
        let extra_args: Vec<&str> = if use_tree {
            vec!["-L", "2"]
        } else {
            vec!["-la"]
        };

        match self.page {
            Page::Mount | Page::Remove => {
                let vault = match self.selected_vault() {
                    Some(v) => v.clone(),
                    None => {
                        self.status = "无卷可选".to_string();
                        return;
                    }
                };
                let mut sections = Vec::new();
                sections.push(self.make_section(
                    &format!("【加密路径】{}", vault.path),
                    &vault.path,
                    cmd,
                    &extra_args,
                    false,
                ));
                sections.push(self.make_section(
                    &format!("【挂载点】{}", vault.mount_point),
                    &vault.mount_point,
                    cmd,
                    &extra_args,
                    !vault.mounted,
                ));
                self.dir_view = DirView {
                    sections,
                    scroll: 0,
                    mode: mode.to_string(),
                };
                self.dir_scroll = ScrollState::default();
                self.focus = Focus::Dir;
                self.status = format!("已加载 {} 的目录 ({})", vault.name, mode);
            }
            Page::Create => {
                let idx = match self.pending_list_state.selected() {
                    Some(i) => i,
                    None => {
                        self.status = "无待处理目录".to_string();
                        return;
                    }
                };
                let source = self.pending[idx].clone();
                let name = Path::new(&source)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "vault".to_string());
                let cipher = format!(
                    "{}/.cipher.d/{}",
                    Path::new(&source)
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| ".".to_string()),
                    name
                );
                let tmp_mount = format!("{}.mount_tmp", source);

                let mut sections = Vec::new();
                sections.push(self.make_section(
                    &format!("【加密目录】{}", cipher),
                    &cipher,
                    cmd,
                    &extra_args,
                    false,
                ));
                sections.push(self.make_section(
                    &format!("【临时挂载点】{}", tmp_mount),
                    &tmp_mount,
                    cmd,
                    &extra_args,
                    false,
                ));
                sections.push(self.make_section(
                    &format!("【挂载点（源目录）】{}", source),
                    &source,
                    cmd,
                    &extra_args,
                    false,
                ));
                self.dir_view = DirView {
                    sections,
                    scroll: 0,
                    mode: mode.to_string(),
                };
                self.dir_scroll = ScrollState::default();
                self.focus = Focus::Dir;
                self.status = format!("已加载 {} 的目录 ({})", name, mode);
            }
        }
    }

    fn make_section(
        &self,
        title: &str,
        path: &str,
        cmd: &str,
        extra_args: &[&str],
        placeholder_missing: bool,
    ) -> DirSection {
        let mut lines = Vec::new();
        let path_obj = Path::new(path);

        if !path_obj.exists() {
            lines.push("（路径不存在）".to_string());
        } else if placeholder_missing {
            lines.push("（未挂载）".to_string());
        } else {
            let mut args: Vec<&str> = extra_args.to_vec();
            args.push(path);
            match sys_run(cmd, &args) {
                Ok(text) => {
                    for l in text.lines() {
                        lines.push(l.to_string());
                    }
                    if lines.is_empty() {
                        lines.push("（空）".to_string());
                    }
                }
                Err(e) => {
                    lines.push(format!("（读取失败: {}）", e));
                }
            }
        }
        DirSection {
            title: title.to_string(),
            lines,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn start_task(
        &mut self,
        action: &str,
        desc: String,
        args: Vec<String>,
        password: Option<String>,
        log_file: PathBuf,
        history_file: PathBuf,
    ) {
        if self.task.is_some() {
            let finished = matches!(
                self.task.as_ref().map(|t| &t.status),
                Some(TaskStatus::Done) | Some(TaskStatus::Failed(_))
            );
            if finished {
                self.task = None;
            } else {
                self.status = "已有任务运行中".to_string();
                return;
            }
        }
        self.add_output(format!("执行: {} {}", cli_path(), args.join(" ")));
        self.status = format!("运行中: {}", desc);

        self.logger.log(
            LogKind::Operation,
            LogEntry::new(action, &desc)
                .result("started")
                .detail(args.join(" ")),
        );

        let rx = spawn_cli_task(args, password, &log_file, &history_file);
        self.task = Some(BackgroundTask {
            description: desc,
            action: action.to_string(),
            status: TaskStatus::Running,
            pid: None,
            rx,
            started_at: Instant::now(),
        });
    }

    fn poll_task(&mut self) {
        let mut events = Vec::new();
        let mut done_code: Option<i32> = None;
        if let Some(task) = &self.task {
            while let Ok(event) = task.rx.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            match event {
                CliEvent::Pid(pid) => {
                    if let Some(task) = &mut self.task {
                        task.pid = Some(pid);
                    }
                }
                CliEvent::Stdout(line) | CliEvent::Stderr(line) => {
                    if !line.is_empty() {
                        self.add_output(line);
                    }
                }
                CliEvent::Progress { pct, done, total } => {
                    if let Some(w) = &mut self.wizard {
                        w.progress = Some((pct, done, total));
                    }
                }
                CliEvent::Check { key, value } => {
                    if let Some(w) = &mut self.wizard {
                        w.checks.insert(key, value);
                    }
                }
                CliEvent::Done(code) => done_code = Some(code),
            }
        }
        if let Some(code) = done_code {
            let (desc, action, pid, duration_ms) = match &self.task {
                Some(t) => (
                    t.description.clone(),
                    t.action.clone(),
                    t.pid,
                    t.started_at.elapsed().as_millis() as u64,
                ),
                None => (String::new(), String::new(), None, 0),
            };

            if let Some(task) = &mut self.task {
                task.status = if code == 0 {
                    TaskStatus::Done
                } else {
                    TaskStatus::Failed(code)
                };
            }

            if code == 0 {
                self.status = format!("完成: {}", desc);
                self.add_output(format!("[OK] {}", desc));
                if let Some(w) = &mut self.wizard {
                    w.step = WizardStep::Done;
                }
            } else {
                self.status = format!("失败 ({}): {}", code, desc);
                self.add_output(format!("[FAIL:{}] {}", code, desc));
                if let Some(w) = &mut self.wizard {
                    w.step = WizardStep::Failed(format!("CLI 退出码: {}", code));
                }
                if code == 2 {
                    self.overlay = Some(Overlay::Message("密码错误".to_string()));
                }
            }

            let mut entry = LogEntry::new(action, desc).duration_ms(duration_ms);
            entry = if code == 0 {
                entry.result("success")
            } else {
                entry.result("failed")
            };
            if let Some(p) = pid {
                entry = entry.pid(p);
            }
            self.logger.log(LogKind::Operation, entry);

            self.reload_vaults();
            self.reload_pending();
            self.task = None;
        }
    }

    fn interrupt_task(&mut self) {
        let info = self.task.as_ref().and_then(|t| {
            t.pid
                .map(|pid| (pid, t.description.clone(), t.action.clone()))
        });

        if let Some((pid, desc, action)) = info {
            kill_pid_tree(pid);
            std::thread::sleep(Duration::from_millis(300));
            self.add_output(format!("已中断任务: {} (PID {})", desc, pid));
            self.status = "已中断".to_string();
            self.logger.log(
                LogKind::Operation,
                LogEntry::new(action, desc).result("cancelled").pid(pid),
            );
        } else if self.task.is_some() {
            self.status = "任务尚未启动 PID".to_string();
        }

        self.task = None;
        if let Some(w) = &mut self.wizard {
            w.step = WizardStep::Failed("用户中断".to_string());
        }
    }

    fn start_create_wizard(&mut self) {
        if self.pending.is_empty() {
            self.status = "待处理目录为空（按 e 编辑配置的 pending 段）".to_string();
            return;
        }
        let idx = self.pending_list_state.selected().unwrap_or(0);
        if idx >= self.pending.len() {
            self.pending_list_state.select(Some(0));
            return self.start_create_wizard();
        }
        let source = self.pending[idx].clone();
        let name = Path::new(&source)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "vault".to_string());
        let cipher = format!(
            "{}/.cipher.d/{}",
            Path::new(&source)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string()),
            name
        );
        let tmp_mount = format!("{}.mount_tmp", source);
        let keep_source_cfg = read_setting(&self.config, "create.keep_source", "false") == "true";

        self.pending_confirmed = None;
        self.wizard = Some(Wizard {
            direction: WizardDirection::Create,
            step: WizardStep::ConfigPaths,
            source,
            name,
            cipher,
            tmp_mount,
            vault_name: String::new(),
            dry_run: false,
            keep_source: keep_source_cfg,
            keep_source_locked: keep_source_cfg,
            restore: true,
            restore_locked: true,
            delete_cipher: false,
            delete_cipher_locked: true,
            password: String::new(),
            password_confirm: String::new(),
            confirming: false,
            delete_confirm_text: String::new(),
            progress: None,
            checks: HashMap::new(),
            error: None,
            selected: 0,
        });
        self.status = "配置路径和选项".to_string();
    }

    fn start_remove_wizard(&mut self) {
        let vault = match self.selected_vault() {
            Some(v) => v.clone(),
            None => {
                self.status = "无卷可选".to_string();
                return;
            }
        };
        if !vault.valid {
            self.overlay = Some(Overlay::Message(format!(
                "「{}」不是有效的 gocryptfs 加密卷，无法删除。\n\n请到 [2] 创建加密页处理。",
                vault.name
            )));
            return;
        }
        let restore_cfg = read_setting(&self.config, "remove.restore", "true") == "true";
        let delete_cipher_cfg =
            read_setting(&self.config, "remove.direct_delete_cipher", "false") == "true";

        self.vault_confirmed = None;
        self.wizard = Some(Wizard {
            direction: WizardDirection::Remove,
            step: WizardStep::ConfigPaths,
            source: vault.mount_point.clone(),
            name: vault.name.clone(),
            cipher: vault.path.clone(),
            tmp_mount: String::new(),
            vault_name: vault.name.clone(),
            dry_run: false,
            keep_source: false,
            keep_source_locked: false,
            restore: restore_cfg,
            restore_locked: restore_cfg,
            delete_cipher: delete_cipher_cfg,
            delete_cipher_locked: !delete_cipher_cfg,
            password: String::new(),
            password_confirm: String::new(),
            confirming: false,
            delete_confirm_text: String::new(),
            progress: None,
            checks: HashMap::new(),
            error: None,
            selected: 0,
        });
        self.status = "配置路径和选项".to_string();
    }

    fn wizard_advance(&mut self) {
        let mut wizard = match self.wizard.take() {
            Some(w) => w,
            None => return,
        };
        match wizard.step {
            WizardStep::ConfigPaths => {
                wizard.step = WizardStep::EnterPassword;
                wizard.password.clear();
                wizard.password_confirm.clear();
                wizard.confirming = false;
                wizard.error = None;
                self.wizard = Some(wizard);
                self.status = "输入密码".to_string();
            }
            WizardStep::EnterPassword => {
                if wizard.password.is_empty() {
                    wizard.error = Some("密码不能为空".to_string());
                    self.wizard = Some(wizard);
                    return;
                }
                if wizard.direction == WizardDirection::Remove {
                    wizard.step = WizardStep::ConfirmDelete;
                    wizard.delete_confirm_text.clear();
                    wizard.error = None;
                    self.wizard = Some(wizard);
                    self.status = "输入 DELETE 确认".to_string();
                    return;
                }
                if wizard.password != wizard.password_confirm {
                    wizard.error = Some("两次密码不一致".to_string());
                    wizard.confirming = false;
                    wizard.password_confirm.clear();
                    self.wizard = Some(wizard);
                    return;
                }
                self.wizard_execute(wizard);
            }
            WizardStep::ConfirmDelete => {
                if wizard.delete_confirm_text != "DELETE" {
                    wizard.error = Some("请输入大写 DELETE 以确认".to_string());
                    self.wizard = Some(wizard);
                    return;
                }
                self.wizard_execute(wizard);
            }
            WizardStep::Running => {
                self.wizard = Some(wizard);
            }
            WizardStep::Done | WizardStep::Failed(_) => {
                self.wizard = None;
                self.status = "就绪".to_string();
                self.reload_vaults();
                self.reload_pending();
            }
        }
    }

    fn wizard_execute(&mut self, mut wizard: Wizard) {
        let password = wizard.password.clone();
        wizard.step = WizardStep::Running;
        wizard.progress = None;
        wizard.checks.clear();
        wizard.error = None;

        let (action, args, desc) = match wizard.direction {
            WizardDirection::Create => {
                let mut a = vec![
                    "-c".to_string(),
                    self.config.clone(),
                    "create".to_string(),
                    wizard.source.clone(),
                    "--name".to_string(),
                    wizard.name.clone(),
                    "--cipher".to_string(),
                    wizard.cipher.clone(),
                    "--yes".to_string(),
                ];
                if wizard.dry_run {
                    a.push("--dry-run".to_string());
                }
                if wizard.keep_source {
                    a.push("--keep-source".to_string());
                } else {
                    a.push("--no-keep-source".to_string());
                }
                ("create", a, format!("创建 {}", wizard.name))
            }
            WizardDirection::Remove => {
                let mut a = vec![
                    "-c".to_string(),
                    self.config.clone(),
                    "remove".to_string(),
                    wizard.vault_name.clone(),
                    "--yes".to_string(),
                ];
                if wizard.dry_run {
                    a.push("--dry-run".to_string());
                }
                if !wizard.restore {
                    a.push("--no-restore".to_string());
                }
                if wizard.delete_cipher {
                    a.push("--delete-cipher".to_string());
                } else {
                    a.push("--keep-cipher".to_string());
                }
                ("remove", a, format!("删除 {}", wizard.vault_name))
            }
        };

        self.wizard = Some(wizard);
        let log = self.log_file.clone();
        let hist = self.history_file.clone();
        self.start_task(action, desc, args, Some(password), log, hist);
        self.status = "执行中...".to_string();
    }

    fn wizard_cancel(&mut self) {
        self.wizard = None;
        self.status = "已取消".to_string();
    }

    fn wizard_toggle_option(&mut self) {
        let mut wizard = match self.wizard.take() {
            Some(w) => w,
            None => return,
        };
        match wizard.direction {
            WizardDirection::Create => match wizard.selected {
                0 => wizard.dry_run = !wizard.dry_run,
                1 if !wizard.keep_source_locked => wizard.keep_source = !wizard.keep_source,
                _ => {}
            },
            WizardDirection::Remove => match wizard.selected {
                0 => wizard.dry_run = !wizard.dry_run,
                1 if !wizard.restore_locked => wizard.restore = !wizard.restore,
                2 if !wizard.delete_cipher_locked => wizard.delete_cipher = !wizard.delete_cipher,
                _ => {}
            },
        }
        self.wizard = Some(wizard);
    }

    fn handle_ctrl_d(&mut self) {
        let now = Instant::now();
        let is_continuous = self
            .last_ctrl_d
            .map(|t| now.duration_since(t) < Duration::from_secs(2))
            .unwrap_or(false);
        if is_continuous {
            self.ctrl_d_count += 1;
        } else {
            self.ctrl_d_count = 1;
        }
        self.last_ctrl_d = Some(now);
        if self.ctrl_d_count >= 3 {
            self.should_quit = true;
        } else {
            self.status = format!("再按 Ctrl+D {} 次强制退出", 3 - self.ctrl_d_count);
        }
    }

    fn export_help_file(&self) -> Result<PathBuf, String> {
        let content = self.help_markdown();
        std::fs::write(&self.help_file, content).map_err(|e| format!("{}", e))?;
        Ok(self.help_file.clone())
    }

    fn help_markdown(&self) -> String {
        let repo = std::env::var("GOCRYPTFS_REPO")
            .unwrap_or_else(|_| env!("CARGO_PKG_REPOSITORY").to_string());
        let cli = cli_path();

        format!(
            "# {app} 帮助\n\n\
## 版本信息\n\n\
- 版本: {version}\n\
- 提交: {commit}\n\
- 构建: {build}\n\
- 仓库: {repo}\n\n\
## 路径\n\n\
- CLI: {cli}\n\
- 配置: {cfg}\n\
- 数据: {data}\n\
- 日志: {log}\n\
- 历史: {hist}\n\n\
## 启动参数\n\n\
- `-c, --config <PATH>` — 配置文件路径\n\
- `-D, --data-dir <DIR>` — 数据目录（日志、历史、帮助）\n\
- `-h, --help` — 显示帮助\n\
- `-V, --version` — 显示版本信息\n\n\
## 全局键\n\n\
| 键 | 功能 |\n\
|----|------|\n\
| `Tab` | 换页 |\n\
| `1` / `2` / `3` | 直接跳页 |\n\
| `Alt+1/2/3/4` | 焦点到 列表/详情/目录/输出 |\n\
| `s` | 设置浮层 |\n\
| `h` | 历史浮层 |\n\
| `e` | 外部编辑器打开配置 |\n\
| `r` | 刷新 |\n\
| `?` | 帮助浮层 |\n\
| `H` | 导出本帮助到文件（帮助浮层内）|\n\
| `q` | 退出 |\n\
| `Ctrl+C` | 中断当前任务 |\n\
| `Ctrl+D` × 3（2 秒内） | 强制退出 |\n\n\
## TAB1 挂载/卸载\n\n\
- 移动 `j/k/↑/↓`，选中 `Space`\n\
- 挂载/卸载 `m/Enter`，卸载 `u`\n\
- 目录 `l`，树状 `t`，打开 `o`\n\n\
## TAB2 创建加密\n\n\
- 移动 `j/k/↑/↓`，选中 `Space`\n\
- 进入创建向导 `c/Enter`\n\
- 目录 `l`，树状 `t`\n\n\
## TAB3 删除加密\n\n\
- 移动 `j/k/↑/↓`，选中 `Space`\n\
- 进入删除向导 `d/Enter`\n\
- 目录 `l`，树状 `t`\n\n\
## 历史浮层\n\n\
- 移动 `j/k`\n\
- 过滤：来源 `s`，结果 `r`，操作 `a`\n\
- 关闭 `Esc`\n",
            app = APP_NAME,
            version = APP_VERSION,
            commit = APP_COMMIT,
            build = APP_BUILD_TIME,
            repo = repo,
            cli = cli,
            cfg = self.config,
            data = self.data_dir.display(),
            log = self.log_file.display(),
            hist = self.history_file.display(),
        )
    }
}

// ============================================================
// 键盘处理
// ============================================================

fn is_delete_key(code: KeyCode, mods: KeyModifiers) -> bool {
    match code {
        KeyCode::Backspace | KeyCode::Delete => true,
        KeyCode::Char('\x7f') | KeyCode::Char('\x08') => true,
        KeyCode::Char('h') | KeyCode::Char('w') if mods.contains(KeyModifiers::CONTROL) => true,
        _ => false,
    }
}

fn handle_key(app: &mut App, key: KeyCode, mods: KeyModifiers) {
    if mods.contains(KeyModifiers::CONTROL) && key == KeyCode::Char('c') {
        app.interrupt_task();
        return;
    }
    if mods.contains(KeyModifiers::CONTROL) && key == KeyCode::Char('d') {
        app.handle_ctrl_d();
        return;
    }

    if let Some(pw) = &mut app.password_input {
        if is_delete_key(key, mods) {
            pw.buffer.pop();
            return;
        }
        match key {
            KeyCode::Esc => {
                app.password_input = None;
                app.status = "已取消".to_string();
            }
            KeyCode::Enter => {
                let vault_name = pw.vault_name.clone();
                let password = pw.buffer.clone();
                if password.is_empty() {
                    pw.error = Some("密码不能为空".to_string());
                    return;
                }
                app.password_input = None;
                let args = vec![
                    "-c".into(),
                    app.config.clone(),
                    "mount".into(),
                    vault_name.clone(),
                ];
                let log = app.log_file.clone();
                let hist = app.history_file.clone();
                app.start_task(
                    "mount",
                    format!("挂载 {}", vault_name),
                    args,
                    Some(password),
                    log,
                    hist,
                );
            }
            KeyCode::Char(c) if !c.is_control() => {
                pw.buffer.push(c);
            }
            _ => {}
        }
        return;
    }

    if app.wizard.is_some() {
        let step = app.wizard.as_ref().unwrap().step.clone();
        match step {
            WizardStep::ConfigPaths => match key {
                KeyCode::Esc => app.wizard_cancel(),
                KeyCode::Enter => app.wizard_advance(),
                KeyCode::Char('j') | KeyCode::Down => {
                    let max = app
                        .wizard
                        .as_ref()
                        .map(|w| w.option_count().saturating_sub(1))
                        .unwrap_or(0);
                    if let Some(w) = &mut app.wizard {
                        w.selected = (w.selected + 1).min(max);
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    if let Some(w) = &mut app.wizard {
                        w.selected = w.selected.saturating_sub(1);
                    }
                }
                KeyCode::Char(' ') => app.wizard_toggle_option(),
                _ => {}
            },
            WizardStep::EnterPassword => {
                if is_delete_key(key, mods) {
                    if let Some(w) = &mut app.wizard {
                        if w.confirming {
                            w.password_confirm.pop();
                        } else {
                            w.password.pop();
                        }
                    }
                    return;
                }
                match key {
                    KeyCode::Esc => app.wizard_cancel(),
                    KeyCode::Enter => {
                        let confirming = app.wizard.as_ref().map(|w| w.confirming).unwrap_or(false);
                        let direction = app.wizard.as_ref().map(|w| w.direction);
                        if !confirming {
                            let password = app
                                .wizard
                                .as_ref()
                                .map(|w| w.password.clone())
                                .unwrap_or_default();
                            if password.is_empty() {
                                if let Some(w) = &mut app.wizard {
                                    w.error = Some("密码不能为空".to_string());
                                }
                            } else if direction == Some(WizardDirection::Remove) {
                                app.wizard_advance();
                            } else if let Some(w) = &mut app.wizard {
                                w.confirming = true;
                                w.error = None;
                            }
                        } else {
                            app.wizard_advance();
                        }
                    }
                    KeyCode::Char(c) if !c.is_control() => {
                        if let Some(w) = &mut app.wizard {
                            if w.confirming {
                                w.password_confirm.push(c);
                            } else {
                                w.password.push(c);
                            }
                        }
                    }
                    _ => {}
                }
            }
            WizardStep::ConfirmDelete => {
                if is_delete_key(key, mods) {
                    if let Some(w) = &mut app.wizard {
                        w.delete_confirm_text.pop();
                    }
                    return;
                }
                match key {
                    KeyCode::Esc => app.wizard_cancel(),
                    KeyCode::Enter => app.wizard_advance(),
                    KeyCode::Char(c) if !c.is_control() => {
                        if let Some(w) = &mut app.wizard {
                            w.delete_confirm_text.push(c);
                        }
                    }
                    _ => {}
                }
            }
            WizardStep::Running => {
                if key == KeyCode::Esc {
                    app.status = "运行中按 Ctrl+C 中断".to_string();
                }
            }
            WizardStep::Done | WizardStep::Failed(_) => {
                app.wizard_advance();
            }
        }
        return;
    }

    if app.overlay.is_some() {
        let overlay = app.overlay.take().unwrap();
        match overlay {
            Overlay::Help => {
                if key == KeyCode::Char('H') {
                    match app.export_help_file() {
                        Ok(p) => {
                            app.status = format!("已导出: {}", p.display());
                            app.overlay =
                                Some(Overlay::Message(format!("帮助已导出:\n\n{}", p.display())));
                        }
                        Err(e) => {
                            app.overlay = Some(Overlay::Message(format!("导出失败:\n{}", e)));
                        }
                    }
                } else if !matches!(key, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')) {
                    app.overlay = Some(Overlay::Help);
                }
            }
            Overlay::Message(_) => {}
            Overlay::ConfirmUmount { vault_index } => match key {
                KeyCode::Char('y') | KeyCode::Char('Y') => app.start_umount_confirmed(vault_index),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    app.status = "已取消卸载".to_string();
                }
                _ => app.overlay = Some(Overlay::ConfirmUmount { vault_index }),
            },
            Overlay::Settings { tab, scope_index } => match key {
                KeyCode::Esc => {}
                KeyCode::Tab => {
                    let n = app.scope_names.len().max(1);
                    app.overlay = Some(Overlay::Settings {
                        tab,
                        scope_index: (scope_index + 1) % n,
                    });
                }
                KeyCode::Char('g') => {
                    app.overlay = Some(Overlay::Settings {
                        tab: SettingsTab::Gocryptfs,
                        scope_index,
                    })
                }
                KeyCode::Char('r') => {
                    app.overlay = Some(Overlay::Settings {
                        tab: SettingsTab::Rsync,
                        scope_index,
                    })
                }
                KeyCode::Char('f') => {
                    app.overlay = Some(Overlay::Settings {
                        tab: SettingsTab::Filters,
                        scope_index,
                    })
                }
                KeyCode::Char('p') => {
                    app.overlay = Some(Overlay::Settings {
                        tab: SettingsTab::Perm,
                        scope_index,
                    })
                }
                _ => app.overlay = Some(Overlay::Settings { tab, scope_index }),
            },
            Overlay::History => match key {
                KeyCode::Esc => {}
                KeyCode::Char('j') | KeyCode::Down => {
                    let filtered = app.filtered_history();
                    let len = filtered.len().min(MAX_HISTORY_DISPLAY);
                    let max = len.saturating_sub(1);
                    app.history_selected = (app.history_selected + 1).min(max);
                    app.overlay = Some(Overlay::History);
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    app.history_selected = app.history_selected.saturating_sub(1);
                    app.overlay = Some(Overlay::History);
                }
                KeyCode::Char('s') => {
                    app.history_filter.cycle_src();
                    app.history_selected = 0;
                    app.overlay = Some(Overlay::History);
                }
                KeyCode::Char('r') => {
                    app.history_filter.cycle_result();
                    app.history_selected = 0;
                    app.overlay = Some(Overlay::History);
                }
                KeyCode::Char('a') => {
                    app.history_filter.cycle_action();
                    app.history_selected = 0;
                    app.overlay = Some(Overlay::History);
                }
                _ => app.overlay = Some(Overlay::History),
            },
        }
        return;
    }

    if mods.contains(KeyModifiers::ALT) {
        match key {
            KeyCode::Char('1') => {
                app.focus = Focus::List;
                return;
            }
            KeyCode::Char('2') => {
                app.focus = Focus::Detail;
                return;
            }
            KeyCode::Char('3') => {
                app.focus = Focus::Dir;
                return;
            }
            KeyCode::Char('4') => {
                app.focus = Focus::Output;
                return;
            }
            _ => {}
        }
    }

    match key {
        KeyCode::Char('q') => {
            app.should_quit = true;
            return;
        }
        KeyCode::Char('?') => {
            app.overlay = Some(Overlay::Help);
            return;
        }
        KeyCode::Char('s') => {
            app.update_scope_names();
            app.overlay = Some(Overlay::Settings {
                tab: SettingsTab::Gocryptfs,
                scope_index: 0,
            });
            return;
        }
        KeyCode::Char('h') => {
            app.history_entries = load_history(&app.log_file);
            app.history_selected = 0;
            app.overlay = Some(Overlay::History);
            return;
        }
        KeyCode::Char('e') => {
            app.editor_request = true;
            return;
        }
        KeyCode::Char('r') => {
            app.reload_vaults();
            app.reload_pending();
            app.dir_view = DirView::default();
            app.status = "已刷新".to_string();
            return;
        }
        KeyCode::Tab | KeyCode::BackTab => {
            app.page = match app.page {
                Page::Mount => Page::Create,
                Page::Create => Page::Remove,
                Page::Remove => Page::Mount,
            };
            app.focus = Focus::List;
            app.vault_confirmed = None;
            app.pending_confirmed = None;
            app.dir_view = DirView::default();
            app.status = match app.page {
                Page::Mount => "挂载/卸载".to_string(),
                Page::Create => "创建加密".to_string(),
                Page::Remove => "删除加密".to_string(),
            };
            return;
        }
        KeyCode::Char('1') => {
            app.page = Page::Mount;
            app.focus = Focus::List;
            app.vault_confirmed = None;
            app.pending_confirmed = None;
            return;
        }
        KeyCode::Char('2') => {
            app.page = Page::Create;
            app.focus = Focus::List;
            app.vault_confirmed = None;
            app.pending_confirmed = None;
            return;
        }
        KeyCode::Char('3') => {
            app.page = Page::Remove;
            app.focus = Focus::List;
            app.vault_confirmed = None;
            app.pending_confirmed = None;
            return;
        }
        _ => {}
    }

    match app.focus {
        Focus::List => match app.page {
            Page::Mount => match key {
                KeyCode::Char('j') | KeyCode::Down => app.next_vault(),
                KeyCode::Char('k') | KeyCode::Up => app.previous_vault(),
                KeyCode::Char(' ') => {
                    if let Some(i) = app.vault_list_state.selected() {
                        app.vault_confirmed = Some(i);
                        app.status = "已选中（按 Enter 或 m 挂载/卸载）".to_string();
                    }
                }
                KeyCode::Enter | KeyCode::Char('m') => app.enter_action(),
                KeyCode::Char('u') => app.trigger_umount_or_confirm(),
                KeyCode::Char('l') => app.load_dir_view(false),
                KeyCode::Char('t') => app.load_dir_view(true),
                KeyCode::Char('o') => {
                    if let Some(v) = app.selected_vault() {
                        let mp = v.mount_point.clone();
                        let _ = Command::new("xdg-open").arg(&mp).spawn();
                        app.status = format!("打开: {}", mp);
                    }
                }
                _ => {}
            },
            Page::Create => match key {
                KeyCode::Char('j') | KeyCode::Down => app.next_pending(),
                KeyCode::Char('k') | KeyCode::Up => app.previous_pending(),
                KeyCode::Char(' ') => {
                    if let Some(i) = app.pending_list_state.selected() {
                        app.pending_confirmed = Some(i);
                        app.status = "已选中（按 c 或 Enter 进入创建向导）".to_string();
                    }
                }
                KeyCode::Enter | KeyCode::Char('c') => app.enter_create_wizard(),
                KeyCode::Char('l') => app.load_dir_view(false),
                KeyCode::Char('t') => app.load_dir_view(true),
                _ => {}
            },
            Page::Remove => match key {
                KeyCode::Char('j') | KeyCode::Down => app.next_vault(),
                KeyCode::Char('k') | KeyCode::Up => app.previous_vault(),
                KeyCode::Char(' ') => {
                    if let Some(i) = app.vault_list_state.selected() {
                        app.vault_confirmed = Some(i);
                        app.status = "已选中（按 d 或 Enter 进入删除向导）".to_string();
                    }
                }
                KeyCode::Enter | KeyCode::Char('d') => app.enter_remove_wizard(),
                KeyCode::Char('l') => app.load_dir_view(false),
                KeyCode::Char('t') => app.load_dir_view(true),
                _ => {}
            },
        },
        Focus::Detail => match key {
            KeyCode::Up => app.detail_scroll.v = app.detail_scroll.v.saturating_sub(1),
            KeyCode::Down => app.detail_scroll.v = app.detail_scroll.v.saturating_add(1),
            KeyCode::Left => app.detail_scroll.h = app.detail_scroll.h.saturating_sub(4),
            KeyCode::Right => app.detail_scroll.h = app.detail_scroll.h.saturating_add(4),
            KeyCode::PageUp => app.detail_scroll.v = app.detail_scroll.v.saturating_sub(5),
            KeyCode::PageDown => app.detail_scroll.v = app.detail_scroll.v.saturating_add(5),
            KeyCode::Char('g') => app.detail_scroll.v = 0,
            KeyCode::Char('G') => app.detail_scroll.v = u16::MAX / 2,
            _ => {}
        },
        Focus::Dir => match key {
            KeyCode::Char('c') => {
                app.dir_view = DirView::default();
                app.dir_scroll = ScrollState::default();
                app.status = "已清空目录".to_string();
            }
            KeyCode::Char('l') => app.load_dir_view(false),
            KeyCode::Char('t') => app.load_dir_view(true),
            KeyCode::Up => app.dir_scroll.v = app.dir_scroll.v.saturating_sub(1),
            KeyCode::Down => app.dir_scroll.v = app.dir_scroll.v.saturating_add(1),
            KeyCode::Left => app.dir_scroll.h = app.dir_scroll.h.saturating_sub(4),
            KeyCode::Right => app.dir_scroll.h = app.dir_scroll.h.saturating_add(4),
            KeyCode::PageUp => app.dir_scroll.v = app.dir_scroll.v.saturating_sub(10),
            KeyCode::PageDown => app.dir_scroll.v = app.dir_scroll.v.saturating_add(10),
            KeyCode::Char('g') => app.dir_scroll.v = 0,
            KeyCode::Char('G') => {
                let total: usize = app
                    .dir_view
                    .sections
                    .iter()
                    .map(|s| s.lines.len() + 2)
                    .sum();
                app.dir_scroll.v = total.saturating_sub(1) as u16;
            }
            _ => {}
        },
        Focus::Output => match key {
            KeyCode::Char('c') => {
                app.output.clear();
                app.output_scroll = ScrollState::default();
                app.status = "已清空输出".to_string();
            }
            KeyCode::Up => app.output_scroll.v = app.output_scroll.v.saturating_sub(1),
            KeyCode::Down => app.output_scroll.v = app.output_scroll.v.saturating_add(1),
            KeyCode::Left => app.output_scroll.h = app.output_scroll.h.saturating_sub(4),
            KeyCode::Right => app.output_scroll.h = app.output_scroll.h.saturating_add(4),
            KeyCode::PageUp => app.output_scroll.v = app.output_scroll.v.saturating_sub(10),
            KeyCode::PageDown => app.output_scroll.v = app.output_scroll.v.saturating_add(10),
            KeyCode::Char('g') => app.output_scroll.v = 0,
            KeyCode::Char('G') => app.output_scroll.v = app.output.len().saturating_sub(1) as u16,
            _ => {}
        },
    }
}

// ============================================================
// UI 渲染
// ============================================================

fn ui(f: &mut Frame, app: &mut App) {
    let area = f.size();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(10),
            Constraint::Length(7),
            Constraint::Length(3),
        ])
        .split(area);

    render_top_bar(f, app, chunks[0]);
    render_tab_bar(f, app, chunks[1]);

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[2]);

    render_list(f, app, main_chunks[0]);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_chunks[1]);

    render_detail(f, app, right_chunks[0]);
    render_dir(f, app, right_chunks[1]);

    render_output(f, app, chunks[3]);
    render_status(f, app, chunks[4]);

    if app.wizard.is_some() {
        f.render_widget(Clear, chunks[2]);
        render_wizard(f, app, chunks[2]);
    }

    if let Some(overlay) = &app.overlay {
        match overlay {
            Overlay::Help => render_help_overlay(f, area, app),
            Overlay::Settings { tab, scope_index } => {
                render_settings_overlay(f, *tab, *scope_index, app, area);
            }
            Overlay::History => {
                let filtered = app.filtered_history();
                render_history_overlay(
                    f,
                    &filtered,
                    app.history_selected,
                    &app.history_filter,
                    area,
                );
            }
            Overlay::Message(msg) => render_message_overlay(f, msg, area),
            Overlay::ConfirmUmount { vault_index } => {
                render_confirm_umount(f, app, *vault_index, area);
            }
        }
    }

    if let Some(pw) = &app.password_input {
        render_password_input(f, pw, area);
    }
}

fn focus_border(focus: Focus, current: Focus) -> Style {
    if focus == current {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        if max < 2 {
            return "…".to_string();
        }
        let n = max - 1;
        let total = s.chars().count();
        let taken: String = s.chars().skip(total - n).collect();
        format!("…{}", taken)
    }
}

fn shorten_home(s: &str) -> String {
    if let Some(home) = std::env::var_os("HOME") {
        let h = home.to_string_lossy();
        if !h.is_empty() && s.starts_with(h.as_ref()) {
            return format!("~{}", &s[h.len()..]);
        }
    }
    s.to_string()
}

fn render_top_bar(f: &mut Frame, app: &App, area: Rect) {
    let w = area.width as usize;
    let ver = short_version();
    let ver_w = ver.chars().count();

    let cmd_max = w.saturating_sub(8 + ver_w + 2).max(20);
    let cmd_str = truncate(&app.startup_command, cmd_max);
    let cmd_disp_w = cmd_str.chars().count() + 6;
    let pad1 = w.saturating_sub(cmd_disp_w + ver_w).max(1);
    let line1 = Line::from(vec![
        Span::styled("命令: ", Style::default().fg(Color::Cyan)),
        Span::raw(cmd_str),
        Span::raw(" ".repeat(pad1)),
        Span::styled(
            ver,
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let cli = cli_path();
    let cfg = shorten_home(&app.config);
    let cfg_max = w.saturating_sub(8 + cli.chars().count() + 8).max(20);
    let cfg_str = truncate(&cfg, cfg_max);
    let line2 = Line::from(vec![
        Span::styled("CLI:  ", Style::default().fg(Color::Cyan)),
        Span::raw(cli),
        Span::raw("  "),
        Span::styled("配置: ", Style::default().fg(Color::Cyan)),
        Span::raw(cfg_str),
    ]);

    let data = shorten_home(&app.data_dir.to_string_lossy());
    let hist_name = app
        .history_file
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "history.jsonl".to_string());
    let log_name = app
        .log_file
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "app.log.jsonl".to_string());

    let line3 = if w >= 100 {
        Line::from(vec![
            Span::styled("数据: ", Style::default().fg(Color::Cyan)),
            Span::raw(truncate(&data, w.saturating_sub(40))),
            Span::raw("  "),
            Span::styled("日志: ", Style::default().fg(Color::Cyan)),
            Span::raw(log_name),
            Span::raw("  "),
            Span::styled("历史: ", Style::default().fg(Color::Cyan)),
            Span::raw(hist_name),
        ])
    } else if w >= 80 {
        Line::from(vec![
            Span::styled("数据: ", Style::default().fg(Color::Cyan)),
            Span::raw(truncate(&data, w.saturating_sub(22))),
            Span::raw("  "),
            Span::styled("日志: ", Style::default().fg(Color::Cyan)),
            Span::raw(log_name),
        ])
    } else {
        Line::from(vec![
            Span::styled("数据: ", Style::default().fg(Color::Cyan)),
            Span::raw(truncate(&data, w.saturating_sub(8))),
        ])
    };

    let para = Paragraph::new(vec![line1, line2, line3])
        .style(Style::default().fg(Color::White).bg(Color::Black));
    f.render_widget(para, area);
}

fn render_tab_bar(f: &mut Frame, app: &App, area: Rect) {
    let tab_style = |p: Page| {
        if app.page == p {
            Style::default()
                .bg(Color::Yellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        }
    };
    let line = Line::from(vec![
        Span::styled(" [1] 挂载/卸载 ", tab_style(Page::Mount)),
        Span::raw("  "),
        Span::styled(" [2] 创建加密 ", tab_style(Page::Create)),
        Span::raw("  "),
        Span::styled(" [3] 删除加密 ", tab_style(Page::Remove)),
        Span::raw("      "),
        Span::styled("换页[Tab]", Style::default().fg(Color::Cyan)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn render_list(f: &mut Frame, app: &mut App, area: Rect) {
    let (items, title): (Vec<ListItem>, String) = match app.page {
        Page::Mount | Page::Remove => {
            let items: Vec<ListItem> = if app.vaults.is_empty() {
                vec![ListItem::new(Span::styled(
                    "（无卷）",
                    Style::default().fg(Color::Gray),
                ))]
            } else {
                app.vaults
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let is_confirmed = app.vault_confirmed == Some(i);
                        let prefix = if is_confirmed { "▶ " } else { "  " };
                        let (icon, icon_color) = if !v.valid {
                            ("⚠ ", Color::Red)
                        } else if v.mounted {
                            ("● ", Color::Green)
                        } else {
                            ("○ ", Color::Gray)
                        };
                        let name_style = if !v.valid {
                            Style::default().fg(Color::Red)
                        } else if is_confirmed {
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD)
                        } else if v.mounted {
                            Style::default().fg(Color::White)
                        } else {
                            Style::default().fg(Color::Gray)
                        };
                        let lock = if !v.valid {
                            "非加密卷".to_string()
                        } else if v.locked {
                            "🔒".to_string()
                        } else {
                            "  ".to_string()
                        };
                        let lock_color = if !v.valid { Color::Red } else { Color::Reset };

                        ListItem::new(Line::from(vec![
                            Span::styled(prefix, Style::default().fg(Color::Yellow)),
                            Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
                            Span::styled(v.name.clone(), name_style),
                            Span::raw("  "),
                            Span::styled(lock, Style::default().fg(lock_color)),
                        ]))
                    })
                    .collect()
            };
            let title = if app.page == Page::Mount {
                " 卷列表 "
            } else {
                " 加密卷列表 "
            };
            (items, title.to_string())
        }
        Page::Create => {
            let items: Vec<ListItem> = if app.pending.is_empty() {
                vec![ListItem::new(Span::styled(
                    "（无待处理目录）",
                    Style::default().fg(Color::Gray),
                ))]
            } else {
                app.pending
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let is_confirmed = app.pending_confirmed == Some(i);
                        let prefix = if is_confirmed { "▶ " } else { "  " };
                        let created = app.pending_created_vault(p).is_some();
                        let name_style = if created {
                            Style::default().fg(Color::Green)
                        } else if is_confirmed {
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        let tail = if created { "  ✓ 已创建" } else { "" };
                        ListItem::new(Line::from(vec![
                            Span::styled(prefix, Style::default().fg(Color::Yellow)),
                            Span::styled(p.clone(), name_style),
                            Span::styled(tail.to_string(), Style::default().fg(Color::Green)),
                        ]))
                    })
                    .collect()
            };
            (items, " 待处理目录 ".to_string())
        }
    };

    let mut list_state = if app.page == Page::Create {
        app.pending_list_state.clone()
    } else {
        app.vault_list_state.clone()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(focus_border(Focus::List, app.focus)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    f.render_stateful_widget(list, area, &mut list_state);

    if app.page == Page::Create {
        app.pending_list_state = list_state;
    } else {
        app.vault_list_state = list_state;
    }
}

fn render_detail(f: &mut Frame, app: &mut App, area: Rect) {
    let lines: Vec<Line> = match app.page {
        Page::Mount | Page::Remove => match app.selected_vault() {
            Some(v) => {
                let mounted_str = if v.mounted {
                    "🔓 已挂载"
                } else {
                    "🔒 未挂载"
                };
                let mounted_color = if v.mounted { Color::Green } else { Color::Red };
                let lock_str = if v.mounted {
                    "🔓 由 gocryptfs 接管"
                } else if v.locked {
                    "🔒 只读锁定 (555)"
                } else {
                    "🔓 未锁定 (755)"
                };
                let valid_line = if v.valid {
                    Line::from(vec![
                        Span::styled("有效性:    ", Style::default().fg(Color::Cyan)),
                        Span::styled("✓ gocryptfs 卷", Style::default().fg(Color::Green)),
                    ])
                } else {
                    Line::from(vec![
                        Span::styled("有效性:    ", Style::default().fg(Color::Cyan)),
                        Span::styled(
                            "✗ 非加密卷（缺少 gocryptfs.conf）",
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ),
                    ])
                };
                vec![
                    Line::from(vec![
                        Span::styled("ID:        ", Style::default().fg(Color::Cyan)),
                        Span::raw(v.id.to_string()),
                    ]),
                    Line::from(vec![
                        Span::styled("名称:      ", Style::default().fg(Color::Cyan)),
                        Span::raw(v.name.clone()),
                    ]),
                    Line::from(vec![
                        Span::styled("加密路径:  ", Style::default().fg(Color::Cyan)),
                        Span::raw(v.path.clone()),
                    ]),
                    Line::from(vec![
                        Span::styled("挂载点:    ", Style::default().fg(Color::Cyan)),
                        Span::raw(v.mount_point.clone()),
                    ]),
                    Line::from(vec![
                        Span::styled("状态:      ", Style::default().fg(Color::Cyan)),
                        Span::styled(mounted_str, Style::default().fg(mounted_color)),
                    ]),
                    Line::from(vec![
                        Span::styled("保护:      ", Style::default().fg(Color::Cyan)),
                        Span::raw(lock_str),
                    ]),
                    valid_line,
                ]
            }
            None => vec![Line::from(Span::styled(
                "（无卷）",
                Style::default().fg(Color::Gray),
            ))],
        },
        Page::Create => {
            let idx = app.pending_list_state.selected();
            match idx.and_then(|i| app.pending.get(i).cloned()) {
                Some(source) => {
                    let name = Path::new(&source)
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "vault".to_string());
                    let cipher = format!(
                        "{}/.cipher.d/{}",
                        Path::new(&source)
                            .parent()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|| ".".to_string()),
                        name
                    );
                    let tmp_mount = format!("{}.mount_tmp", source);

                    let created_vault = app.pending_created_vault(&source).cloned();
                    let status_line = if let Some(v) = &created_vault {
                        Line::from(vec![
                            Span::styled("状态:      ", Style::default().fg(Color::Cyan)),
                            Span::styled(
                                format!("✓ 已创建（卷「{}」）", v.name),
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ])
                    } else {
                        Line::from(vec![
                            Span::styled("状态:      ", Style::default().fg(Color::Cyan)),
                            Span::styled("待创建", Style::default().fg(Color::Yellow)),
                        ])
                    };

                    vec![
                        Line::from(vec![
                            Span::styled("任务名称:  ", Style::default().fg(Color::Cyan)),
                            Span::raw(name.clone()),
                        ]),
                        Line::from(vec![
                            Span::styled("源目录:    ", Style::default().fg(Color::Cyan)),
                            Span::raw(source.clone()),
                        ]),
                        Line::from(vec![
                            Span::styled("加密目录:  ", Style::default().fg(Color::Cyan)),
                            Span::raw(cipher.clone()),
                        ]),
                        Line::from(vec![
                            Span::styled("临时挂载:  ", Style::default().fg(Color::Cyan)),
                            Span::raw(tmp_mount.clone()),
                        ]),
                        Line::from(vec![
                            Span::styled("目标挂载:  ", Style::default().fg(Color::Cyan)),
                            Span::raw(source.clone()),
                        ]),
                        status_line,
                    ]
                }
                None => vec![Line::from(Span::styled(
                    "（无待处理目录）",
                    Style::default().fg(Color::Gray),
                ))],
            }
        }
    };

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 详情 ")
                .border_style(focus_border(Focus::Detail, app.focus)),
        )
        .wrap(Wrap { trim: true })
        .scroll((app.detail_scroll.v, app.detail_scroll.h));
    f.render_widget(para, area);
}

fn render_dir(f: &mut Frame, app: &mut App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    if !app.dir_view.sections.is_empty() {
        for (i, section) in app.dir_view.sections.iter().enumerate() {
            if i > 0 {
                lines.push(Line::from(""));
            }
            lines.push(Line::from(Span::styled(
                section.title.clone(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                "─".repeat(area.width.saturating_sub(2) as usize),
                Style::default().fg(Color::DarkGray),
            )));
            for l in &section.lines {
                lines.push(Line::from(l.as_str()));
            }
        }
    }

    let title = if app.dir_view.mode.is_empty() {
        " 目录 ".to_string()
    } else {
        format!(" 目录 ({}) ", app.dir_view.mode)
    };

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(focus_border(Focus::Dir, app.focus)),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.dir_scroll.v, app.dir_scroll.h));
    f.render_widget(para, area);
}

fn render_output(f: &mut Frame, app: &App, area: Rect) {
    let lines: Vec<Line> = app.output.iter().map(|s| Line::from(s.as_str())).collect();
    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 输出 ")
                .border_style(focus_border(Focus::Output, app.focus)),
        )
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .wrap(Wrap { trim: false })
        .scroll((app.output_scroll.v, app.output_scroll.h));
    f.render_widget(para, area);
}

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let global_line = Line::from(vec![
        Span::styled("全局: ", Style::default().fg(Color::Yellow)),
        Span::raw("换页[Tab] 焦点[Alt+1/2/3/4] 设置[s] 历史[h] 编辑[e] 刷新[r] 帮助[?] 退出[q]"),
    ]);

    let page_hint = match app.page {
        Page::Mount => "页面: [1] 挂载/卸载[m/Enter] 卸载[u] 目录[l] 树状[t] 打开[o]",
        Page::Create => "页面: [2] 创建向导[c/Enter] 目录[l] 树状[t]",
        Page::Remove => "页面: [3] 删除向导[d/Enter] 目录[l] 树状[t]",
    };
    let page_line = Line::from(Span::raw(page_hint));

    let focus_str = match app.focus {
        Focus::List => "列表",
        Focus::Detail => "详情",
        Focus::Dir => "目录",
        Focus::Output => "输出",
    };
    let area_hint = match app.focus {
        Focus::List => "移动[j/k/↑/↓] 选中[Space]".to_string(),
        Focus::Detail => "滚动[↑↓←→] 翻页[PgUp/PgDn] 首尾[g/G]".to_string(),
        Focus::Dir => "滚动[↑↓←→] 翻页[PgUp/PgDn] 首尾[g/G] 清空[c] 刷新[l/t]".to_string(),
        Focus::Output => "滚动[↑↓←→] 翻页[PgUp/PgDn] 首尾[g/G] 清空[c]".to_string(),
    };
    let task_info = if let Some(t) = &app.task {
        match t.status {
            TaskStatus::Running => format!("运行中: {}", t.description),
            TaskStatus::Done => format!("完成: {}", t.description),
            TaskStatus::Failed(_) => format!("失败: {}", t.description),
        }
    } else {
        "无".to_string()
    };
    let region_line = Line::from(vec![
        Span::styled(
            format!("区域: {} │ ", focus_str),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw(area_hint),
        Span::raw("    "),
        Span::styled("状态: ", Style::default().fg(Color::Cyan)),
        Span::raw(&app.status),
        Span::styled(" 任务: ", Style::default().fg(Color::Cyan)),
        Span::raw(task_info),
    ]);

    let para = Paragraph::new(vec![global_line, page_line, region_line])
        .style(Style::default().fg(Color::White).bg(Color::DarkGray));
    f.render_widget(para, area);
}

fn render_wizard(f: &mut Frame, app: &App, area: Rect) {
    let wizard = match &app.wizard {
        Some(w) => w,
        None => return,
    };
    let mut lines: Vec<Line> = Vec::new();

    let title = match wizard.direction {
        WizardDirection::Create => "🆕 创建加密",
        WizardDirection::Remove => "🗑  删除加密",
    };
    lines.push(Line::from(Span::styled(
        title,
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    match &wizard.step {
        WizardStep::ConfigPaths => {
            lines.push(Line::from("步骤 1/3: 确认路径与选项"));
            lines.push(Line::from(""));
            match wizard.direction {
                WizardDirection::Create => {
                    lines.push(Line::from(vec![
                        Span::styled("源目录:   ", Style::default().fg(Color::Cyan)),
                        Span::raw(&wizard.source),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("加密目录: ", Style::default().fg(Color::Cyan)),
                        Span::raw(&wizard.cipher),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("临时挂载: ", Style::default().fg(Color::Cyan)),
                        Span::raw(&wizard.tmp_mount),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(render_option(
                        0,
                        wizard.selected,
                        wizard.dry_run,
                        false,
                        "预览模式",
                    ));
                    let (keep_text, keep_locked) = if wizard.keep_source_locked {
                        ("保留源文件 · 配置已锁定", true)
                    } else {
                        ("保留源文件", false)
                    };
                    lines.push(render_option(
                        1,
                        wizard.selected,
                        wizard.keep_source,
                        keep_locked,
                        keep_text,
                    ));
                }
                WizardDirection::Remove => {
                    lines.push(Line::from(vec![
                        Span::styled("加密卷:   ", Style::default().fg(Color::Cyan)),
                        Span::raw(&wizard.vault_name),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("加密目录: ", Style::default().fg(Color::Cyan)),
                        Span::raw(&wizard.cipher),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("挂载点:   ", Style::default().fg(Color::Cyan)),
                        Span::raw(&wizard.source),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(render_option(
                        0,
                        wizard.selected,
                        wizard.dry_run,
                        false,
                        "预览模式",
                    ));
                    let (restore_text, restore_locked) = if wizard.restore_locked {
                        ("还原明文 · 配置已锁定", true)
                    } else {
                        ("还原明文", false)
                    };
                    lines.push(render_option(
                        1,
                        wizard.selected,
                        wizard.restore,
                        restore_locked,
                        restore_text,
                    ));
                    let (del_text, del_locked) = if wizard.delete_cipher_locked {
                        ("直接删除加密后端 · 配置未授权", true)
                    } else {
                        ("直接删除加密后端", false)
                    };
                    lines.push(render_option(
                        2,
                        wizard.selected,
                        wizard.delete_cipher,
                        del_locked,
                        del_text,
                    ));
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "切换[Space] 移动[j/k] 继续[Enter] 取消[Esc]",
                Style::default().fg(Color::Gray),
            )));
        }
        WizardStep::EnterPassword => {
            lines.push(Line::from("步骤 2/3: 输入密码"));
            lines.push(Line::from(""));

            if wizard.direction == WizardDirection::Remove || !wizard.confirming {
                lines.push(Line::from("请输入密码："));
                lines.push(Line::from(Span::styled(
                    "*".repeat(wizard.password.chars().count()),
                    Style::default().fg(Color::White),
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    "密码已输入 ✓",
                    Style::default().fg(Color::Green),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from("确认密码："));
                lines.push(Line::from(Span::styled(
                    "*".repeat(wizard.password_confirm.chars().count()),
                    Style::default().fg(Color::White),
                )));
            }
            lines.push(Line::from(""));
            if let Some(err) = &wizard.error {
                lines.push(Line::from(Span::styled(
                    format!("❌ {}", err),
                    Style::default().fg(Color::Red),
                )));
            }
            lines.push(Line::from(Span::styled(
                "继续[Enter] 删除[Backspace/Delete] 取消[Esc]",
                Style::default().fg(Color::Gray),
            )));
        }
        WizardStep::ConfirmDelete => {
            lines.push(Line::from("步骤 3/3: 确认删除"));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "⚠️  此操作将删除加密卷并还原明文",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(format!("卷名: {}", wizard.vault_name)));
            lines.push(Line::from(""));
            lines.push(Line::from("请输入 DELETE 以确认："));
            lines.push(Line::from(Span::styled(
                wizard.delete_confirm_text.clone(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            if let Some(err) = &wizard.error {
                lines.push(Line::from(Span::styled(
                    format!("❌ {}", err),
                    Style::default().fg(Color::Red),
                )));
            }
            lines.push(Line::from(Span::styled(
                "确认[Enter] 取消[Esc]",
                Style::default().fg(Color::Gray),
            )));
        }
        WizardStep::Running => {
            lines.push(Line::from("执行中..."));
            lines.push(Line::from(""));
            if !wizard.checks.is_empty() {
                lines.push(Line::from(Span::styled(
                    "容量检查：",
                    Style::default().fg(Color::Cyan),
                )));
                for k in &["src_size", "target_free", "required"] {
                    if let Some(v) = wizard.checks.get(*k) {
                        lines.push(Line::from(format!("  {}: {}", k, v)));
                    }
                }
                lines.push(Line::from(""));
            }
            if let Some((pct, done, total)) = wizard.progress {
                lines.push(Line::from(format!("迁移进度: {}%", pct)));
                let bar_width = 40usize.min(area.width.saturating_sub(20) as usize);
                let filled = (bar_width * pct as usize) / 100;
                let bar: String =
                    "▓".repeat(filled) + &"░".repeat(bar_width.saturating_sub(filled));
                lines.push(Line::from(Span::styled(
                    bar,
                    Style::default().fg(Color::Green),
                )));
                if total > 0 {
                    lines.push(Line::from(format!("{} / {} 字节", done, total)));
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "中断[Ctrl+C]",
                Style::default().fg(Color::Gray),
            )));
        }
        WizardStep::Done => {
            lines.push(Line::from(Span::styled(
                "✅ 完成",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from("按任意键返回"));
        }
        WizardStep::Failed(err) => {
            lines.push(Line::from(Span::styled(
                "❌ 失败",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(err.as_str()));
            lines.push(Line::from(""));
            lines.push(Line::from("按任意键返回"));
        }
    }

    let para = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" 向导 "))
        .wrap(Wrap { trim: true });
    f.render_widget(para, area);
}

fn render_option(
    idx: usize,
    selected: usize,
    value: bool,
    locked: bool,
    label: &str,
) -> Line<'static> {
    let marker = if value { "[x]" } else { "[ ]" };
    let prefix = if idx == selected { "> " } else { "  " };
    let style = if locked {
        Style::default().fg(Color::DarkGray)
    } else if idx == selected {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    Line::from(Span::styled(
        format!("{}{} {}", prefix, marker, label),
        style,
    ))
}

fn render_confirm_umount(f: &mut Frame, app: &App, idx: usize, area: Rect) {
    let (name, mp) = match app.vaults.get(idx) {
        Some(v) => (v.name.clone(), v.mount_point.clone()),
        None => ("未知".to_string(), "-".to_string()),
    };
    let popup = centered_rect(60, 30, area);
    f.render_widget(Clear, popup);

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "确认卸载",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(format!("卷:    {}", name)),
        Line::from(format!("挂载点: {}", mp)),
        Line::from(""),
        Line::from(Span::styled(
            "确认[y] 取消[n/Esc]",
            Style::default().fg(Color::Gray),
        )),
    ];
    let para = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" ⚡ 操作 ")
                .style(Style::default().bg(Color::Rgb(0, 0, 100))),
        )
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(para, popup);
}

fn render_help_overlay(f: &mut Frame, area: Rect, app: &App) {
    let popup = centered_rect(85, 92, area);
    f.render_widget(Clear, popup);

    let repo_url = std::env::var("GOCRYPTFS_REPO")
        .unwrap_or_else(|_| env!("CARGO_PKG_REPOSITORY").to_string());
    let cli = cli_path();
    let data = shorten_home(&app.data_dir.to_string_lossy());
    let cfg = shorten_home(&app.config);

    let text = vec![
        Line::from(Span::styled(
            "帮助",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "── 版本信息 ──",
            Style::default().fg(Color::Magenta),
        )),
        Line::from(format!("  {} {}", APP_NAME, APP_VERSION)),
        Line::from(format!("  提交: {}", APP_COMMIT)),
        Line::from(format!("  构建: {}", APP_BUILD_TIME)),
        Line::from(format!("  仓库: {}", repo_url)),
        Line::from(""),
        Line::from(Span::styled(
            "── 路径 ──",
            Style::default().fg(Color::Magenta),
        )),
        Line::from(format!("  CLI:  {}", cli)),
        Line::from(format!("  配置: {}", cfg)),
        Line::from(format!("  数据: {}", data)),
        Line::from(""),
        Line::from(Span::styled(
            "── 启动参数 ──",
            Style::default().fg(Color::Cyan),
        )),
        Line::from("  -c, --config <PATH>   配置文件路径"),
        Line::from("  -D, --data-dir <DIR>  数据目录（日志、历史、帮助）"),
        Line::from("  -h, --help            显示帮助"),
        Line::from("  -V, --version         显示版本信息"),
        Line::from(""),
        Line::from(Span::styled(
            "── 全局键 ──",
            Style::default().fg(Color::Cyan),
        )),
        Line::from("  换页[Tab] 跳页[1/2/3] 焦点[Alt+1/2/3/4]"),
        Line::from("  设置[s] 历史[h] 编辑[e] 刷新[r] 帮助[?] 退出[q]"),
        Line::from("  中断任务[Ctrl+C] 强制退出[Ctrl+D × 3 (2 秒内)]"),
        Line::from(""),
        Line::from(Span::styled(
            "── TAB1 挂载/卸载 ──",
            Style::default().fg(Color::Cyan),
        )),
        Line::from("  移动[j/k/↑/↓] 选中[Space]"),
        Line::from("  挂载/卸载[m/Enter] 卸载[u]"),
        Line::from("  目录[l] 树状[t] 打开[o]"),
        Line::from(""),
        Line::from(Span::styled(
            "── TAB2 创建加密 ──",
            Style::default().fg(Color::Cyan),
        )),
        Line::from("  移动[j/k/↑/↓] 选中[Space]"),
        Line::from("  进入创建向导[c/Enter]"),
        Line::from("  目录[l] 树状[t]"),
        Line::from(""),
        Line::from(Span::styled(
            "── TAB3 删除加密 ──",
            Style::default().fg(Color::Cyan),
        )),
        Line::from("  移动[j/k/↑/↓] 选中[Space]"),
        Line::from("  进入删除向导[d/Enter]"),
        Line::from("  目录[l] 树状[t]"),
        Line::from(""),
        Line::from(Span::styled(
            "── 历史浮层 ──",
            Style::default().fg(Color::Cyan),
        )),
        Line::from("  移动[j/k] 过滤来源[s] 结果[r] 操作[a] 关闭[Esc]"),
        Line::from(""),
        Line::from(Span::styled("── 导出 ──", Style::default().fg(Color::Cyan))),
        Line::from(format!("  H  导出帮助到 {}", app.help_file.display())),
        Line::from(""),
        Line::from(Span::styled(
            "按 Esc / ? 关闭",
            Style::default().fg(Color::Gray),
        )),
    ];
    let para = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().bg(Color::Rgb(0, 0, 100))),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(para, popup);
}

fn render_settings_overlay(
    f: &mut Frame,
    tab: SettingsTab,
    scope_index: usize,
    app: &App,
    area: Rect,
) {
    let popup = centered_rect(80, 75, area);
    f.render_widget(Clear, popup);

    let scope_name = app
        .scope_names
        .get(scope_index)
        .map(|s| s.as_str())
        .unwrap_or("全局");
    let tab_str = match tab {
        SettingsTab::Gocryptfs => "[g]gocryptfs",
        SettingsTab::Rsync => "[r]rsync",
        SettingsTab::Filters => "[f]filters",
        SettingsTab::Perm => "[p]权限",
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled("作用域: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                scope_name,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                "   ({} / {})",
                scope_index + 1,
                app.scope_names.len().max(1)
            )),
        ]),
        Line::from(Span::styled(
            format!("分类: {}", tab_str),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(""),
    ];

    let content = std::fs::read_to_string(&app.config).unwrap_or_default();
    match tab {
        SettingsTab::Gocryptfs => {
            for k in &[
                "allow_other",
                "allow_root",
                "read_only",
                "nosuid",
                "nodev",
                "noexec",
                "nonempty",
                "kernel_cache",
                "one_file_system",
                "reverse",
            ] {
                let display = match extract_yaml_value(&content, k).as_deref() {
                    Some("true") => "[x]",
                    Some("false") => "[ ]",
                    _ => "[?]",
                };
                lines.push(Line::from(format!("{} {}", display, k)));
            }
        }
        SettingsTab::Rsync => {
            for k in &[
                "archive",
                "compress",
                "verbose",
                "human_readable",
                "progress",
                "partial",
                "delete",
                "update",
                "checksum",
            ] {
                let display = match extract_yaml_value(&content, k).as_deref() {
                    Some("true") => "[x]",
                    Some("false") => "[ ]",
                    _ => "[?]",
                };
                lines.push(Line::from(format!("{} {}", display, k)));
            }
        }
        SettingsTab::Filters => {
            lines.push(Line::from("（过滤器规则）"));
            let mut in_filters = false;
            for line in content.lines() {
                if line.trim_start().starts_with("filters:") {
                    in_filters = true;
                    continue;
                }
                if in_filters {
                    let t = line.trim_start();
                    if t.starts_with("- kind:") || t.starts_with("pattern:") {
                        lines.push(Line::from(format!("  {}", t)));
                    } else if !t.is_empty() && !line.starts_with(' ') && !line.starts_with('\t') {
                        break;
                    }
                }
            }
        }
        SettingsTab::Perm => {
            let unlock = extract_yaml_value(&content, "unlock_mode").unwrap_or("755".into());
            let lock = extract_yaml_value(&content, "lock_mode").unwrap_or("555".into());
            lines.push(Line::from(format!("unlock_mode: {}", unlock)));
            lines.push(Line::from(format!("lock_mode:   {}", lock)));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "切换作用域[Tab] 分类[g/r/f/p] 编辑[e] 关闭[Esc]",
        Style::default().fg(Color::Gray),
    )));

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" ⚙ 设置 ")
                .style(Style::default().bg(Color::Rgb(0, 0, 100))),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(para, popup);
}

fn extract_yaml_value(content: &str, key: &str) -> Option<String> {
    let pattern = format!("{}:", key);
    for line in content.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix(&pattern) {
            let v = rest.trim().trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn render_history_overlay(
    f: &mut Frame,
    entries: &[HistoryEntry],
    selected: usize,
    filter: &HistoryFilter,
    area: Rect,
) {
    let popup = centered_rect(88, 85, area);
    f.render_widget(Clear, popup);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        "📜 历史记录",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    // 过滤状态
    let src_str = filter.src.as_deref().unwrap_or("全部");
    let result_str = filter.result.as_deref().unwrap_or("全部");
    let action_str = filter.action.as_deref().unwrap_or("全部");
    lines.push(Line::from(vec![
        Span::styled("过滤: ", Style::default().fg(Color::Cyan)),
        Span::styled(
            format!("来源[{}]", src_str),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("  "),
        Span::styled(
            format!("结果[{}]", result_str),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("  "),
        Span::styled(
            format!("操作[{}]", action_str),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("    "),
        Span::styled("(s/r/a 切换)", Style::default().fg(Color::DarkGray)),
    ]));
    lines.push(Line::from(format!(
        "共 {} 条（显示最近 {} 条）",
        entries.len(),
        MAX_HISTORY_DISPLAY
    )));
    lines.push(Line::from(""));

    if entries.is_empty() {
        lines.push(Line::from("（无匹配记录）"));
    } else {
        let recent: Vec<&HistoryEntry> = entries.iter().rev().take(MAX_HISTORY_DISPLAY).collect();
        for (i, e) in recent.iter().enumerate() {
            let icon = match e.result.as_str() {
                "success" => "✅",
                "failed" => "❌",
                "started" => "🔄",
                "cancelled" => "⚠ ",
                _ => "  ",
            };
            let prefix = if i == selected { "> " } else { "  " };
            let ts = if e.ts.len() >= 19 {
                &e.ts[11..19]
            } else {
                &e.ts
            };
            let detail = if e.detail.is_empty() {
                String::new()
            } else {
                format!("  {}", e.detail)
            };
            let src_short = if e.src.is_empty() { "-" } else { &e.src };
            let target_short = truncate(&e.target, 28);
            let action_short = truncate(&e.action, 12);

            let row_style = if i == selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if e.result == "failed" {
                Style::default().fg(Color::Red)
            } else if e.result == "success" {
                Style::default().fg(Color::Green)
            } else {
                Style::default()
            };

            lines.push(Line::from(Span::styled(
                format!(
                    "{}{} {}  {:<4}  {:<12}  {:<28}  {:<10}{}",
                    prefix, icon, ts, src_short, action_short, target_short, e.result, detail
                ),
                row_style,
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "移动[j/k] 过滤来源[s] 结果[r] 操作[a] 关闭[Esc]",
        Style::default().fg(Color::Gray),
    )));

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().bg(Color::Rgb(0, 0, 100))),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(para, popup);
}

fn render_message_overlay(f: &mut Frame, msg: &str, area: Rect) {
    let popup = centered_rect(70, 40, area);
    f.render_widget(Clear, popup);
    let mut lines: Vec<Line> = vec![Line::from("")];
    for line in msg.lines() {
        lines.push(Line::from(line.to_string()));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "按任意键关闭",
        Style::default().fg(Color::Gray),
    )));

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 提示 ")
                .style(Style::default().bg(Color::Rgb(0, 0, 100))),
        )
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(para, popup);
}

fn render_password_input(f: &mut Frame, pw: &PasswordInput, area: Rect) {
    let popup = centered_rect(50, 30, area);
    f.render_widget(Clear, popup);
    let mut text = vec![
        Line::from(Span::styled(
            format!("挂载 {}", pw.vault_name),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("请输入密码："),
    ];
    text.push(Line::from(Span::styled(
        "*".repeat(pw.buffer.chars().count()),
        Style::default().fg(Color::White),
    )));
    if let Some(err) = &pw.error {
        text.push(Line::from(""));
        text.push(Line::from(Span::styled(
            format!("❌ {}", err),
            Style::default().fg(Color::Red),
        )));
    }
    text.push(Line::from(""));
    text.push(Line::from(Span::styled(
        "确认[Enter] 删除[Backspace/Delete] 取消[Esc]",
        Style::default().fg(Color::Gray),
    )));

    let para = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().bg(Color::Rgb(0, 0, 100))),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(para, popup);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let mut width = (r.width as u32 * percent_x as u32 / 100) as u16;
    let mut height = (r.height as u32 * percent_y as u32 / 100) as u16;
    if width < 30 {
        width = 30;
    }
    if height < 8 {
        height = 8;
    }
    if width > r.width {
        width = r.width;
    }
    if height > r.height {
        height = r.height;
    }
    let x = r.x + (r.width.saturating_sub(width)) / 2;
    let y = r.y + (r.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}

// ============================================================
// 主循环
// ============================================================

enum RunResult {
    Quit,
    EditConfig,
}

fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<RunResult, Box<dyn std::error::Error>> {
    loop {
        terminal.draw(|f| ui(f, app))?;
        app.poll_task();

        if app.should_quit {
            return Ok(RunResult::Quit);
        }
        if app.editor_request {
            return Ok(RunResult::EditConfig);
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                let is_ctrl_c =
                    key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');

                handle_key(app, key.code, key.modifiers);

                if is_ctrl_c {
                    std::thread::sleep(Duration::from_millis(100));
                    terminal.clear()?;
                }
            }
        }
    }
}

// ---------- 打印帮助 ----------

fn print_help(app_config: &str, data_dir: &Path, log_file: &Path, history_file: &Path) {
    let cli_env = std::env::var("GOCRYPTFS_CLI").unwrap_or_else(|_| CLI_NAME.to_string());
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());

    println!("{} {}", APP_NAME, version_string());
    println!();
    println!("USAGE:");
    println!("    {} [OPTIONS]", APP_NAME);
    println!();
    println!("OPTIONS:");
    println!("    -c, --config <PATH>    配置文件路径");
    println!("                           [当前: {}]", app_config);
    println!("    -D, --data-dir <DIR>   数据目录（日志、历史、帮助）");
    println!("                           [当前: {}]", data_dir.display());
    println!("    -h, --help             显示本帮助");
    println!("    -V, --version          显示版本信息");
    println!();
    println!("ENVIRONMENT:");
    println!(
        "    GOCRYPTFS_CLI          CLI 可执行文件路径 [当前: {}]",
        cli_env
    );
    println!("    EDITOR                 配置编辑器 [当前: {}]", editor);
    println!();
    println!("PATHS:");
    println!("    配置文件: {}", app_config);
    println!("    数据目录: {}", data_dir.display());
    println!("    日志文件: {}", log_file.display());
    println!("    历史文件: {}", history_file.display());
    println!();
    println!("更多信息见 README.md");
}

fn print_version() {
    println!("{} {}", APP_NAME, version_string());
}

// ---------- 主入口 ----------

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let outcome = cli::parse(std::env::args().collect());

    if let Some(err) = &outcome.error {
        if !matches!(outcome.action, cli::Action::Help | cli::Action::Version) {
            eprintln!("错误: {}", err);
            eprintln!("运行 '{} --help' 查看用法", APP_NAME);
            std::process::exit(2);
        }
    }

    let config = resolve_config(outcome.opts.config.clone());
    let data_dir = resolve_data_dir(outcome.opts.data_dir.clone());
    let log_file = data_dir.join("app.log.jsonl");
    let history_file = data_dir.join("history.jsonl");
    let help_file = data_dir.join("HELP.md");

    match outcome.action {
        cli::Action::Help => {
            print_help(&config, &data_dir, &log_file, &history_file);
            return Ok(());
        }
        cli::Action::Version => {
            print_version();
            return Ok(());
        }
        cli::Action::Run => {}
    }

    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        eprintln!("无法创建数据目录 {}: {}", data_dir.display(), e);
        std::process::exit(1);
    }

    // 自动迁移旧 history.jsonl → app.log.jsonl
    migrate_old_history(&data_dir);

    std::env::set_var("HISTORY_FILE", &history_file);

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(config, data_dir, log_file, history_file, help_file);

    let final_result = loop {
        match run_app(&mut terminal, &mut app) {
            Ok(RunResult::Quit) => break Ok(()),
            Ok(RunResult::EditConfig) => {
                disable_raw_mode()?;
                execute!(
                    terminal.backend_mut(),
                    LeaveAlternateScreen,
                    DisableMouseCapture
                )?;
                terminal.show_cursor()?;
                let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
                let _ = Command::new(&editor).arg(&app.config).status();
                enable_raw_mode()?;
                let mut stdout = std::io::stdout();
                execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
                let backend = CrosstermBackend::new(stdout);
                terminal = Terminal::new(backend)?;
                app.editor_request = false;
                app.reload_vaults();
                app.reload_pending();
                app.add_output("配置已编辑".to_string());
            }
            Err(e) => break Err(e),
        }
    };

    app.logger.log(
        LogKind::Operation,
        LogEntry::new("tui.quit", APP_NAME).result("success"),
    );

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    final_result
}
