#![allow(dead_code)]

#[macro_use]
mod i18n;
mod backend;
mod cli;
mod deps;
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

/// 版本串（写进内嵌后端的 VERSION 文件，供 `gocryptfs-cli --version` 使用）
fn version_line() -> String {
    version_string()
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

/// 后端路径优先级：`GOCRYPTFS_CLI` > 内嵌释放副本 > PATH 中的 `gocryptfs-cli`。
fn cli_path_from(env: Option<&str>, embedded: Option<&str>) -> String {
    if let Some(v) = env {
        if !v.trim().is_empty() {
            return v.to_string();
        }
    }
    match embedded {
        Some(p) => p.to_string(),
        None => CLI_NAME.to_string(),
    }
}

fn cli_path() -> String {
    let env = std::env::var("GOCRYPTFS_CLI").ok();
    let embedded = backend::embedded_path().map(|p| p.to_string_lossy().to_string());
    cli_path_from(env.as_deref(), embedded.as_deref())
}

/// CLI 的实际路径：便于发现「调用到了旧的已安装后端」这类问题
/// （例如 /usr/local/bin 里的旧 gocryptfs-cli 不认识 GOCRYPTFS_LANG）。
fn resolved_cli_path() -> String {
    let cli = cli_path();
    if cli.contains('/') {
        return cli;
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(&cli);
            if candidate.is_file() {
                return candidate.to_string_lossy().to_string();
            }
        }
    }
    cli
}

/// 探测 CLI 是否支持 `--lang`（新版 Shell 后端支持；旧版安装不认识，
/// 会导致英文界面下输出区仍是中文）。探测失败时返回 true，避免误报。
fn cli_supports_lang(cli: &str) -> bool {
    match backend::cli_command(cli).arg("--help").output() {
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stdout).to_string();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            text.contains("--lang")
        }
        Err(_) => true,
    }
}

/// 展开 `~` 并把相对路径转成绝对路径（不解析符号链接，保持用户可预期）。
/// 用于顶栏/帮助里展示配置与数据目录：`-c demo.yaml` 也应显示完整路径。
fn absolute_path(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let expanded = if path == "~" || path.starts_with("~/") {
        let rest = path.strip_prefix("~/").unwrap_or("");
        match std::env::var_os("HOME") {
            Some(home) => PathBuf::from(home).join(rest),
            None => PathBuf::from(path),
        }
    } else {
        PathBuf::from(path)
    };
    if expanded.is_absolute() {
        return expanded.to_string_lossy().to_string();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(expanded).to_string_lossy().to_string(),
        Err(_) => expanded.to_string_lossy().to_string(),
    }
}

fn resolve_config(cli_opt: Option<PathBuf>) -> String {
    let raw = if let Some(p) = cli_opt {
        p.to_string_lossy().to_string()
    } else if let Some(p) = backend_config_path() {
        p
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home)
            .join(".config")
            .join(APP_NAME)
            .join("config.yaml")
            .to_string_lossy()
            .to_string()
    } else if let Some(cfg) = dirs::config_dir() {
        cfg.join(APP_NAME)
            .join("config.yaml")
            .to_string_lossy()
            .to_string()
    } else {
        String::new()
    };
    absolute_path(&raw)
}

/// 问 Shell 后端要配置路径（`gocryptfs-cli config`），失败返回 None。
fn backend_config_path() -> Option<String> {
    let o = backend::cli_command(&cli_path())
        .arg("config")
        .output()
        .ok()?;
    if !o.status.success() {
        return None;
    }
    let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if p.is_empty() {
        None
    } else {
        Some(p)
    }
}

fn resolve_data_dir(cli_opt: Option<PathBuf>) -> PathBuf {
    let raw = if let Some(p) = cli_opt {
        p
    } else if let Some(d) = dirs::data_local_dir() {
        d.join(APP_NAME)
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".local/share").join(APP_NAME)
    } else {
        PathBuf::from("/tmp").join(APP_NAME)
    };
    PathBuf::from(absolute_path(&raw.to_string_lossy()))
}

