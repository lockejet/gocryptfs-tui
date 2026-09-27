#![allow(dead_code)]

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

const APP_NAME: &str = env!("CARGO_PKG_NAME");
const CLI_NAME: &str = "gocryptfs-cli";
const MAX_OUTPUT: usize = 200;

// ============================================================
// 数据模型
// ============================================================

#[derive(Debug, Clone, Deserialize)]
struct Vault {
    id: u32,
    name: String,
    path: String,
    mount_point: String,
    mounted: bool,
    locked: bool,
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
    action: String,
    name: String,
    status: String,
    #[serde(default)]
    detail: String,
}

// ============================================================
// CLI 交互
// ============================================================

fn cli_path() -> String {
    std::env::var("GOCRYPTFS_CLI").unwrap_or_else(|_| CLI_NAME.to_string())
}

fn get_config_path() -> String {
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
        return cfg.join(APP_NAME).join("config.yaml").to_string_lossy().to_string();
    }
    String::new()
}

fn load_vaults(config: &str) -> Result<Vec<Vault>, String> {
    if config.is_empty() {
        return Err("配置路径为空".to_string());
    }
    let out = Command::new(cli_path())
        .args(["-c", config, "list", "--json"])
        .output()
        .map_err(|e| format!("执行 CLI 失败: {}（检查 PATH 或 GOCRYPTFS_CLI）", e))?;
    if !out.status.success() {
        return Err(format!(
            "CLI 错误: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let resp: CliListResponse = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("JSON 解析失败: {}", e))?;
    if resp.status != "ok" {
        return Err("CLI 返回非 ok".to_string());
    }
    Ok(resp.data.vaults)
}

#[derive(Debug)]
enum CliEvent {
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

fn spawn_cli_task(args: Vec<String>, password: Option<String>) -> Receiver<CliEvent> {
    let (tx, rx) = mpsc::channel();
    let cli = cli_path();
    thread::spawn(move || {
        let mut cmd = Command::new(&cli);
        cmd.args(&args)
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
            for line in BufReader::new(stderr).lines().flatten() {
                if let Some(evt) = parse_protocol_line(&line) {
                    if tx_err.send(evt).is_err() {
                        break;
                    }
                } else if !line.starts_with("@@") {
                    if tx_err.send(CliEvent::Stderr(line)).is_err() {
                        break;
                    }
                }
            }
        });
        let stdout = child.stdout.take().unwrap();
        let tx_out = tx.clone();
        let h_out = thread::spawn(move || {
            for line in BufReader::new(stdout).lines().flatten() {
                if let Some(evt) = parse_protocol_line(&line) {
                    if tx_out.send(evt).is_err() {
                        break;
                    }
                } else if !line.starts_with("@@") {
                    if tx_out.send(CliEvent::Stdout(line)).is_err() {
                        break;
                    }
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

/// 同步执行 CLI 并返回 stdout（用于 l/t 这种快速命令）
fn cli_sync(args: &[&str]) -> Result<String, String> {
    let out = Command::new(cli_path())
        .args(args)
        .output()
        .map_err(|e| format!("执行 CLI 失败: {}", e))?;
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

// ============================================================
// 状态定义
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq)]
enum Page {
    Mount,
    Create,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CreateMode {
    Create,
    Remove,
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
    Settings { tab: SettingsTab, scope_index: usize },
    History { selected: usize },
    Message(String),
}

struct PasswordInput {
    vault_name: String,
    buffer: String,
    error: Option<String>,
}

struct BackgroundTask {
    description: String,
    rx: Receiver<CliEvent>,
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

/// 目录内容展示（l/t 同步结果）
struct DirView {
    title: String,
    lines: Vec<String>,
    scroll: u16,
}

struct App {
    page: Page,
    create_mode: CreateMode,
    overlay: Option<Overlay>,
    vaults: Vec<Vault>,
    list_state: ListState,
    pending: Vec<String>,
    pending_state: ListState,
    wizard: Option<Wizard>,
    output: Vec<String>,
    status: String,
    config: String,
    password_input: Option<PasswordInput>,
    task: Option<BackgroundTask>,
    should_quit: bool,
    editor_request: bool,
    scope_names: Vec<String>,
    history_entries: Vec<HistoryEntry>,
    dir_view: Option<DirView>,
}

impl App {
    fn new() -> Self {
        let config = get_config_path();
        let mut app = App {
            page: Page::Mount,
            create_mode: CreateMode::Create,
            overlay: None,
            vaults: Vec::new(),
            list_state: ListState::default(),
            pending: Vec::new(),
            pending_state: ListState::default(),
            wizard: None,
            output: Vec::new(),
            status: "就绪".to_string(),
            config: config.clone(),
            password_input: None,
            task: None,
            should_quit: false,
            editor_request: false,
            scope_names: vec!["全局".to_string()],
            history_entries: Vec::new(),
            dir_view: None,
        };
        app.add_output(format!("{} 启动", APP_NAME));
        app.add_output(format!("CLI:  {}", cli_path()));
        app.add_output(format!(
            "配置: {}",
            if app.config.is_empty() { "(未找到)" } else { &app.config }
        ));

        if !app.config.is_empty() {
            match load_vaults(&config) {
                Ok(vaults) => {
                    let n = vaults.len();
                    app.vaults = vaults;
                    app.add_output(format!("加载 {} 个卷", n));
                    if n > 0 {
                        app.list_state.select(Some(0));
                    }
                }
                Err(e) => app.add_output(format!("加载失败: {}", e)),
            }
        }
        app.reload_pending();
        app.update_scope_names();
        app
    }

    fn add_output(&mut self, line: String) {
        let ts = chrono::Local::now().format("%H:%M:%S").to_string();
        self.output.push(format!("[{}] {}", ts, line));
        if self.output.len() > MAX_OUTPUT {
            let excess = self.output.len() - MAX_OUTPUT;
            self.output.drain(0..excess);
        }
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
                    let cur = self.list_state.selected().unwrap_or(0);
                    let idx = cur.min(self.vaults.len() - 1);
                    self.list_state.select(Some(idx));
                } else {
                    self.list_state.select(None);
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
                self.pending_state.select(Some(0));
            } else {
                self.pending_state.select(None);
            }
        }
    }

    fn selected_vault(&self) -> Option<&Vault> {
        self.list_state.selected().and_then(|i| self.vaults.get(i))
    }

    fn next(&mut self) {
        if self.vaults.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => (i + 1) % self.vaults.len(),
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn previous(&mut self) {
        if self.vaults.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(0) | None => self.vaults.len() - 1,
            Some(i) => i - 1,
        };
        self.list_state.select(Some(i));
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
                self.add_output(format!("跳过: {} 已挂载", name));
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

    fn start_umount(&mut self) {
        let name = match self.selected_vault() {
            Some(v) => v.name.clone(),
            None => {
                self.status = "无卷可选".to_string();
                return;
            }
        };
        if let Some(v) = self.selected_vault() {
            if !v.mounted {
                self.status = format!("{} 未挂载", name);
                self.add_output(format!("跳过: {} 未挂载", name));
                return;
            }
        }
        self.start_task(
            format!("卸载 {}", name),
            vec![
                "-c".into(),
                self.config.clone(),
                "umount".into(),
                name.clone(),
            ],
            None,
        );
    }

    fn list_dir(&mut self) {
        let vault = match self.selected_vault() {
            Some(v) => v.clone(),
            None => {
                self.status = "无卷可选".to_string();
                return;
            }
        };
        if !vault.mounted {
            self.status = format!("{} 未挂载", vault.name);
            self.add_output(format!("跳过: {} 未挂载", vault.name));
            return;
        }
        self.add_output(format!("执行: gocryptfs-cli ls {}", vault.name));
        match cli_sync(&["-c", &self.config, "ls", &vault.name]) {
            Ok(text) => {
                let lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
                let n = lines.len();
                self.dir_view = Some(DirView {
                    title: format!(" {} 列表 (ls -la, {} 行) ", vault.name, n),
                    lines,
                    scroll: 0,
                });
                self.status = format!("已获取 {} 的列表", vault.name);
            }
            Err(e) => {
                self.status = format!("列表失败: {}", e);
                self.add_output(format!("[FAIL] ls: {}", e));
            }
        }
    }

    fn tree_dir(&mut self) {
        let vault = match self.selected_vault() {
            Some(v) => v.clone(),
            None => {
                self.status = "无卷可选".to_string();
                return;
            }
        };
        if !vault.mounted {
            self.status = format!("{} 未挂载", vault.name);
            self.add_output(format!("跳过: {} 未挂载", vault.name));
            return;
        }
        self.add_output(format!("执行: gocryptfs-cli tree {}", vault.name));
        match cli_sync(&["-c", &self.config, "tree", &vault.name]) {
            Ok(text) => {
                let lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
                let n = lines.len();
                self.dir_view = Some(DirView {
                    title: format!(" {} 树状 (tree, {} 行) ", vault.name, n),
                    lines,
                    scroll: 0,
                });
                self.status = format!("已获取 {} 的树状", vault.name);
            }
            Err(e) => {
                self.status = format!("树状失败: {}", e);
                self.add_output(format!("[FAIL] tree: {}", e));
            }
        }
    }

    fn start_task(&mut self, desc: String, args: Vec<String>, password: Option<String>) {
        if self.task.is_some() {
            self.status = "已有任务运行中".to_string();
            return;
        }
        self.add_output(format!("执行: {} {}", cli_path(), args.join(" ")));
        self.status = format!("运行中: {}", desc);
        let rx = spawn_cli_task(args, password);
        self.task = Some(BackgroundTask {
            description: desc,
            rx,
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
            let desc = self.task.as_ref().map(|t| t.description.clone());
            self.task = None;
            if let Some(desc) = desc {
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
                }
            }
            self.reload_vaults();
        }
    }

    fn read_setting(&self, path: &str, default: &str) -> String {
        if let Ok(content) = std::fs::read_to_string(&self.config) {
            let key = path.split('.').last().unwrap_or(path);
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

    fn start_create_wizard(&mut self) {
        self.page = Page::Create;
        self.create_mode = CreateMode::Create;

        if self.pending.is_empty() {
            self.status = "待处理目录为空（按 e 编辑配置的 pending 段）".to_string();
            self.add_output("待处理目录为空，请编辑配置文件的 pending 段".to_string());
            return;
        }
        let idx = self.pending_state.selected().unwrap_or(0);
        if idx >= self.pending.len() {
            self.pending_state.select(Some(0));
            return self.start_create_wizard();
        }
        let source = self.pending[idx].clone();
        let name = std::path::Path::new(&source)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "vault".to_string());
        let cipher = format!(
            "{}/.cipher.d/{}",
            std::path::Path::new(&source)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string()),
            name
        );
        let tmp_mount = format!("{}.mount_tmp", source);
        let keep_source_cfg = self.read_setting("create.keep_source", "false") == "true";

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
            progress: None,
            checks: HashMap::new(),
            error: None,
            selected: 0,
        });
        self.status = "配置路径和选项".to_string();
    }

    fn start_remove_wizard(&mut self) {
        self.page = Page::Create;
        self.create_mode = CreateMode::Remove;

        let vault = match self.selected_vault() {
            Some(v) => v.clone(),
            None => {
                self.status = "无卷可选".to_string();
                self.add_output("无卷可选，请先切到 Tab1 查看卷列表".to_string());
                return;
            }
        };
        let restore_cfg = self.read_setting("remove.restore", "true") == "true";
        let delete_cipher_cfg = self.read_setting("remove.direct_delete_cipher", "false") == "true";

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
                if wizard.password != wizard.password_confirm {
                    wizard.error = Some("两次密码不一致".to_string());
                    wizard.confirming = false;
                    wizard.password_confirm.clear();
                    self.wizard = Some(wizard);
                    return;
                }
                let password = wizard.password.clone();
                wizard.step = WizardStep::Running;
                wizard.progress = None;
                wizard.checks.clear();
                wizard.error = None;

                let args = match wizard.direction {
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
                        a
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
                        a
                    }
                };
                let desc = match wizard.direction {
                    WizardDirection::Create => format!("创建 {}", wizard.name),
                    WizardDirection::Remove => format!("删除 {}", wizard.vault_name),
                };
                self.wizard = Some(wizard);
                self.start_task(desc, args, Some(password));
                self.status = "执行中...".to_string();
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
                1 => {
                    if !wizard.keep_source_locked {
                        wizard.keep_source = !wizard.keep_source;
                    }
                }
                _ => {}
            },
            WizardDirection::Remove => match wizard.selected {
                0 => wizard.dry_run = !wizard.dry_run,
                1 => {
                    if !wizard.restore_locked {
                        wizard.restore = !wizard.restore;
                    }
                }
                2 => {
                    if !wizard.delete_cipher_locked {
                        wizard.delete_cipher = !wizard.delete_cipher;
                    }
                }
                _ => {}
            },
        }
        self.wizard = Some(wizard);
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
    // ---- 密码输入模态 ----
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
                app.start_task(format!("挂载 {}", vault_name), args, Some(password));
            }
            KeyCode::Char(c) if !c.is_control() => {
                pw.buffer.push(c);
            }
            _ => {}
        }
        return;
    }

    // ---- 目录视图滚动（无浮层/向导时） ----
    if app.dir_view.is_some() && app.overlay.is_none() && app.wizard.is_none() {
        match key {
            KeyCode::PageDown => {
                if let Some(dv) = &mut app.dir_view {
                    dv.scroll = dv.scroll.saturating_add(10);
                }
                return;
            }
            KeyCode::PageUp => {
                if let Some(dv) = &mut app.dir_view {
                    dv.scroll = dv.scroll.saturating_sub(10);
                }
                return;
            }
            KeyCode::Char('g') => {
                if let Some(dv) = &mut app.dir_view {
                    dv.scroll = 0;
                }
                return;
            }
            KeyCode::Char('G') => {
                if let Some(dv) = &mut app.dir_view {
                    dv.scroll = dv.lines.len().saturating_sub(1) as u16;
                }
                return;
            }
            _ => {}
        }
    }

    // ---- 向导 ----
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
                        let confirming =
                            app.wizard.as_ref().map(|w| w.confirming).unwrap_or(false);
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
            WizardStep::Running => {
                if key == KeyCode::Esc {
                    app.status = "运行中无法中止".to_string();
                }
            }
            WizardStep::Done | WizardStep::Failed(_) => {
                app.wizard_advance();
            }
        }
        return;
    }

    // ---- 浮层 ----
    if app.overlay.is_some() {
        let overlay = app.overlay.take().unwrap();
        match overlay {
            Overlay::Help => {
                if !matches!(key, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')) {
                    app.overlay = Some(Overlay::Help);
                }
            }
            Overlay::Message(_) => {}
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
            Overlay::History { selected } => match key {
                KeyCode::Esc => {}
                KeyCode::Char('j') | KeyCode::Down => {
                    let max = app.history_entries.len().saturating_sub(1);
                    app.overlay = Some(Overlay::History {
                        selected: (selected + 1).min(max),
                    });
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    app.overlay = Some(Overlay::History {
                        selected: selected.saturating_sub(1),
                    });
                }
                _ => app.overlay = Some(Overlay::History { selected }),
            },
        }
        return;
    }

    // ---- 全局 ----
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
            app.history_entries = load_history();
            app.overlay = Some(Overlay::History { selected: 0 });
            return;
        }
        KeyCode::Char('e') => {
            app.editor_request = true;
            return;
        }
        KeyCode::Char('r') => {
            app.reload_vaults();
            app.reload_pending();
            app.dir_view = None;
            app.status = "已刷新".to_string();
            return;
        }
        KeyCode::Tab | KeyCode::BackTab => {
            app.page = match app.page {
                Page::Mount => Page::Create,
                Page::Create => Page::Mount,
            };
            app.status = match app.page {
                Page::Mount => "挂载/卸载 页面".to_string(),
                Page::Create => "创建/删除 页面".to_string(),
            };
            return;
        }
        KeyCode::Char('1') => {
            app.page = Page::Mount;
            app.status = "挂载/卸载 页面".to_string();
            return;
        }
        KeyCode::Char('2') => {
            app.page = Page::Create;
            app.status = "创建/删除 页面".to_string();
            return;
        }
        _ => {}
    }

    // ---- 页面专属 ----
    match app.page {
        Page::Mount => match key {
            KeyCode::Char('j') | KeyCode::Down => app.next(),
            KeyCode::Char('k') | KeyCode::Up => app.previous(),
            KeyCode::Char('m') | KeyCode::Char(' ') => app.start_mount(),
            KeyCode::Char('u') => app.start_umount(),
            KeyCode::Char('l') => app.list_dir(),
            KeyCode::Char('t') => app.tree_dir(),
            _ => {}
        },
        Page::Create => match key {
            KeyCode::Char('c') => {
                app.create_mode = CreateMode::Create;
                app.status = "创建 模式".to_string();
            }
            KeyCode::Char('d') => {
                app.create_mode = CreateMode::Remove;
                app.status = "删除 模式".to_string();
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                if app.create_mode == CreateMode::Create {
                    app.start_create_wizard();
                } else {
                    app.start_remove_wizard();
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if app.create_mode == CreateMode::Create {
                    if !app.pending.is_empty() {
                        let i = app
                            .pending_state
                            .selected()
                            .map(|i| (i + 1) % app.pending.len())
                            .unwrap_or(0);
                        app.pending_state.select(Some(i));
                    }
                } else {
                    app.next();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if app.create_mode == CreateMode::Create {
                    if !app.pending.is_empty() {
                        let i = app
                            .pending_state
                            .selected()
                            .map(|i| if i == 0 { app.pending.len() - 1 } else { i - 1 })
                            .unwrap_or(0);
                        app.pending_state.select(Some(i));
                    }
                } else {
                    app.previous();
                }
            }
            KeyCode::Char('l') => {
                if app.create_mode == CreateMode::Remove {
                    app.list_dir();
                } else {
                    app.status = "创建模式下不支持 l（请按 d 切到删除模式）".to_string();
                }
            }
            KeyCode::Char('t') => {
                if app.create_mode == CreateMode::Remove {
                    app.tree_dir();
                } else {
                    app.status = "创建模式下不支持 t（请按 d 切到删除模式）".to_string();
                }
            }
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
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(7),
            Constraint::Length(2),
        ])
        .split(area);

    render_tab_bar(f, app, chunks[0]);

    if app.wizard.is_some() {
        render_wizard(f, app, chunks[1]);
    } else {
        match app.page {
            Page::Mount => render_mount_page(f, app, chunks[1]),
            Page::Create => render_create_page(f, app, chunks[1]),
        }
    }

    render_output(f, app, chunks[2]);
    render_status(f, app, chunks[3]);

    if let Some(overlay) = &app.overlay {
        match overlay {
            Overlay::Help => render_help_overlay(f, area),
            Overlay::Settings { tab, scope_index } => {
                render_settings_overlay(f, *tab, *scope_index, app, area);
            }
            Overlay::History { selected } => {
                render_history_overlay(f, &app.history_entries, *selected, area);
            }
            Overlay::Message(msg) => render_message_overlay(f, msg, area),
        }
    }

    if let Some(pw) = &app.password_input {
        render_password_input(f, pw, area);
    }
}

fn render_tab_bar(f: &mut Frame, app: &App, area: Rect) {
    let (mount_style, create_style) = match app.page {
        Page::Mount => (
            Style::default()
                .bg(Color::Yellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(Color::Gray),
        ),
        Page::Create => (
            Style::default().fg(Color::Gray),
            Style::default()
                .bg(Color::Yellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
    };

    let line = Line::from(vec![
        Span::styled(" [1] 挂载/卸载 ", mount_style),
        Span::raw("  "),
        Span::styled(" [2] 创建/删除 ", create_style),
        Span::raw("    "),
        Span::styled("切换[Tab]", Style::default().fg(Color::Cyan)),
    ]);

    let para = Paragraph::new(line);
    f.render_widget(para, area);
}

fn render_mount_page(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let items: Vec<ListItem> = if app.vaults.is_empty() {
        vec![ListItem::new(Span::styled(
            "（无卷）",
            Style::default().fg(Color::Gray),
        ))]
    } else {
        app.vaults
            .iter()
            .map(|v| {
                let icon = if v.mounted { "●" } else { "○" };
                let color = if v.mounted { Color::Green } else { Color::Gray };
                let lock = if v.locked { "🔒" } else { "  " };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{} ", icon), Style::default().fg(color)),
                    Span::raw(&v.name),
                    Span::raw("  "),
                    Span::styled(lock.to_string(), Style::default().fg(Color::Yellow)),
                ]))
            })
            .collect()
    };

    let mut list_state = app.list_state.clone();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" 卷列表 "))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    f.render_stateful_widget(list, chunks[0], &mut list_state);
    app.list_state = list_state;

    // 右侧：如果 dir_view 有内容，上下分割
    if let Some(dv) = &app.dir_view {
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(7), Constraint::Min(3)])
            .split(chunks[1]);

        let detail_text = match app.selected_vault() {
            Some(v) => {
                let mounted_str = if v.mounted { "🔓 已挂载" } else { "🔒 未挂载" };
                let mounted_color = if v.mounted { Color::Green } else { Color::Red };
                Text::from(vec![
                    Line::from(vec![
                        Span::styled("名称: ", Style::default().fg(Color::Cyan)),
                        Span::raw(&v.name),
                        Span::raw("    "),
                        Span::styled("状态: ", Style::default().fg(Color::Cyan)),
                        Span::styled(mounted_str, Style::default().fg(mounted_color)),
                    ]),
                    Line::from(vec![
                        Span::styled("挂载点: ", Style::default().fg(Color::Cyan)),
                        Span::raw(&v.mount_point),
                    ]),
                ])
            }
            None => Text::from("（无卷）"),
        };
        let para = Paragraph::new(detail_text)
            .block(Block::default().borders(Borders::ALL).title(" 详情 "));
        f.render_widget(para, right_chunks[0]);

        let dv_lines: Vec<Line> = dv.lines.iter().map(|s| Line::from(s.as_str())).collect();
        let para = Paragraph::new(dv_lines)
            .block(Block::default().borders(Borders::ALL).title(dv.title.clone()))
            .scroll((dv.scroll, 0));
        f.render_widget(para, right_chunks[1]);
    } else {
        let detail = match app.selected_vault() {
            Some(v) => {
                let mounted_str = if v.mounted { "🔓 已挂载" } else { "🔒 未挂载" };
                let mounted_color = if v.mounted { Color::Green } else { Color::Red };
                let lock_str = if v.mounted {
                    "🔓 由 gocryptfs 接管"
                } else if v.locked {
                    "🔒 只读锁定 (555)"
                } else {
                    "🔓 未锁定 (755)"
                };
                vec![
                    Line::from(vec![
                        Span::styled("ID:       ", Style::default().fg(Color::Cyan)),
                        Span::raw(v.id.to_string()),
                    ]),
                    Line::from(vec![
                        Span::styled("名称:     ", Style::default().fg(Color::Cyan)),
                        Span::raw(&v.name),
                    ]),
                    Line::from(vec![
                        Span::styled("加密路径: ", Style::default().fg(Color::Cyan)),
                        Span::raw(&v.path),
                    ]),
                    Line::from(vec![
                        Span::styled("挂载点:   ", Style::default().fg(Color::Cyan)),
                        Span::raw(&v.mount_point),
                    ]),
                    Line::from(vec![
                        Span::styled("状态:     ", Style::default().fg(Color::Cyan)),
                        Span::styled(mounted_str, Style::default().fg(mounted_color)),
                    ]),
                    Line::from(vec![
                        Span::styled("保护:     ", Style::default().fg(Color::Cyan)),
                        Span::raw(lock_str),
                    ]),
                    Line::from(""),
                    Line::from(Span::styled(
                        "按 l 列表  t 树状  查看挂载点目录内容",
                        Style::default().fg(Color::DarkGray),
                    )),
                ]
            }
            None => vec![
                Line::from(Span::styled("（无卷）", Style::default().fg(Color::Gray))),
                Line::from(""),
                Line::from(Span::styled(
                    "按 e 编辑配置添加卷",
                    Style::default().fg(Color::DarkGray),
                )),
            ],
        };
        let para = Paragraph::new(detail)
            .block(Block::default().borders(Borders::ALL).title(" 详情 "))
            .wrap(Wrap { trim: false });
        f.render_widget(para, chunks[1]);
    }
}

fn render_create_page(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let mode = if app.create_mode == CreateMode::Create {
        "创建"
    } else {
        "删除"
    };
    let items: Vec<ListItem> = if app.create_mode == CreateMode::Create {
        if app.pending.is_empty() {
            vec![ListItem::new(Span::styled(
                "（无待处理目录）",
                Style::default().fg(Color::Gray),
            ))]
        } else {
            app.pending
                .iter()
                .map(|p| ListItem::new(Line::from(vec![Span::raw("○ "), Span::raw(p)])))
                .collect()
        }
    } else {
        if app.vaults.is_empty() {
            vec![ListItem::new(Span::styled(
                "（无卷）",
                Style::default().fg(Color::Gray),
            ))]
        } else {
            app.vaults
                .iter()
                .map(|v| {
                    let icon = if v.mounted { "●" } else { "○" };
                    let color = if v.mounted { Color::Green } else { Color::Gray };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{} ", icon), Style::default().fg(color)),
                        Span::raw(&v.name),
                    ]))
                })
                .collect()
        }
    };

    let mut list_state = if app.create_mode == CreateMode::Create {
        app.pending_state.clone()
    } else {
        app.list_state.clone()
    };

    let title = format!(" {} 加密卷 ", mode);
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    f.render_stateful_widget(list, chunks[0], &mut list_state);

    if app.create_mode == CreateMode::Create {
        app.pending_state = list_state;
    } else {
        app.list_state = list_state;
    }

    if let Some(dv) = &app.dir_view {
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(5), Constraint::Min(3)])
            .split(chunks[1]);

        render_create_hint(f, app, right_chunks[0]);

        let dv_lines: Vec<Line> = dv.lines.iter().map(|s| Line::from(s.as_str())).collect();
        let para = Paragraph::new(dv_lines)
            .block(Block::default().borders(Borders::ALL).title(dv.title.clone()))
            .scroll((dv.scroll, 0));
        f.render_widget(para, right_chunks[1]);
    } else {
        render_create_hint(f, app, chunks[1]);
    }
}