fn load_vaults(config: &str) -> Result<Vec<Vault>, String> {
    if config.is_empty() {
        return Err(t!("cli.error.config_empty").to_string());
    }
    let out = backend::cli_command(&cli_path())
        .args(["-c", config, "list", "--json"])
        .output()
        .map_err(|e| t!("cli.error.exec_failed", e))?;
    if !out.status.success() {
        return Err(t!(
            "cli.error.cli_failed",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let resp: CliListResponse =
        serde_json::from_slice(&out.stdout).map_err(|e| t!("cli.error.json_parse", e))?;
    if resp.status != "ok" {
        return Err(t!("cli.error.bad_status").to_string());
    }
    Ok(resp.data.vaults)
}

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
    // 把当前界面语言传给 Shell 后端，保证 CLI 输出与 TUI 一致
    spawn_cli_task_with(
        &cli_path(),
        i18n::lang().code(),
        args,
        password,
        log_file,
        history_file,
    )
}

fn spawn_cli_task_with(
    cli: &str,
    lang: &str,
    args: Vec<String>,
    password: Option<String>,
    log_file: &Path,
    history_file: &Path,
) -> Receiver<CliEvent> {
    let (tx, rx) = mpsc::channel();
    let cli = cli.to_string();
    let lang = lang.to_string();
    let log = log_file.to_path_buf();
    let hist = history_file.to_path_buf();
    thread::spawn(move || {
        let mut cmd = backend::cli_command(&cli);
        cmd.args(&args)
            .env("GOCRYPTFS_LANG", lang)
            .env("LOG_FILE", &log)
            .env("HISTORY_FILE", &hist)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(CliEvent::Stderr(t!("cli.error.spawn_failed", e)));
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
        .map_err(|e| t!("cli.error.run_failed", cmd, e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            t!("cli.error.exit_code", out.status.code().unwrap_or(1))
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
    kind: TaskKind,
    action: String,
    status: TaskStatus,
    pid: Option<u32>,
    rx: Receiver<CliEvent>,
    started_at: Instant,
}

/// 任务类型（携带卷名）：渲染时再本地化，语言切换后不会残留旧语言。
#[derive(Debug, Clone)]
enum TaskKind {
    Mount(String),
    Umount(String),
    Create(String),
    Remove(String),
}

impl TaskKind {
    /// CLI 动作名（写入日志，保持稳定不翻译）。
    fn action(&self) -> &'static str {
        match self {
            TaskKind::Mount(_) => "mount",
            TaskKind::Umount(_) => "umount",
            TaskKind::Create(_) => "create",
            TaskKind::Remove(_) => "remove",
        }
    }

    /// 当前语言下的任务描述。
    fn label(&self) -> String {
        match self {
            TaskKind::Mount(name) => t!("task.mount", name),
            TaskKind::Umount(name) => t!("task.umount", name),
            TaskKind::Create(name) => t!("task.create", name),
            TaskKind::Remove(name) => t!("task.remove", name),
        }
    }
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
    /// 状态文字是否用灰色（信息性提示，如"已挂载/未挂载"）
    status_dim: bool,
    /// 最近一次加载卷列表失败的原因（列表区会直接显示，避免"空列表无解释"）
    load_error: Option<String>,
    config: String,
    data_dir: PathBuf,
    log_file: PathBuf,
    history_file: PathBuf,
    help_file: PathBuf,
    logger: Logger,
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
            status: t!("status.ready").to_string(),
            status_dim: false,
            load_error: None,
            config: config.clone(),
            data_dir,
            log_file,
            history_file,
            help_file,
            logger,
            password_input: None,
            task: None,
            should_quit: false,
            editor_request: false,
            scope_names: vec![t!("common.global").to_string()],
            history_entries: Vec::new(),
            history_selected: 0,
            history_filter: HistoryFilter::default(),
            last_ctrl_d: None,
            ctrl_d_count: 0,
        };
        app.add_output(t!("output.started", APP_NAME, short_version()));
        // 打印解析后的实际路径：便于发现调用到了旧的已安装后端
        app.add_output(format!("{}{}", t!("common.cli"), resolved_cli_path()));
        // 内嵌后端释放失败：给出可操作提示（此时会回退到 PATH 中的后端）
        if let Some(err) = backend::error() {
            app.add_output(t!(
                "output.backend_extract_failed",
                backend::extract_dir(&app.data_dir).display().to_string(),
                err
            ));
        }
        // 运行时依赖缺失（或 yq 不是 Go v4）时给出安装提示
        let missing_deps = deps::missing();
        let yq_bad = deps::yq_flavor_ok();
        if deps::has_problems(&missing_deps, yq_bad) {
            for line in deps::report_lines(&missing_deps, yq_bad) {
                app.add_output(line);
            }
        }
        // 英文界面 + 旧版后端（不认识 --lang）时提示一次，否则输出区会残留中文
        if i18n::lang() != i18n::DEFAULT_LANG && !cli_supports_lang(&cli_path()) {
            app.add_output(t!("output.cli_lang_unsupported").to_string());
        }
        app.add_output(t!(
            "output.config",
            if app.config.is_empty() {
                t!("common.not_found")
            } else {
                &app.config
            }
        ));

        if !app.config.is_empty() {
            match load_vaults(&config) {
                Ok(vaults) => {
                    let n = vaults.len();
                    app.vaults = vaults;
                    app.load_error = None;
                    app.add_output(t!("output.loaded_vaults", n));
                    if n > 0 {
                        app.vault_list_state.select(Some(0));
                    }
                }
                Err(e) => {
                    app.load_error = Some(e.clone());
                    app.add_output(t!("output.load_failed", e));
                }
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
        let mut names = vec![t!("common.global").to_string()];
        for v in &self.vaults {
            names.push(v.name.clone());
        }
        self.scope_names = names;
    }

    /// 运行期切换界面语言（仅当前会话有效；持久化请写配置 `language:`）。
    fn switch_lang(&mut self) {
        let lang = i18n::toggle_lang();
        // 作用域首个条目是本地化文案，需要重建
        self.update_scope_names();
        // 向导/密码框的错误提示是即时文案，切换后清掉避免残留旧语言
        if let Some(w) = &mut self.wizard {
            w.error = None;
        }
        if let Some(p) = &mut self.password_input {
            p.error = None;
        }
        // 目录区内容（section 标题与命令输出）在加载时已本地化，按原模式重新加载
        if !self.dir_view.sections.is_empty() {
            let use_tree = self.dir_view.mode == "tree";
            let focus = self.focus;
            // 先清空：即使重载条件已不满足（如选中项被移除），也不会残留旧语言内容
            self.dir_view = DirView::default();
            self.load_dir_view(use_tree);
            self.focus = focus;
        }
        let msg = t!("status.lang_switched", lang.display_name());
        self.status = msg.clone();
        self.add_output(msg);
    }

    fn reload_vaults(&mut self) {
        if self.config.is_empty() {
            return;
        }
        match load_vaults(&self.config) {
            Ok(vaults) => {
                self.load_error = None;
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
            Err(e) => {
                self.load_error = Some(e.clone());
                self.add_output(t!("output.refresh_failed", e));
            }
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
                self.status = t!("status.no_vault").to_string();
                return;
            }
        };
        if let Some(v) = self.selected_vault() {
            if v.mounted {
                self.set_dim_status(t!("status.mounted", name));
                return;
            }
        }
        self.password_input = Some(PasswordInput {
            vault_name: name,
            buffer: String::new(),
            error: None,
        });
        self.status = t!("status.enter_password_hint").to_string();
    }

    fn trigger_umount_or_confirm(&mut self) {
        let idx = match self.vault_list_state.selected() {
            Some(i) => i,
            None => {
                self.status = t!("status.no_vault").to_string();
                return;
            }
        };
        let vault = match self.vaults.get(idx) {
            Some(v) => v.clone(),
            None => return,
        };
        if !vault.valid {
            self.overlay = Some(Overlay::Message(t!(
                "error.invalid_vault_umount",
                vault.name,
                vault.path
            )));
            return;
        }
        if !vault.mounted {
            self.set_dim_status(t!("status.not_mounted", vault.name));
            return;
        }
        self.overlay = Some(Overlay::ConfirmUmount { vault_index: idx });
    }

    /// 设置灰色（信息性）状态提示
    fn set_dim_status(&mut self, msg: String) {
        self.status = msg;
        self.status_dim = true;
    }

    /// `m`：挂载选中卷。已挂载时只给灰色提示，不再切换成卸载（toggle 已取消）。
    fn mount_action(&mut self) {
        let idx = match self.vault_list_state.selected() {
            Some(i) => i,
            None => {
                self.status = t!("status.no_vault").to_string();
                return;
            }
        };
        if self.vault_confirmed != Some(idx) {
            self.status = t!("status.select_first").to_string();
            return;
        }
        let vault = match self.vaults.get(idx) {
            Some(v) => v.clone(),
            None => return,
        };
        if !vault.valid {
            self.overlay = Some(Overlay::Message(t!(
                "error.invalid_vault_create",
                vault.name,
                vault.path
            )));
            return;
        }
        if vault.mounted {
            self.set_dim_status(t!("status.mounted", vault.name));
            return;
        }
        self.start_mount();
    }

    fn enter_create_wizard(&mut self) {
        let idx = match self.pending_list_state.selected() {
            Some(i) => i,
            None => {
                self.status = t!("status.no_pending").to_string();
                return;
            }
        };
        if self.pending_confirmed != Some(idx) {
            self.status = t!("status.select_first").to_string();
            return;
        }
        let source = self.pending[idx].clone();
        if let Some(v) = self.pending_created_vault(&source) {
            let name = v.name.clone();
            self.overlay = Some(Overlay::Message(t!("error.pending_occupied", name)));
            return;
        }
        self.start_create_wizard();
    }

    fn enter_remove_wizard(&mut self) {
        let idx = match self.vault_list_state.selected() {
            Some(i) => i,
            None => {
                self.status = t!("status.no_vault").to_string();
                return;
            }
        };
        if self.vault_confirmed != Some(idx) {
            self.status = t!("status.select_first").to_string();
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
        self.start_task(TaskKind::Umount(name), args, None, log, hist);
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
                        self.status = t!("status.no_vault").to_string();
                        return;
                    }
                };
                let mut sections = vec![self.make_section(
                    &t!("dir.cipher_path", vault.path),
                    &vault.path,
                    cmd,
                    &extra_args,
                    false,
                )];
                sections.push(self.make_section(
                    &t!("dir.mount_point", vault.mount_point),
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
                self.status = t!("status.dir_loaded", vault.name, mode);
            }
            Page::Create => {
                let idx = match self.pending_list_state.selected() {
                    Some(i) => i,
                    None => {
                        self.status = t!("status.no_pending").to_string();
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

                let mut sections = vec![self.make_section(
                    &t!("dir.cipher_dir", cipher),
                    &cipher,
                    cmd,
                    &extra_args,
                    false,
                )];
                sections.push(self.make_section(
                    &t!("dir.tmp_mount", tmp_mount),
                    &tmp_mount,
                    cmd,
                    &extra_args,
                    false,
                ));
                sections.push(self.make_section(
                    &t!("dir.source_mount", source),
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
                self.status = t!("status.dir_loaded", name, mode);
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
            lines.push(t!("dir.path_missing").to_string());
        } else if placeholder_missing {
            lines.push(t!("dir.not_mounted").to_string());
        } else {
            let mut args: Vec<&str> = extra_args.to_vec();
            args.push(path);
            match sys_run(cmd, &args) {
                Ok(text) => {
                    for l in text.lines() {
                        lines.push(l.to_string());
                    }
                    if lines.is_empty() {
                        lines.push(t!("dir.empty").to_string());
                    }
                }
                Err(e) => {
                    lines.push(t!("dir.read_failed", e));
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
        kind: TaskKind,
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
                self.status = t!("status.task_busy").to_string();
                return;
            }
        }
        let action = kind.action();
        let desc = kind.label();
        self.add_output(t!("output.exec", cli_path(), args.join(" ")));
        self.status = t!("status.running", &desc);

        self.logger.log(
            LogKind::Operation,
            LogEntry::new(action, &desc)
                .result("started")
                .detail(args.join(" ")),
        );

        let rx = spawn_cli_task(args, password, &log_file, &history_file);
        self.task = Some(BackgroundTask {
            kind,
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
        if !events.is_empty() {
            self.status_dim = false;
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
                    t.kind.label(),
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
                self.status = t!("status.done", desc);
                self.add_output(format!("[OK] {}", desc));
                if let Some(w) = &mut self.wizard {
                    w.step = WizardStep::Done;
                }
            } else {
                self.status = t!("status.failed_code", code, desc);
                self.add_output(format!("[FAIL:{}] {}", code, desc));
                if let Some(w) = &mut self.wizard {
                    w.step = WizardStep::Failed(t!("wizard.failed_cli_code", code));
                }
                if code == 2 {
                    self.overlay = Some(Overlay::Message(t!("error.wrong_password").to_string()));
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
        let info = self
            .task
            .as_ref()
            .and_then(|t| t.pid.map(|pid| (pid, t.kind.label(), t.action.clone())));

        if let Some((pid, desc, action)) = info {
            kill_pid_tree(pid);
            std::thread::sleep(Duration::from_millis(300));
            self.add_output(t!("output.task_interrupted", desc, pid));
            self.status = t!("status.interrupted").to_string();
            self.logger.log(
                LogKind::Operation,
                LogEntry::new(action, desc).result("cancelled").pid(pid),
            );
        } else if self.task.is_some() {
            self.status = t!("status.task_no_pid").to_string();
        }

        self.task = None;
        if let Some(w) = &mut self.wizard {
            w.step = WizardStep::Failed(t!("wizard.user_abort").to_string());
        }
    }

    fn start_create_wizard(&mut self) {
        if self.pending.is_empty() {
            self.status = t!("status.pending_empty").to_string();
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
        self.status = t!("wizard.step1_status").to_string();
    }

    fn start_remove_wizard(&mut self) {
        let vault = match self.selected_vault() {
            Some(v) => v.clone(),
            None => {
                self.status = t!("status.no_vault").to_string();
                return;
            }
        };
        if !vault.valid {
            self.overlay = Some(Overlay::Message(t!(
                "error.invalid_vault_remove",
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
        self.status = t!("wizard.step1_status").to_string();
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
                self.status = t!("wizard.step2_status").to_string();
            }
            WizardStep::EnterPassword => {
                if wizard.password.is_empty() {
                    wizard.error = Some(t!("wizard.password_empty").to_string());
                    self.wizard = Some(wizard);
                    return;
                }
                if wizard.direction == WizardDirection::Remove {
                    wizard.step = WizardStep::ConfirmDelete;
                    wizard.delete_confirm_text.clear();
                    wizard.error = None;
                    self.wizard = Some(wizard);
                    self.status = t!("wizard.step3_status").to_string();
                    return;
                }
                if wizard.password != wizard.password_confirm {
                    wizard.error = Some(t!("wizard.password_mismatch").to_string());
                    wizard.confirming = false;
                    wizard.password_confirm.clear();
                    self.wizard = Some(wizard);
                    return;
                }
                self.wizard_execute(wizard);
            }
            WizardStep::ConfirmDelete => {
                if wizard.delete_confirm_text != "DELETE" {
                    wizard.error = Some(t!("wizard.delete_confirm_required").to_string());
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
                self.status = t!("status.ready").to_string();
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

        let (args, kind) = match wizard.direction {
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
                (a, TaskKind::Create(wizard.name.clone()))
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
                (a, TaskKind::Remove(wizard.vault_name.clone()))
            }
        };

        self.wizard = Some(wizard);
        let log = self.log_file.clone();
        let hist = self.history_file.clone();
        self.start_task(kind, args, Some(password), log, hist);
        self.status = t!("status.executing").to_string();
    }

    fn wizard_cancel(&mut self) {
        self.wizard = None;
        self.status = t!("status.cancelled").to_string();
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
            self.status = t!("status.force_quit_hint", 3 - self.ctrl_d_count);
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

        // 导出内容随当前界面语言本地化，占位符为具名形式
        tn!(
            "help.export_markdown",
            "app" => APP_NAME,
            "version" => APP_VERSION,
            "commit" => APP_COMMIT,
            "build" => APP_BUILD_TIME,
            "repo" => repo,
            "cli" => cli_path(),
            "cfg" => self.config.clone(),
            "data" => self.data_dir.display().to_string(),
            "log" => self.log_file.display().to_string(),
            "hist" => self.history_file.display().to_string(),
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
    // 任何新按键都结束上一条灰色信息提示
    app.status_dim = false;
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
                app.status = t!("status.cancelled").to_string();
            }
            KeyCode::Enter => {
                let vault_name = pw.vault_name.clone();
                let password = pw.buffer.clone();
                if password.is_empty() {
                    pw.error = Some(t!("wizard.password_empty").to_string());
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
                app.start_task(TaskKind::Mount(vault_name), args, Some(password), log, hist);
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
                                    w.error = Some(t!("wizard.password_empty").to_string());
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
                    app.status = t!("status.running_ctrl_c").to_string();
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
                            app.status = t!("status.exported", p.display());
                            app.overlay =
                                Some(Overlay::Message(t!("overlay.help_exported", p.display())));
                        }
                        Err(e) => {
                            app.overlay = Some(Overlay::Message(t!("overlay.export_failed", e)));
                        }
                    }
                } else if key == KeyCode::Char('L') {
                    // 帮助浮层内也允许切换语言（帮助文案里已标注 L 键）
                    app.switch_lang();
                    app.overlay = Some(Overlay::Help);
                } else if !matches!(key, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')) {
                    app.overlay = Some(Overlay::Help);
                }
            }
            Overlay::Message(_) => {}
            Overlay::ConfirmUmount { vault_index } => match key {
                KeyCode::Char('y') | KeyCode::Char('Y') => app.start_umount_confirmed(vault_index),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    app.status = t!("status.umount_cancelled").to_string();
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
                KeyCode::Char('l') | KeyCode::Char('L') => {
                    // 运行期切换语言，浮层保持打开以便立即看到效果
                    app.switch_lang();
                    app.overlay = Some(Overlay::Settings { tab, scope_index });
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
                KeyCode::Char('L') => {
                    // 历史浮层内也可切换语言，浮层保持打开
                    app.switch_lang();
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
            app.status = t!("status.refreshed").to_string();
            return;
        }
        KeyCode::Char('L') => {
            // 全局切换界面语言（zh-CN <-> en-US）
            app.switch_lang();
            return;
        }
        KeyCode::Tab => {
            // 轮转换区（正向）
            app.focus = match app.focus {
                Focus::List => Focus::Detail,
                Focus::Detail => Focus::Dir,
                Focus::Dir => Focus::Output,
                Focus::Output => Focus::List,
            };
            return;
        }
        KeyCode::BackTab => {
            // 轮转换区（反向）
            app.focus = match app.focus {
                Focus::List => Focus::Output,
                Focus::Output => Focus::Dir,
                Focus::Dir => Focus::Detail,
                Focus::Detail => Focus::List,
            };
            return;
        }
        KeyCode::Char(']') => {
            // 顺序换页（下一页）
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
                Page::Mount => t!("page.mount").to_string(),
                Page::Create => t!("page.create").to_string(),
                Page::Remove => t!("page.remove").to_string(),
            };
            return;
        }
        KeyCode::Char('[') => {
            // 顺序换页（上一页）
            app.page = match app.page {
                Page::Mount => Page::Remove,
                Page::Create => Page::Mount,
                Page::Remove => Page::Create,
            };
            app.focus = Focus::List;
            app.vault_confirmed = None;
            app.pending_confirmed = None;
            app.dir_view = DirView::default();
            app.status = match app.page {
                Page::Mount => t!("page.mount").to_string(),
                Page::Create => t!("page.create").to_string(),
                Page::Remove => t!("page.remove").to_string(),
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
                        app.status = t!("status.selected_mount").to_string();
                    }
                }
                // TAB1：m 挂载、u 卸载；Enter 不再参与挂载/卸载
                KeyCode::Char('m') => app.mount_action(),
                KeyCode::Char('u') => app.trigger_umount_or_confirm(),
                KeyCode::Char('l') => app.load_dir_view(false),
                KeyCode::Char('t') => app.load_dir_view(true),
                _ => {}
            },
            Page::Create => match key {
                KeyCode::Char('j') | KeyCode::Down => app.next_pending(),
                KeyCode::Char('k') | KeyCode::Up => app.previous_pending(),
                KeyCode::Char(' ') => {
                    if let Some(i) = app.pending_list_state.selected() {
                        app.pending_confirmed = Some(i);
                        app.status = t!("status.selected_create").to_string();
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
                        app.status = t!("status.selected_remove").to_string();
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
                app.status = t!("status.dir_cleared").to_string();
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
                app.status = t!("status.output_cleared").to_string();
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
            Constraint::Length(2), // 顶栏：Bin+CLI+版本 / 配置+数据
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

// ------------------------------------------------------------
// 标签对齐
//
// 翻译表里的标签**不带**对齐空格；列宽在渲染时按显示宽度（CJK 记 2 列）
// 对同一组标签取最大值 + 1 个分隔空格，这样中/英文都能对齐，
// 也不会因为某个译文变长而错列。
// ------------------------------------------------------------

/// 同组标签的列宽：当前语言下最大显示宽度 + 1（分隔空格）。
fn label_column(keys: &[&'static str]) -> usize {
    keys.iter()
        .map(|k| Line::from(t!(*k)).width())
        .max()
        .unwrap_or(0)
        + 1
}

/// 按显示宽度把标签补齐到 `width` 列；标签本身超宽时只补一个空格。
fn pad_label(label: &str, width: usize) -> String {
    let w = Line::from(label).width();
    if w >= width {
        format!("{label} ")
    } else {
        format!("{label}{}", " ".repeat(width - w))
    }
}

/// 按**显示宽度**截断（CJK 记 2 列），保留尾部并加前导 `…`。
/// 保留尾部对路径更友好（能看到文件名/参数）。
fn truncate(s: &str, max: usize) -> String {
    if Line::from(s).width() <= max {
        return s.to_string();
    }
    if max < 2 {
        return "…".to_string();
    }
    let mut taken: Vec<char> = Vec::new();
    let mut used = 0usize;
    for ch in s.chars().rev() {
        let cw = Line::from(ch.to_string()).width();
        if used + cw > max - 1 {
            break;
        }
        used += cw;
        taken.push(ch);
    }
    taken.reverse();
    format!("…{}", taken.into_iter().collect::<String>())
}

/// 当前运行的可执行文件路径（真实绝对路径），用于顶栏 `Bin:`。
fn bin_path() -> String {
    if let Ok(p) = std::env::current_exe() {
        return p.to_string_lossy().to_string();
    }
    match std::env::args().next() {
        Some(a) if !a.is_empty() => absolute_path(&a),
        _ => "?".to_string(),
    }
}

/// 在 `avail` 列内按自然长度比例给两个字符串分配预算，各自保留尾部截断。
fn fit_two(a: &str, b: &str, avail: usize) -> (String, String) {
    let (aw, bw) = (Line::from(a).width(), Line::from(b).width());
    if aw + bw <= avail {
        return (a.to_string(), b.to_string());
    }
    if avail == 0 {
        return (String::new(), String::new());
    }
    if aw == 0 {
        return (String::new(), truncate(b, avail));
    }
    if bw == 0 {
        return (truncate(a, avail), String::new());
    }
    let min = if avail >= 16 { 8 } else { 1 };
    let total = aw + bw;
    let mut a_max = (avail * aw) / total;
    a_max = a_max.max(min).min(avail - min.min(avail));
    (truncate(a, a_max), truncate(b, avail - a_max))
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
    let cyan = Style::default().fg(Color::Cyan);
    let sep_w = 2; // 两个字段之间的分隔空格

    // 第 1 行：Bin（当前可执行文件）+ CLI（后端），版本号右对齐
    let bin_label = t!("topbar.bin");
    let cli_label = t!("common.cli");
    let ver = short_version();
    let (bin_label_w, cli_label_w) = (Line::from(bin_label).width(), Line::from(cli_label).width());
    let ver_w = Line::from(ver.as_str()).width();
    let avail1 = w.saturating_sub(bin_label_w + cli_label_w + sep_w + ver_w + 1);
    let (bin, cli) = fit_two(&bin_path(), &resolved_cli_path(), avail1);
    let used1 = bin_label_w
        + Line::from(bin.as_str()).width()
        + sep_w
        + cli_label_w
        + Line::from(cli.as_str()).width()
        + ver_w;
    let line1 = Line::from(vec![
        Span::styled(bin_label, cyan),
        Span::raw(bin),
        Span::raw("  "),
        Span::styled(cli_label, cyan),
        Span::raw(cli),
        Span::raw(" ".repeat(w.saturating_sub(used1))),
        Span::styled(
            ver,
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    // 第 2 行：配置 + 数据目录（完整路径；不再显示日志/历史文件）
    let cfg_label = t!("topbar.config");
    let data_label = t!("topbar.data");
    let (cfg_label_w, data_label_w) = (
        Line::from(cfg_label).width(),
        Line::from(data_label).width(),
    );
    let avail2 = w.saturating_sub(cfg_label_w + data_label_w + sep_w);
    let data_full = app.data_dir.to_string_lossy().to_string();
    let (cfg, data) = fit_two(&app.config, &data_full, avail2);
    let line2 = Line::from(vec![
        Span::styled(cfg_label, cyan),
        Span::raw(cfg),
        Span::raw("  "),
        Span::styled(data_label, cyan),
        Span::raw(data),
    ]);

    let para = Paragraph::new(vec![line1, line2])
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
        Span::styled(t!("tab.mount"), tab_style(Page::Mount)),
        Span::raw("  "),
        Span::styled(t!("tab.create"), tab_style(Page::Create)),
        Span::raw("  "),
        Span::styled(t!("tab.remove"), tab_style(Page::Remove)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn render_list(f: &mut Frame, app: &mut App, area: Rect) {
    let (items, title): (Vec<ListItem>, String) = match app.page {
        Page::Mount | Page::Remove => {
            let items: Vec<ListItem> = if app.vaults.is_empty() {
                // 空列表必须能看出原因：加载失败 / 配置为空 / 配置里没有卷
                let mut empty: Vec<ListItem> = Vec::new();
                if let Some(err) = &app.load_error {
                    empty.push(ListItem::new(Span::styled(
                        t!("list.load_failed", err),
                        Style::default().fg(Color::Red),
                    )));
                    empty.push(ListItem::new(Span::styled(
                        t!("list.load_failed_hint"),
                        Style::default().fg(Color::Gray),
                    )));
                } else if app.config.is_empty() {
                    empty.push(ListItem::new(Span::styled(
                        t!("list.no_vault"),
                        Style::default().fg(Color::Gray),
                    )));
                } else {
                    empty.push(ListItem::new(Span::styled(
                        t!("list.empty_config", &app.config),
                        Style::default().fg(Color::Yellow),
                    )));
                    empty.push(ListItem::new(Span::styled(
                        t!("list.empty_hint"),
                        Style::default().fg(Color::Gray),
                    )));
                }
                empty
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
                            t!("list.not_encrypted").to_string()
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
                t!("list.title")
            } else {
                t!("list.title_cipher")
            };
            (items, title.to_string())
        }
        Page::Create => {
            let items: Vec<ListItem> = if app.pending.is_empty() {
                vec![ListItem::new(Span::styled(
                    t!("list.no_pending"),
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
                        let tail = if created { t!("list.created_mark") } else { "" };
                        ListItem::new(Line::from(vec![
                            Span::styled(prefix, Style::default().fg(Color::Yellow)),
                            Span::styled(p.clone(), name_style),
                            Span::styled(tail.to_string(), Style::default().fg(Color::Green)),
                        ]))
                    })
                    .collect()
            };
            (items, t!("list.pending_title").to_string())
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
                let lw = label_column(&[
                    "detail.label_id",
                    "detail.label_name",
                    "detail.label_cipher_path",
                    "detail.label_mount_point",
                    "detail.label_status",
                    "detail.label_protection",
                    "detail.label_valid",
                ]);
                let mounted_str = if v.mounted {
                    t!("detail.mounted")
                } else {
                    t!("detail.unmounted")
                };
                let mounted_color = if v.mounted { Color::Green } else { Color::Red };
                let lock_str = if v.mounted {
                    t!("detail.mounted_kernel")
                } else if v.locked {
                    t!("detail.locked_ro")
                } else {
                    t!("detail.unlocked")
                };
                let valid_line = if v.valid {
                    Line::from(vec![
                        Span::styled(
                            pad_label(t!("detail.label_valid"), lw),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::styled(t!("detail.valid"), Style::default().fg(Color::Green)),
                    ])
                } else {
                    Line::from(vec![
                        Span::styled(
                            pad_label(t!("detail.label_valid"), lw),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::styled(
                            t!("detail.invalid"),
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ),
                    ])
                };
                vec![
                    Line::from(vec![
                        Span::styled(
                            pad_label(t!("detail.label_id"), lw),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::raw(v.id.to_string()),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            pad_label(t!("detail.label_name"), lw),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::raw(v.name.clone()),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            pad_label(t!("detail.label_cipher_path"), lw),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::raw(v.path.clone()),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            pad_label(t!("detail.label_mount_point"), lw),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::raw(v.mount_point.clone()),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            pad_label(t!("detail.label_status"), lw),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::styled(mounted_str, Style::default().fg(mounted_color)),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            pad_label(t!("detail.label_protection"), lw),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::raw(lock_str),
                    ]),
                    valid_line,
                ]
            }
            None => vec![Line::from(Span::styled(
                t!("list.no_vault"),
                Style::default().fg(Color::Gray),
            ))],
        },
        Page::Create => {
            let idx = app.pending_list_state.selected();
            match idx.and_then(|i| app.pending.get(i).cloned()) {
                Some(source) => {
                    let lw = label_column(&[
                        "detail.label_task_name",
                        "detail.label_source",
                        "detail.label_cipher_dir",
                        "detail.label_tmp_mount",
                        "detail.label_target_mount",
                        "detail.label_status",
                    ]);
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
                            Span::styled(
                                pad_label(t!("detail.label_status"), lw),
                                Style::default().fg(Color::Cyan),
                            ),
                            Span::styled(
                                t!("detail.pending_created", v.name),
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ])
                    } else {
                        Line::from(vec![
                            Span::styled(
                                pad_label(t!("detail.label_status"), lw),
                                Style::default().fg(Color::Cyan),
                            ),
                            Span::styled(
                                t!("detail.pending_waiting"),
                                Style::default().fg(Color::Yellow),
                            ),
                        ])
                    };

                    vec![
                        Line::from(vec![
                            Span::styled(
                                pad_label(t!("detail.label_task_name"), lw),
                                Style::default().fg(Color::Cyan),
                            ),
                            Span::raw(name.clone()),
                        ]),
                        Line::from(vec![
                            Span::styled(
                                pad_label(t!("detail.label_source"), lw),
                                Style::default().fg(Color::Cyan),
                            ),
                            Span::raw(source.clone()),
                        ]),
                        Line::from(vec![
                            Span::styled(
                                pad_label(t!("detail.label_cipher_dir"), lw),
                                Style::default().fg(Color::Cyan),
                            ),
                            Span::raw(cipher.clone()),
                        ]),
                        Line::from(vec![
                            Span::styled(
                                pad_label(t!("detail.label_tmp_mount"), lw),
                                Style::default().fg(Color::Cyan),
                            ),
                            Span::raw(tmp_mount.clone()),
                        ]),
                        Line::from(vec![
                            Span::styled(
                                pad_label(t!("detail.label_target_mount"), lw),
                                Style::default().fg(Color::Cyan),
                            ),
                            Span::raw(source.clone()),
                        ]),
                        status_line,
                    ]
                }
                None => vec![Line::from(Span::styled(
                    t!("list.no_pending"),
                    Style::default().fg(Color::Gray),
                ))],
            }
        }
    };

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(t!("detail.title"))
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
        t!("dir.title").to_string()
    } else {
        t!("dir.title_mode", app.dir_view.mode)
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
                .title(t!("output.title"))
                .border_style(focus_border(Focus::Output, app.focus)),
        )
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .wrap(Wrap { trim: false })
        .scroll((app.output_scroll.v, app.output_scroll.h));
    f.render_widget(para, area);
}

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    // 全局键提示 + 语言切换提示（En/中文[L]，当前语言加粗）
    let active_zh = i18n::lang() == i18n::Lang::ZhCn;
    let active_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let idle_style = Style::default().fg(Color::Gray);
    // 语言提示放在最前面：全局键提示较长，放在行尾在窄终端会被裁掉
    let global_line = Line::from(vec![
        Span::styled(t!("statusbar.global"), Style::default().fg(Color::Yellow)),
        Span::styled(
            i18n::Lang::EnUs.short_name(),
            if active_zh { idle_style } else { active_style },
        ),
        Span::styled("/", idle_style),
        Span::styled(
            i18n::Lang::ZhCn.short_name(),
            if active_zh { active_style } else { idle_style },
        ),
        Span::styled("[L]  ", Style::default().fg(Color::Cyan)),
        Span::raw(t!("statusbar.global_hint")),
    ]);

    // 页面行：`Page: [n]` 用黄色（与全局行标签一致），动作提示保持白色
    let page_actions = match app.page {
        Page::Mount => t!("statusbar.page_mount"),
        Page::Create => t!("statusbar.page_create"),
        Page::Remove => t!("statusbar.page_remove"),
    };
    let page_no: usize = match app.page {
        Page::Mount => 1,
        Page::Create => 2,
        Page::Remove => 3,
    };
    let page_line = Line::from(vec![
        Span::styled(
            t!("statusbar.page_label"),
            Style::default().fg(Color::Yellow),
        ),
        Span::styled(
            t!("statusbar.page_index", page_no),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw(page_actions),
    ]);

    let focus_str = match app.focus {
        Focus::List => t!("focus.list"),
        Focus::Detail => t!("focus.detail"),
        Focus::Dir => t!("focus.dir"),
        Focus::Output => t!("focus.output"),
    };
    let area_hint = match app.focus {
        Focus::List => t!("focus.list_hint").to_string(),
        Focus::Detail => t!("focus.scroll_hint").to_string(),
        Focus::Dir => t!("focus.dir_hint").to_string(),
        Focus::Output => t!("focus.output_hint").to_string(),
    };
    let task_info = if let Some(t) = &app.task {
        match t.status {
            TaskStatus::Running => t!("status.running", t.kind.label()),
            TaskStatus::Done => t!("status.done", t.kind.label()),
            TaskStatus::Failed(_) => t!("status.failed", t.kind.label()),
        }
    } else {
        t!("common.none").to_string()
    };
    let region_line = Line::from(vec![
        Span::styled(
            t!("statusbar.region", focus_str),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw(area_hint),
        Span::raw("    "),
        Span::styled(t!("statusbar.status"), Style::default().fg(Color::Cyan)),
        Span::styled(
            app.status.clone(),
            if app.status_dim {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            },
        ),
        Span::styled(t!("statusbar.task"), Style::default().fg(Color::Cyan)),
        Span::raw(task_info),
    ]);

    // 底栏不设背景色，与顶部栏及其他区域保持一致
    let para = Paragraph::new(vec![global_line, page_line, region_line])
        .style(Style::default().fg(Color::White));
    f.render_widget(para, area);
}

fn render_wizard(f: &mut Frame, app: &App, area: Rect) {
    let wizard = match &app.wizard {
        Some(w) => w,
        None => return,
    };
    let mut lines: Vec<Line> = Vec::new();

    let title = match wizard.direction {
        WizardDirection::Create => t!("wizard.title_create"),
        WizardDirection::Remove => t!("wizard.title_remove"),
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
            lines.push(Line::from(t!("wizard.step1")));
            lines.push(Line::from(""));
            match wizard.direction {
                WizardDirection::Create => {
                    let lw = label_column(&[
                        "wizard.label_source",
                        "wizard.label_cipher_dir",
                        "wizard.label_tmp_mount",
                    ]);
                    lines.push(Line::from(vec![
                        Span::styled(
                            pad_label(t!("wizard.label_source"), lw),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::raw(&wizard.source),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled(
                            pad_label(t!("wizard.label_cipher_dir"), lw),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::raw(&wizard.cipher),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled(
                            pad_label(t!("wizard.label_tmp_mount"), lw),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::raw(&wizard.tmp_mount),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(render_option(
                        0,
                        wizard.selected,
                        wizard.dry_run,
                        false,
                        t!("wizard.preview_mode"),
                    ));
                    let (keep_text, keep_locked) = if wizard.keep_source_locked {
                        (t!("wizard.keep_source_locked"), true)
                    } else {
                        (t!("wizard.keep_source"), false)
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
                    let lw = label_column(&[
                        "wizard.label_vault",
                        "wizard.label_cipher_dir",
                        "wizard.label_mount_point",
                    ]);
                    lines.push(Line::from(vec![
                        Span::styled(
                            pad_label(t!("wizard.label_vault"), lw),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::raw(&wizard.vault_name),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled(
                            pad_label(t!("wizard.label_cipher_dir"), lw),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::raw(&wizard.cipher),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled(
                            pad_label(t!("wizard.label_mount_point"), lw),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::raw(&wizard.source),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(render_option(
                        0,
                        wizard.selected,
                        wizard.dry_run,
                        false,
                        t!("wizard.preview_mode"),
                    ));
                    let (restore_text, restore_locked) = if wizard.restore_locked {
                        (t!("wizard.restore_locked"), true)
                    } else {
                        (t!("wizard.restore"), false)
                    };
                    lines.push(render_option(
                        1,
                        wizard.selected,
                        wizard.restore,
                        restore_locked,
                        restore_text,
                    ));
                    let (del_text, del_locked) = if wizard.delete_cipher_locked {
                        (t!("wizard.delete_cipher_locked"), true)
                    } else {
                        (t!("wizard.delete_cipher"), false)
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
                t!("wizard.hint_step1"),
                Style::default().fg(Color::Gray),
            )));
        }
        WizardStep::EnterPassword => {
            lines.push(Line::from(t!("wizard.step2")));
            lines.push(Line::from(""));

            if wizard.direction == WizardDirection::Remove || !wizard.confirming {
                lines.push(Line::from(t!("wizard.enter_password")));
                lines.push(Line::from(Span::styled(
                    "*".repeat(wizard.password.chars().count()),
                    Style::default().fg(Color::White),
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    t!("wizard.password_entered"),
                    Style::default().fg(Color::Green),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(t!("wizard.confirm_password")));
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
                t!("wizard.hint_password"),
                Style::default().fg(Color::Gray),
            )));
        }
        WizardStep::ConfirmDelete => {
            lines.push(Line::from(t!("wizard.step3")));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                t!("wizard.delete_warning"),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(t!("wizard.vault_name", wizard.vault_name)));
            lines.push(Line::from(""));
            lines.push(Line::from(t!("wizard.confirm_delete_prompt")));
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
                t!("wizard.hint_confirm"),
                Style::default().fg(Color::Gray),
            )));
        }
        WizardStep::Running => {
            lines.push(Line::from(t!("status.executing")));
            lines.push(Line::from(""));
            if !wizard.checks.is_empty() {
                lines.push(Line::from(Span::styled(
                    t!("wizard.capacity_check"),
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
                lines.push(Line::from(t!("wizard.migrate_progress", pct)));
                let bar_width = 40usize.min(area.width.saturating_sub(20) as usize);
                let filled = (bar_width * pct as usize) / 100;
                let bar: String =
                    "▓".repeat(filled) + &"░".repeat(bar_width.saturating_sub(filled));
                lines.push(Line::from(Span::styled(
                    bar,
                    Style::default().fg(Color::Green),
                )));
                if total > 0 {
                    lines.push(Line::from(t!("wizard.bytes_progress", done, total)));
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                t!("wizard.hint_running"),
                Style::default().fg(Color::Gray),
            )));
        }
        WizardStep::Done => {
            lines.push(Line::from(Span::styled(
                t!("wizard.done"),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(t!("wizard.any_key_return")));
        }
        WizardStep::Failed(err) => {
            lines.push(Line::from(Span::styled(
                t!("wizard.failed"),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(err.as_str()));
            lines.push(Line::from(""));
            lines.push(Line::from(t!("wizard.any_key_return")));
        }
    }

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(t!("wizard.title")),
        )
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
        None => (t!("common.unknown").to_string(), "-".to_string()),
    };
    let popup = centered_rect(60, 30, area);
    f.render_widget(Clear, popup);

    let lw = label_column(&["confirm.vault", "confirm.mount_point"]);
    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            t!("confirm.umount_title"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw(pad_label(t!("confirm.vault"), lw)),
            Span::raw(name),
        ]),
        Line::from(vec![
            Span::raw(pad_label(t!("confirm.mount_point"), lw)),
            Span::raw(mp),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            t!("confirm.hint"),
            Style::default().fg(Color::Gray),
        )),
    ];
    let para = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(t!("confirm.title"))
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
            t!("help.title"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            t!("help.section_version"),
            Style::default().fg(Color::Magenta),
        )),
        Line::from(format!("  {} {}", APP_NAME, APP_VERSION)),
        Line::from(t!("help.commit", APP_COMMIT)),
        Line::from(t!("help.build", APP_BUILD_TIME)),
        Line::from(t!("help.repo", repo_url)),
        Line::from(""),
        Line::from(Span::styled(
            t!("help.section_paths"),
            Style::default().fg(Color::Magenta),
        )),
        Line::from(format!("  {}{}", t!("common.cli"), cli)),
        Line::from(t!("help.config", cfg)),
        Line::from(t!("help.data", data)),
        Line::from(""),
        Line::from(Span::styled(
            t!("help.section_args"),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(t!("help.arg_config")),
        Line::from(t!("help.arg_data_dir")),
        Line::from(t!("help.arg_lang")),
        Line::from(t!("help.arg_help")),
        Line::from(t!("help.arg_version")),
        Line::from(""),
        Line::from(Span::styled(
            t!("help.section_keys_nav"),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(t!("help.nav_direct_page")),
        Line::from(t!("help.nav_cycle_page")),
        Line::from(t!("help.nav_direct_focus")),
        Line::from(t!("help.nav_cycle_focus")),
        Line::from(""),
        Line::from(Span::styled(
            t!("help.section_global"),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(t!("help.global_keys")),
        Line::from(t!("help.global_ctrl")),
        Line::from(t!("help.global_lang", i18n::lang().display_name())),
        Line::from(""),
        Line::from(Span::styled(
            t!("help.section_tab1"),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(t!("help.tab_move_select")),
        Line::from(t!("help.tab1_actions")),
        Line::from(t!("help.tab1_dir")),
        Line::from(""),
        Line::from(Span::styled(
            t!("help.section_tab2"),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(t!("help.tab_move_select")),
        Line::from(t!("help.tab2_wizard")),
        Line::from(t!("help.tab_dir")),
        Line::from(""),
        Line::from(Span::styled(
            t!("help.section_tab3"),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(t!("help.tab_move_select")),
        Line::from(t!("help.tab3_wizard")),
        Line::from(t!("help.tab_dir")),
        Line::from(""),
        Line::from(Span::styled(
            t!("help.section_history"),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(t!("help.history_keys")),
        Line::from(""),
        Line::from(Span::styled(
            t!("help.section_export"),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(t!("help.export_hint", app.help_file.display())),
        Line::from(""),
        Line::from(Span::styled(
            t!("help.close_hint"),
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
        .unwrap_or(t!("common.global"));
    let tab_str = match tab {
        SettingsTab::Gocryptfs => t!("settings.tab_gocryptfs"),
        SettingsTab::Rsync => t!("settings.tab_rsync"),
        SettingsTab::Filters => t!("settings.tab_filters"),
        SettingsTab::Perm => t!("settings.tab_perm"),
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(t!("settings.scope"), Style::default().fg(Color::Cyan)),
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
            t!("settings.category", tab_str),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(Span::styled(
            t!("settings.language", i18n::lang().display_name()),
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
            lines.push(Line::from(t!("settings.filter_rules")));
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
        t!("settings.hint"),
        Style::default().fg(Color::Gray),
    )));

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(t!("settings.title"))
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
        t!("history.title"),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    let src_str = filter.src.as_deref().unwrap_or(t!("history.filter_all"));
    let result_str = filter.result.as_deref().unwrap_or(t!("history.filter_all"));
    let action_str = filter.action.as_deref().unwrap_or(t!("history.filter_all"));
    lines.push(Line::from(vec![
        Span::styled(t!("history.filter_label"), Style::default().fg(Color::Cyan)),
        Span::styled(
            t!("history.filter_src", src_str),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("  "),
        Span::styled(
            t!("history.filter_result", result_str),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("  "),
        Span::styled(
            t!("history.filter_action", action_str),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("    "),
        Span::styled(
            t!("history.filter_hint"),
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    lines.push(Line::from(t!(
        "history.count",
        entries.len(),
        MAX_HISTORY_DISPLAY
    )));
    lines.push(Line::from(""));

    if entries.is_empty() {
        lines.push(Line::from(t!("history.no_match")));
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
        t!("history.hint"),
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
        t!("common.any_key_close"),
        Style::default().fg(Color::Gray),
    )));

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(t!("overlay.message_title"))
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
            t!("task.mount", pw.vault_name),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(t!("wizard.enter_password")),
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
        t!("password.hint"),
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

// ---------- 打印生效路径 ----------

/// 组装 `--print-paths` 的报告行（排障用：一眼看清实际用的是哪份二进制/后端/配置/数据）
fn print_paths_lines(
    config: &str,
    data_dir: &Path,
    log_file: &Path,
    history_file: &Path,
    help_file: &Path,
) -> Vec<String> {
    const LABELS: [&str; 10] = [
        "paths.label_bin",
        "paths.label_version",
        "paths.label_backend",
        "paths.label_backend_dir",
        "paths.label_config",
        "paths.label_data",
        "paths.label_log",
        "paths.label_history",
        "paths.label_help",
        "paths.label_lang",
    ];
    let w = label_column(&LABELS);
    let row = |key: &'static str, value: String| format!("  {}{}", pad_label(t!(key), w), value);

    // 后端的来源：GOCRYPTFS_CLI > 内嵌释放副本 > PATH
    let backend = cli_path();
    let source = if std::env::var("GOCRYPTFS_CLI")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        t!("paths.source_env")
    } else if backend::embedded_path().is_some() {
        t!("paths.source_embedded")
    } else if resolved_cli_path() != CLI_NAME {
        t!("paths.source_path")
    } else {
        t!("paths.source_missing")
    };
    let backend_dir = backend::extract_dir(data_dir);

    vec![
        t!("paths.header").to_string(),
        row("paths.label_bin", bin_path()),
        row("paths.label_version", version_string()),
        row(
            "paths.label_backend",
            t!("paths.backend_line", backend, source).to_string(),
        ),
        row("paths.label_backend_dir", backend_dir.display().to_string()),
        row("paths.label_config", config.to_string()),
        row("paths.label_data", data_dir.display().to_string()),
        row("paths.label_log", log_file.display().to_string()),
        row("paths.label_history", history_file.display().to_string()),
        row("paths.label_help", help_file.display().to_string()),
        row("paths.label_lang", i18n::lang().code().to_string()),
        String::new(),
        t!("paths.hint").to_string(),
    ]
}

// ---------- 打印帮助 ----------

fn print_help(app_config: &str, data_dir: &Path, log_file: &Path, history_file: &Path) {
    // 显示"实际会用的后端"：GOCRYPTFS_CLI > 内嵌释放副本 > PATH 中的 gocryptfs-cli
    let cli_env = cli_path();
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
    let current = |v: String| t!("clihelp.current", v);

    println!("{} {}", APP_NAME, version_string());
    println!();
    // 顶部用法与选项
    println!("{}", t!("clihelp.usage"));
    println!("    {} [OPTIONS]", APP_NAME);
    println!();
    println!("{}", t!("clihelp.options"));
    println!("{}", t!("clihelp.opt_config"));
    println!("{}", current(app_config.to_string()));
    println!("{}", t!("clihelp.opt_data_dir"));
    println!("{}", current(data_dir.display().to_string()));
    println!("{}", t!("clihelp.opt_lang"));
    println!("{}", current(i18n::lang().code().to_string()));
    println!("{}", t!("clihelp.opt_check_deps"));
    println!("{}", t!("clihelp.opt_print_paths"));
    println!("{}", t!("clihelp.opt_help"));
    println!("{}", t!("clihelp.opt_version"));
    println!();
    println!("{}", t!("clihelp.environment"));
    println!("{}", t!("clihelp.env_cli", cli_env));
    println!("{}", t!("clihelp.env_editor", editor));
    println!();
    println!("{}", t!("clihelp.paths"));
    println!("{}", t!("clihelp.path_config", app_config));
    println!("{}", t!("clihelp.path_data", data_dir.display()));
    println!("{}", t!("clihelp.path_log", log_file.display()));
    println!("{}", t!("clihelp.path_history", history_file.display()));
    println!();
    println!("{}", t!("clihelp.more_info"));
}

fn print_version() {
    println!("{} {}", APP_NAME, version_string());
}

// ---------- 主入口 ----------

/// 解析界面语言：`--lang` > `GOCRYPTFS_TUI_LANG` > 配置 `language:` > 系统 locale > 默认。
fn resolve_lang(cli_lang: Option<&str>, config: &str) -> i18n::Lang {
    let config_lang = if config.is_empty() {
        String::new()
    } else {
        let primary = read_setting(config, "language", "");
        if primary.is_empty() {
            read_setting(config, "lang", "")
        } else {
            primary
        }
    };
    i18n::resolve(cli_lang, Some(config_lang.as_str()))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let outcome = cli::parse(std::env::args().collect());

    let data_dir = resolve_data_dir(outcome.opts.data_dir.clone());

    // 发行包不含 Shell 后端：运行 TUI 前先把内嵌副本释放到数据目录
    // （--help/--version 不写盘，只探测已释放的副本，保持只读）
    if matches!(outcome.action, cli::Action::Run) {
        if let Err(e) = backend::install_embedded(&data_dir, &version_line()) {
            backend::set_error(e);
        }
    } else {
        backend::peek_embedded(&data_dir);
    }

    let config = resolve_config(outcome.opts.config.clone());
    let log_file = data_dir.join("app.log.jsonl");
    let history_file = data_dir.join("history.jsonl");
    let help_file = data_dir.join("HELP.md");

    // 语言必须在任何用户可见输出之前确定
    i18n::set_lang(resolve_lang(outcome.opts.lang.as_deref(), &config));

    if let Some(err) = &outcome.error {
        if !matches!(outcome.action, cli::Action::Help | cli::Action::Version) {
            eprintln!("{}", t!("cli.error.prefix", err.render()));
            eprintln!("{}", t!("cli.error.usage_hint", APP_NAME));
            std::process::exit(2);
        }
    }

    match outcome.action {
        cli::Action::Help => {
            print_help(&config, &data_dir, &log_file, &history_file);
            return Ok(());
        }
        cli::Action::Version => {
            print_version();
            return Ok(());
        }
        cli::Action::PrintPaths => {
            for line in print_paths_lines(&config, &data_dir, &log_file, &history_file, &help_file)
            {
                println!("{}", line);
            }
            return Ok(());
        }
        cli::Action::CheckDeps => {
            let missing = deps::missing();
            let yq_bad = deps::yq_flavor_ok();
            for line in deps::report_lines(&missing, yq_bad) {
                println!("{}", line);
            }
            std::process::exit(if deps::has_problems(&missing, yq_bad) {
                1
            } else {
                0
            });
        }
        cli::Action::Run => {}
    }

    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        eprintln!("{}", t!("cli.error.data_dir_create", data_dir.display(), e));
        std::process::exit(1);
    }

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
                app.add_output(t!("status.config_edited").to_string());
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

#[cfg(test)]
mod i18n_render_tests;