fn render_create_hint(f: &mut Frame, app: &App, area: Rect) {
    let mode = if app.create_mode == CreateMode::Create {
        "创建"
    } else {
        "删除"
    };
    let mut text = vec![
        Line::from(Span::styled(
            format!("当前模式: {} 加密卷", mode),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    if app.create_mode == CreateMode::Create {
        if app.pending.is_empty() {
            text.push(Line::from(Span::styled(
                "待处理目录为空",
                Style::default().fg(Color::Red),
            )));
            text.push(Line::from(""));
            text.push(Line::from("按 e 编辑配置的 pending 段"));
        } else {
            text.push(Line::from("按 Enter/Space 对选中的待处理目录创建加密"));
        }
    } else {
        if app.vaults.is_empty() {
            text.push(Line::from(Span::styled(
                "无卷可删除",
                Style::default().fg(Color::Red),
            )));
        } else {
            text.push(Line::from("按 Enter/Space 对选中的加密卷执行删除加密"));
            text.push(Line::from("按 l / t 查看该卷的目录"));
        }
    }

    let para = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title(" 提示 "))
        .wrap(Wrap { trim: false });
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
                    lines.push(Line::from(Span::styled(
                        "（路径如需修改，请按 Esc 退出后编辑配置文件）",
                        Style::default().fg(Color::Gray),
                    )));
                    lines.push(Line::from(""));
                    lines.push(render_option(
                        0,
                        wizard.selected,
                        wizard.dry_run,
                        false,
                        "预览模式（不实际修改）",
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
                "切换选项[Space]  移动[j/k]  继续[Enter]  取消[Esc]",
                Style::default().fg(Color::Gray),
            )));
        }
        WizardStep::EnterPassword => {
            lines.push(Line::from("步骤 2/3: 输入密码"));
            lines.push(Line::from(""));
            if !wizard.confirming {
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
                "继续[Enter]  删除[Backspace/Delete]  取消[Esc]",
                Style::default().fg(Color::Gray),
            )));
        }
        WizardStep::Running => {
            lines.push(Line::from("步骤 3/3: 执行中..."));
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
                "请等待...",
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
        .wrap(Wrap { trim: false });
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
    Line::from(Span::styled(format!("{}{} {}", prefix, marker, label), style))
}

fn render_output(f: &mut Frame, app: &App, area: Rect) {
    let lines: Vec<Line> = app.output.iter().map(|s| Line::from(s.as_str())).collect();
    let para = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" 输出 "))
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let page_hint = match app.page {
        Page::Mount => " 移动[j/k] 挂载[m/Space] 卸载[u] 列表[l] 树状[t]",
        Page::Create => {
            if app.create_mode == CreateMode::Create {
                " 移动[j/k] 创建模式[c] 删除模式[d] 进入[Enter/Space]"
            } else {
                " 移动[j/k] 创建模式[c] 删除模式[d] 进入[Enter/Space] 列表[l] 树状[t]"
            }
        }
    };
    let global_hint = " 设置[s] 历史[h] 编辑[e] 帮助[?] 退出[q] 切页[Tab]";

    let task_info = if let Some(t) = &app.task {
        format!("  [运行: {}]", t.description)
    } else {
        String::new()
    };

    let line1 = Line::from(vec![
        Span::styled(page_hint, Style::default().fg(Color::White)),
        Span::styled(global_hint, Style::default().fg(Color::Gray)),
    ]);
    let line2 = Line::from(vec![
        Span::styled(" 状态: ", Style::default().fg(Color::Cyan)),
        Span::raw(&app.status),
        Span::styled(task_info, Style::default().fg(Color::Yellow)),
    ]);
    let para = Paragraph::new(vec![line1, line2])
        .style(Style::default().fg(Color::White).bg(Color::DarkGray));
    f.render_widget(para, area);
}

fn render_help_overlay(f: &mut Frame, area: Rect) {
    let popup = centered_rect(80, 85, area);
    f.render_widget(Clear, popup);

    let text = vec![
        Line::from(Span::styled(
            "帮助",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "全局（任何页面都可用）:",
            Style::default().fg(Color::Cyan),
        )),
        Line::from("  切页[Tab]       在 [1]挂载/卸载 与 [2]创建/删除 之间切换"),
        Line::from("  跳页[1/2]       直接跳到指定页面"),
        Line::from("  设置[s]         打开设置浮层"),
        Line::from("  历史[h]         打开历史浮层"),
        Line::from("  编辑[e]         用外部编辑器打开配置文件"),
        Line::from("  刷新[r]         重新加载卷列表与待处理目录"),
        Line::from("  帮助[?]         打开/关闭本帮助"),
        Line::from("  退出[q]         退出程序"),
        Line::from(""),
        Line::from(Span::styled(
            "页面 1（挂载/卸载）:",
            Style::default().fg(Color::Cyan),
        )),
        Line::from("  移动[j/k]       上下移动选择"),
        Line::from("  挂载[m/Space]   挂载选中的卷（弹出密码输入）"),
        Line::from("  卸载[u]         卸载选中的卷"),
        Line::from("  列表[l]         列出挂载点目录（ls -la）"),
        Line::from("  树状[t]         树状显示挂载点目录（tree）"),
        Line::from(""),
        Line::from(Span::styled(
            "页面 2（创建/删除）:",
            Style::default().fg(Color::Cyan),
        )),
        Line::from("  移动[j/k]       上下移动选择"),
        Line::from("  创建模式[c]     切换到创建模式（左侧显示 pending 目录）"),
        Line::from("  删除模式[d]     切换到删除模式（左侧显示加密卷）"),
        Line::from("  进入[Enter/Space] 用当前模式打开向导"),
        Line::from("  列表[l]         列出当前卷目录（仅删除模式）"),
        Line::from("  树状[t]         树状显示当前卷目录（仅删除模式）"),
        Line::from(""),
        Line::from(Span::styled(
            "目录视图滚动:",
            Style::default().fg(Color::Cyan),
        )),
        Line::from("  [PgDn/PgUp]     上下翻页"),
        Line::from("  [g/G]           跳到首/尾"),
        Line::from(""),
        Line::from(Span::styled("向导:", Style::default().fg(Color::Cyan))),
        Line::from("  切换选项[Space] 切换选项（dry-run / 保留源文件 等）"),
        Line::from("  移动[j/k]       在选项间移动"),
        Line::from("  继续[Enter]     推进向导"),
        Line::from("  取消[Esc]       取消向导"),
        Line::from("  删除[Backspace/Delete/Ctrl+H] 删除密码字符"),
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
        .wrap(Wrap { trim: false });
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
        "切换作用域[Tab]  分类[g/r/f/p]  编辑[e]  关闭[Esc]",
        Style::default().fg(Color::Gray),
    )));

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" ⚙ 设置 ")
                .style(Style::default().bg(Color::Rgb(0, 0, 100))),
        )
        .wrap(Wrap { trim: false });
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

fn render_history_overlay(f: &mut Frame, entries: &[HistoryEntry], selected: usize, area: Rect) {
    let popup = centered_rect(80, 75, area);
    f.render_widget(Clear, popup);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        "📜 历史记录",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    if entries.is_empty() {
        lines.push(Line::from("（暂无历史）"));
    } else {
        for (i, e) in entries.iter().enumerate().rev().take(30) {
            let icon = match e.status.as_str() {
                "success" => "✅",
                s if s.starts_with("failed") => "❌",
                _ => "  ",
            };
            let prefix = if i == selected { "> " } else { "  " };
            let ts = if e.ts.len() >= 19 { &e.ts[11..19] } else { &e.ts };
            let detail = if e.detail.is_empty() {
                String::new()
            } else {
                format!(" ({})", e.detail)
            };
            lines.push(Line::from(format!(
                "{}{} {}  {} {}  {}{}",
                prefix, icon, ts, e.action, e.name, e.status, detail
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "移动[j/k]  关闭[Esc]",
        Style::default().fg(Color::Gray),
    )));

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().bg(Color::Rgb(0, 0, 100))),
        )
        .wrap(Wrap { trim: false });
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
        .wrap(Wrap { trim: false });
    f.render_widget(para, popup);
}

fn load_history() -> Vec<HistoryEntry> {
    let path = dirs::data_local_dir()
        .map(|d| d.join(APP_NAME).join("history.jsonl"))
        .unwrap_or_default();

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<HistoryEntry>(line).ok())
        .collect()
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
        "确认[Enter]  删除[Backspace/Delete]  取消[Esc]",
        Style::default().fg(Color::Gray),
    )));

    let para = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().bg(Color::Rgb(0, 0, 100))),
        )
        .wrap(Wrap { trim: false });
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
                handle_key(app, key.code, key.modifiers);
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

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

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    final_result
}