use crossterm::{
    event::{
        read, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind,
        KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use serde::{Deserialize, Serialize};
use serde_yaml;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};
use std::{error::Error, fs, io, path::PathBuf};

// ---------- 应用程序常量（全部派生自 Cargo.toml，不硬编码）----------
const APP_NAME: &str = env!("CARGO_PKG_NAME");
const LOG_FILE: &str = concat!("/tmp/", env!("CARGO_PKG_NAME"), ".log");
const CONFIG_FILE: &str = "config.yaml";
const CONFIG_EXAMPLE_FILE: &str = concat!(env!("CARGO_PKG_NAME"), ".yaml.example");
const MAX_LOGS: usize = 100;

// ---------- 数据模型 ----------
#[derive(Debug, Clone, Deserialize, Serialize)]
struct VaultConfig {
    id: u32,
    name: String,
    path: String,
    mount_point: String,
}

#[derive(Debug, Clone)]
struct Vault {
    id: u32,
    name: String,
    path: String,
    mount_point: String,
    mounted: bool,
    locked: bool,
}

impl From<VaultConfig> for Vault {
    fn from(cfg: VaultConfig) -> Self {
        Vault {
            id: cfg.id,
            name: cfg.name,
            path: cfg.path,
            mount_point: cfg.mount_point,
            mounted: false,
            locked: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ConfigYaml {
    vaults: Vec<VaultConfig>,
}

// ---------- 内置默认配置 ----------
fn default_vaults() -> Vec<VaultConfig> {
    vec![
        VaultConfig {
            id: 1,
            name: "personal".to_string(),
            path: "/home/user/.cipher.d/personal".to_string(),
            mount_point: "/mnt/personal".to_string(),
        },
        VaultConfig {
            id: 2,
            name: "work".to_string(),
            path: "/home/user/.cipher.d/work".to_string(),
            mount_point: "/mnt/work".to_string(),
        },
        VaultConfig {
            id: 3,
            name: "backup".to_string(),
            path: "/home/user/.cipher.d/backup".to_string(),
            mount_point: "/mnt/backup".to_string(),
        },
    ]
}

const DEFAULT_YAML_EXAMPLE: &str = concat!(
    "# 示例配置文件 - ",
    env!("CARGO_PKG_NAME"),
    "\n# 复制到 ~/.config/",
    env!("CARGO_PKG_NAME"),
    "/config.yaml 并取消注释即可生效\n#\n# 注意：ID 必须唯一且从 1 开始递增\n# vaults:\n#   - id: 1\n#     name: personal\n#     path: /home/user/.cipher.d/personal\n#     mount_point: /mnt/personal\n#   - id: 2\n#     name: work\n#     path: /home/user/.cipher.d/work\n#     mount_point: /mnt/work\n#   - id: 3\n#     name: backup\n#     path: /home/user/.cipher.d/backup\n#     mount_point: /mnt/backup\n"
);

#[derive(Debug, Serialize, Deserialize, Default)]
struct PersistedState {
    selected_id: Option<u32>,
    mount_status: HashMap<u32, bool>,
}

#[derive(Debug, Clone)]
struct ScrollState {
    v: u16,
    h: u16,
}

impl Default for ScrollState {
    fn default() -> Self {
        ScrollState { v: 0, h: 0 }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Focus {
    List,
    Dir,
    Log,
}

struct App {
    vaults: Vec<Vault>,
    list_state: ListState,
    modal: Option<ModalState>,
    status: String,
    state_path: PathBuf,
    dir_content: Option<DirContent>,
    logs: VecDeque<String>,
    focus: Focus,
    dir_scroll: ScrollState,
    log_scroll: ScrollState,
    mount_attempts: HashMap<u32, u8>,
}

struct DirContent {
    text: String,
    dir_type: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
enum ModalState {
    PasswordInput {
        vault_index: usize,
        password: String,
        error: Option<String>,
    },
    ConfirmRemove {
        vault_index: usize,
    },
    AddVault,
    NewVault,
    Info(String),
    ConfigError {
        error: String,
    },
    ConfirmQuit,
}

impl App {
    fn new() -> Self {
        let config_dir = dirs::config_dir()
            .map(|d| d.join(APP_NAME))
            .unwrap_or_else(|| PathBuf::from(".config").join(APP_NAME));
        let config_path = config_dir.join(CONFIG_FILE);

        let (vaults, config_status, config_error) = if config_path.exists() {
            match Self::load_config(&config_path) {
                Ok(v) => {
                    let status = format!("加载配置: {}", config_path.display());
                    (v, Some(status), None)
                }
                Err(e) => {
                    let error_msg = format!("加载配置文件失败: {}", e);
                    (
                        Self::default_vaults_vec(),
                        Some(error_msg.clone()),
                        Some(error_msg),
                    )
                }
            }
        } else {
            let _ = Self::generate_example_config();
            let msg = format!(
                "未找到配置文件，已生成示例文件: {}/{}",
                std::env::current_dir()
                    .unwrap_or(PathBuf::from("."))
                    .display(),
                CONFIG_EXAMPLE_FILE
            );
            (Self::default_vaults_vec(), Some(msg), None)
        };

        let state_path = dirs::data_local_dir()
            .map(|d| d.join(APP_NAME).join("state.json"))
            .unwrap_or_else(|| PathBuf::from("state.json"));

        let mut app = App {
            vaults,
            list_state: ListState::default(),
            modal: None,
            status: "就绪".to_string(),
            state_path: state_path.clone(),
            dir_content: None,
            logs: VecDeque::with_capacity(MAX_LOGS),
            focus: Focus::List,
            dir_scroll: ScrollState::default(),
            log_scroll: ScrollState::default(),
            mount_attempts: HashMap::new(),
        };

        if let Some(msg) = config_status {
            app.add_log(msg);
        }
        if let Some(err) = config_error {
            app.modal = Some(ModalState::ConfigError { error: err });
        }

        // 检测挂载状态，未挂载的加只读锁 (chmod 555，保留 setgid)
        // 先收集日志，循环结束后写入，避免借用冲突
        let mut pending_logs: Vec<String> = Vec::new();
        for vault in &mut app.vaults {
            vault.mounted = Self::is_mounted(&vault.mount_point);
            if !vault.mounted {
                let mp = vault.mount_point.clone();
                match Self::lock_mount_point(&mp) {
                    Ok(()) => {
                        vault.locked = true;
                        pending_logs.push(format!("已锁定 (555): {}", mp));
                    }
                    Err(e) => {
                        pending_logs.push(format!("锁定失败 {}: {}", mp, e));
                    }
                }
            }
        }
        for msg in pending_logs {
            app.add_log(msg);
        }

        if let Some(state) = Self::load_state(&state_path) {
            if let Some(selected_id) = state.selected_id {
                if let Some(pos) = app.vaults.iter().position(|v| v.id == selected_id) {
                    app.list_state.select(Some(pos));
                } else if !app.vaults.is_empty() {
                    app.list_state.select(Some(0));
                }
            } else if !app.vaults.is_empty() {
                app.list_state.select(Some(0));
            }
        } else if !app.vaults.is_empty() {
            app.list_state.select(Some(0));
        }

        app.add_log(format!("{} 启动", APP_NAME));
        app
    }

    fn default_vaults_vec() -> Vec<Vault> {
        default_vaults().into_iter().map(Vault::from).collect()
    }

    fn generate_example_config() -> Result<(), String> {
        let example_path = PathBuf::from(CONFIG_EXAMPLE_FILE);
        fs::write(&example_path, DEFAULT_YAML_EXAMPLE)
            .map_err(|e| format!("生成示例配置文件失败: {}", e))?;
        Ok(())
    }

    fn load_config(path: &PathBuf) -> Result<Vec<Vault>, String> {
        if !path.exists() {
            return Err(format!("配置文件不存在: {}", path.display()));
        }
        let content =
            fs::read_to_string(path).map_err(|e| format!("读取配置文件失败: {}", e))?;
        let cfg: ConfigYaml =
            serde_yaml::from_str(&content).map_err(|e| format!("YAML 语法错误: {}", e))?;
        Ok(cfg.vaults.into_iter().map(Vault::from).collect())
    }

    fn save_config_yaml(&self) -> Result<PathBuf, String> {
        let config_dir = dirs::config_dir()
            .map(|d| d.join(APP_NAME))
            .unwrap_or_else(|| PathBuf::from(".config").join(APP_NAME));
        fs::create_dir_all(&config_dir).map_err(|e| format!("创建配置目录失败: {}", e))?;
        let config_path = config_dir.join(CONFIG_FILE);

        let configs: Vec<VaultConfig> = self
            .vaults
            .iter()
            .map(|v| VaultConfig {
                id: v.id,
                name: v.name.clone(),
                path: v.path.clone(),
                mount_point: v.mount_point.clone(),
            })
            .collect();
        let yaml_cfg = ConfigYaml { vaults: configs };
        let yaml_content =
            serde_yaml::to_string(&yaml_cfg).map_err(|e| format!("YAML 序列化失败: {}", e))?;
        fs::write(&config_path, yaml_content)
            .map_err(|e| format!("保存配置文件失败: {}", e))?;
        Ok(config_path)
    }

    fn get_config_path(&self) -> PathBuf {
        let config_dir = dirs::config_dir()
            .map(|d| d.join(APP_NAME))
            .unwrap_or_else(|| PathBuf::from(".config").join(APP_NAME));
        config_dir.join(CONFIG_FILE)
    }

    fn add_log(&mut self, msg: String) {
        let now = chrono::Local::now();
        let timestamp = now.format("%H:%M:%S").to_string();
        let log_entry = format!("[{}] {}", timestamp, msg);
        if self.logs.len() >= MAX_LOGS {
            self.logs.pop_front();
        }
        self.logs.push_back(log_entry);
        self.log_scroll.v = self.logs.len().saturating_sub(5) as u16;
    }

    fn reload_config(&mut self) -> Result<(), Box<dyn Error>> {
        let config_path = self.get_config_path();
        if !config_path.exists() {
            self.add_log("配置文件不存在，使用内置默认配置".to_string());
            self.vaults = Self::default_vaults_vec();
            let mut pending_logs: Vec<String> = Vec::new();
            for v in &mut self.vaults {
                v.mounted = Self::is_mounted(&v.mount_point);
                if !v.mounted {
                    let mp = v.mount_point.clone();
                    match Self::lock_mount_point(&mp) {
                        Ok(()) => {
                            v.locked = true;
                            pending_logs.push(format!("已锁定 (555): {}", mp));
                        }
                        Err(e) => {
                            pending_logs.push(format!("锁定失败 {}: {}", mp, e));
                        }
                    }
                }
            }
            for msg in pending_logs {
                self.add_log(msg);
            }
            self.list_state.select(Some(0));
            self.dir_content = None;
            self.dir_scroll = ScrollState::default();
            self.status = "配置已重置为默认".to_string();
            self.add_log("配置已重置为默认".to_string());
            return Ok(());
        }
        let new_vaults = Self::load_config(&config_path)
            .map_err(|e| Box::new(io::Error::new(io::ErrorKind::Other, e)))?;
        let old_selected = self.selected_vault().map(|v| v.id);
        self.vaults = new_vaults;
        let mut pending_logs: Vec<String> = Vec::new();
        for v in &mut self.vaults {
            v.mounted = Self::is_mounted(&v.mount_point);
            if !v.mounted {
                let mp = v.mount_point.clone();
                match Self::lock_mount_point(&mp) {
                    Ok(()) => {
                        v.locked = true;
                        pending_logs.push(format!("已锁定 (555): {}", mp));
                    }
                    Err(e) => {
                        pending_logs.push(format!("锁定失败 {}: {}", mp, e));
                    }
                }
            }
        }
        for msg in pending_logs {
            self.add_log(msg);
        }
        if let Some(id) = old_selected {
            if let Some(pos) = self.vaults.iter().position(|v| v.id == id) {
                self.list_state.select(Some(pos));
            } else if !self.vaults.is_empty() {
                self.list_state.select(Some(0));
            }
        } else if !self.vaults.is_empty() {
            self.list_state.select(Some(0));
        }
        self.dir_content = None;
        self.dir_scroll = ScrollState::default();
        self.status = "配置已重新加载".to_string();
        self.add_log("配置已重新加载".to_string());
        Ok(())
    }

    fn load_state(path: &PathBuf) -> Option<PersistedState> {
        if path.exists() {
            fs::read_to_string(path)
                .ok()
                .and_then(|content| serde_json::from_str(&content).ok())
        } else {
            None
        }
    }

    fn save_state(&self) {
        let state = PersistedState {
            selected_id: self.selected_vault().map(|v| v.id),
            mount_status: HashMap::new(),
        };
        if let Some(parent) = self.state_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(content) = serde_json::to_string_pretty(&state) {
            let _ = fs::write(&self.state_path, content);
        }
    }

    fn is_mounted(mount_point: &str) -> bool {
        let status = Command::new("mountpoint")
            .arg("-q")
            .arg(mount_point)
            .status();
        if let Ok(status) = status {
            return status.success();
        }
        if let Ok(content) = fs::read_to_string("/proc/mounts") {
            for line in content.lines() {
                if let Some(mp) = line.split_whitespace().nth(1) {
                    if mp == mount_point {
                        return true;
                    }
                }
            }
        }
        false
    }

    // ---------- 只读锁定 (chmod 555) ----------
    // 使用命令行 chmod 而非 fs::set_permissions，以保留 setgid/setuid/sticky 特殊位
    fn lock_mount_point(mount_point: &str) -> Result<(), String> {
        let path = PathBuf::from(mount_point);
        if !path.exists() {
            return Err(format!("挂载点不存在: {}", mount_point));
        }
        let output = Command::new("chmod")
            .arg("555")
            .arg(&path)
            .output()
            .map_err(|e| format!("执行 chmod 命令失败: {}", e))?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("chmod 555 失败: {}", stderr.trim()))
        }
    }

    // ---------- 解锁 (chmod 755) ----------
    fn unlock_mount_point(mount_point: &str) -> Result<(), String> {
        let path = PathBuf::from(mount_point);
        if !path.exists() {
            return Ok(());
        }
        let output = Command::new("chmod")
            .arg("755")
            .arg(&path)
            .output()
            .map_err(|e| format!("执行 chmod 命令失败: {}", e))?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("chmod 755 失败: {}", stderr.trim()))
        }
    }

    fn selected_index(&self) -> Option<usize> {
        self.list_state.selected()
    }

    fn selected_vault(&self) -> Option<&Vault> {
        self.selected_index().and_then(|i| self.vaults.get(i))
    }

    #[allow(dead_code)]
    fn selected_vault_mut(&mut self) -> Option<&mut Vault> {
        self.selected_index().and_then(|i| self.vaults.get_mut(i))
    }

    // ---------- 挂载 / 卸载 ----------
    fn toggle_mount(&mut self) {
        if self.focus != Focus::List {
            self.status = "请先切换到列表区 (按 1 或 Tab)".to_string();
            return;
        }
        if let Some(idx) = self.selected_index() {
            if let Some(vault) = self.vaults.get_mut(idx) {
                if vault.mounted {
                    self.modal = Some(ModalState::Info("已挂载，无需重复挂载".to_string()));
                    self.status = "已挂载，无需重复挂载".to_string();
                } else {
                    self.mount_attempts.insert(vault.id, 0);
                    self.modal = Some(ModalState::PasswordInput {
                        vault_index: idx,
                        password: String::new(),
                        error: None,
                    });
                    self.status = format!("请输入 {} 的密码", vault.name);
                }
            }
        }
    }

    fn do_unmount(&mut self) {
        if self.focus != Focus::List {
            self.status = "请先切换到列表区 (按 1 或 Tab)".to_string();
            return;
        }
        let idx = match self.selected_index() {
            Some(i) => i,
            None => return,
        };
        let (name, mount_point) = {
            if let Some(vault) = self.vaults.get_mut(idx) {
                if !vault.mounted {
                    self.modal = Some(ModalState::Info("已卸载，无需重复卸载".to_string()));
                    self.status = "已卸载，无需重复卸载".to_string();
                    return;
                }
                (vault.name.clone(), vault.mount_point.clone())
            } else {
                return;
            }
        };

        self.add_log(format!("执行: fusermount -u {}", mount_point));
        let output = Self::umount_vault(&mount_point);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            // 卸载成功后加锁 555
            let lock_result = Self::lock_mount_point(&mount_point);
            if let Some(vault) = self.vaults.get_mut(idx) {
                vault.mounted = false;
                vault.locked = lock_result.is_ok();
            }
            match lock_result {
                Ok(()) => self.add_log(format!("卸载后锁定 (555): {}", mount_point)),
                Err(e) => self.add_log(format!("锁定失败 {}: {}", mount_point, e)),
            }
            self.status = format!("已卸载: {}", name);
            self.save_state();
            self.dir_content = None;
            self.dir_scroll = ScrollState::default();
            self.add_log(format!("卸载成功: {}", name));
            if !stdout.is_empty() {
                self.add_log(format!("stdout: {}", stdout));
            }
            if !stderr.is_empty() {
                self.add_log(format!("stderr: {}", stderr));
            }
        } else {
            let err = format!("卸载失败: {}: {}", name, stderr);
            Self::log_error(&err);
            self.status = format!("卸载失败: {}", name);
            self.add_log(format!("卸载失败: {}", err));
            let msg = format!(
                "卸载失败\nstdout:\n{}\nstderr:\n{}\n\n提示: 若有进程占用挂载点，请先 cd 离开该目录或关闭相关程序。",
                stdout, stderr
            );
            self.modal = Some(ModalState::Info(msg));
        }
    }

    fn do_mount(&mut self, idx: usize, password: &str) {
        let (name, path, mount_point, vault_id) = {
            if let Some(vault) = self.vaults.get_mut(idx) {
                if vault.mounted {
                    self.modal = Some(ModalState::Info("已挂载，无需重复挂载".to_string()));
                    self.status = "已挂载，无需重复挂载".to_string();
                    return;
                }
                (
                    vault.name.clone(),
                    vault.path.clone(),
                    vault.mount_point.clone(),
                    vault.id,
                )
            } else {
                return;
            }
        };

        if let Err(e) = Self::check_gocryptfs() {
            self.status = format!("错误: {}", e);
            self.modal = Some(ModalState::Info(format!("错误: {}", e)));
            return;
        }

        // ===== 挂载前解锁（chmod 755，保留 setgid）=====
        if PathBuf::from(&mount_point).exists() {
            match Self::unlock_mount_point(&mount_point) {
                Ok(()) => {
                    self.add_log(format!("挂载前解锁 (755): {}", mount_point));
                    if let Some(vault) = self.vaults.get_mut(idx) {
                        vault.locked = false;
                    }
                }
                Err(e) => {
                    self.add_log(format!("解锁失败 {}: {}", mount_point, e));
                    self.status = format!("解锁失败: {}", e);
                    self.modal = Some(ModalState::Info(format!("无法解锁挂载点:\n{}", e)));
                    return;
                }
            }
        }

        self.add_log(format!(
            "执行: gocryptfs -passfile /tmp/... -allow_other {} {}",
            path, mount_point
        ));
        let output = Self::mount_vault(&path, &mount_point, password);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            self.mount_attempts.remove(&vault_id);
            if let Some(vault) = self.vaults.get_mut(idx) {
                vault.mounted = true;
                vault.locked = false;
            }
            self.status = format!("已挂载: {}", name);
            self.save_state();
            self.dir_content = None;
            self.dir_scroll = ScrollState::default();
            self.add_log(format!("挂载成功: {}", name));
            if !stdout.is_empty() {
                self.add_log(format!("stdout: {}", stdout));
            }
            if !stderr.is_empty() {
                self.add_log(format!("stderr: {}", stderr));
            }
            self.modal = None;
        } else {
            // 挂载失败：恢复 555 保护
            if PathBuf::from(&mount_point).exists() {
                let _ = Self::lock_mount_point(&mount_point);
                if let Some(vault) = self.vaults.get_mut(idx) {
                    vault.locked = true;
                }
            }

            let attempts = self.mount_attempts.entry(vault_id).or_insert(0);
            *attempts += 1;
            let remaining = 3 - *attempts;
            if remaining > 0 {
                let err_msg = format!("密码错误，剩余尝试次数: {}", remaining);
                self.add_log(format!("挂载失败: {}: {}", name, err_msg));
                self.status = format!("密码错误 (剩余{}次)", remaining);
                self.modal = Some(ModalState::PasswordInput {
                    vault_index: idx,
                    password: String::new(),
                    error: Some(err_msg),
                });
            } else {
                let err_msg = format!("密码错误3次，挂载失败: {}", name);
                self.add_log(err_msg.clone());
                self.save_state();
                let _ = disable_raw_mode();
                let _ = execute!(
                    std::io::stdout(),
                    LeaveAlternateScreen,
                    DisableMouseCapture
                );
                eprintln!("{}", err_msg);
                eprintln!("按 Enter 键退出...");
                let mut input = String::new();
                let _ = std::io::stdin().read_line(&mut input);
                std::process::exit(1);
            }
        }
    }

    fn check_gocryptfs() -> Result<(), String> {
        let status = Command::new("which")
            .arg("gocryptfs")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match status {
            Ok(s) if s.success() => Ok(()),
            _ => Err("gocryptfs is not installed or not in PATH".to_string()),
        }
    }

    fn mount_vault(cipher_path: &str, mount_point: &str, password: &str) -> std::process::Output {
        let passfile_path = format!("/tmp/{}-pass-{}.tmp", APP_NAME, std::process::id());
        let passfile_created = if let Ok(mut f) = File::create(&passfile_path) {
            let _ = f.write_all(password.as_bytes());
            let _ = f.sync_all();
            true
        } else {
            false
        };

        if !passfile_created {
            let err_msg = "Unable to create temporary password file".to_string();
            return std::process::Output {
                status: std::process::ExitStatus::from_raw(1),
                stdout: vec![],
                stderr: err_msg.into_bytes(),
            };
        }

        let output = Command::new("gocryptfs")
            .arg("-passfile")
            .arg(&passfile_path)
            .arg("-allow_other")
            .arg(cipher_path)
            .arg(mount_point)
            .output()
            .unwrap_or_else(|e| {
                eprintln!("执行挂载命令失败: {}", e);
                std::process::Output {
                    status: std::process::ExitStatus::from_raw(1),
                    stdout: vec![],
                    stderr: e.to_string().into_bytes(),
                }
            });

        let _ = fs::remove_file(&passfile_path);
        output
    }

    // ---------- 卸载（失败自动 lazy unmount）----------
    fn umount_vault(mount_point: &str) -> std::process::Output {
        // 第一次：常规卸载
        let output = Command::new("fusermount")
            .arg("-u")
            .arg(mount_point)
            .output();

        if let Ok(out) = &output {
            if out.status.success() {
                return output.unwrap();
            }
        }

        // 第二次：lazy unmount（进程占用时有效）
        let lazy_output = Command::new("fusermount")
            .arg("-u")
            .arg("-z")
            .arg(mount_point)
            .output()
            .unwrap_or_else(|e| std::process::Output {
                status: std::process::ExitStatus::from_raw(1),
                stdout: vec![],
                stderr: e.to_string().into_bytes(),
            });

        if lazy_output.status.success() {
            lazy_output
        } else {
            output.unwrap_or(lazy_output)
        }
    }

    fn log_error(msg: &str) {
        let now = chrono::Local::now();
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(LOG_FILE)
            .and_then(|mut f| f.write_all(format!("[{}] {}\n", now, msg).as_bytes()));
    }

    // ---------- 显示目录内容 ----------
    fn show_dir(&mut self, use_tree: bool) {
        if self.focus != Focus::List {
            self.status = "请先切换到列表区 (按 1 或 Tab)".to_string();
            return;
        }
        let vault = self.selected_vault().cloned();
        if let Some(v) = vault {
            let mut output_text = String::new();
            let cmd = if use_tree { "tree" } else { "ls" };
            let args = if use_tree {
                vec!["-L", "2"]
            } else {
                vec!["-la"]
            };

            let title = format!("明文目录 ({})", v.mount_point);
            output_text.push_str(&format!("\n{:^width$}\n", title, width = 60));
            if v.mounted {
                let mut cmd_args = args.clone();
                cmd_args.insert(0, &v.mount_point);
                let full_cmd = format!("{} {}", cmd, cmd_args.join(" "));
                self.add_log(format!("执行: {}", full_cmd));
                let out = Command::new(cmd)
                    .args(&cmd_args)
                    .output()
                    .unwrap_or_else(|e| {
                        eprintln!("执行 {} 命令失败: {}", cmd, e);
                        std::process::Output {
                            status: std::process::ExitStatus::from_raw(1),
                            stdout: vec![],
                            stderr: e.to_string().into_bytes(),
                        }
                    });
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                if out.status.success() {
                    output_text.push_str(&stdout);
                    output_text.push('\n');
                    self.add_log("明文目录读取成功".to_string());
                } else {
                    output_text.push_str(&format!("读取失败:\n{}\n", stderr));
                    self.add_log(format!("明文目录读取失败: {}", stderr));
                }
            } else {
                output_text.push_str("<未挂载>\n");
            }

            output_text.push('\n');

            let title = format!("密文目录 ({})", v.path);
            output_text.push_str(&format!("\n{:^width$}\n", title, width = 60));
            let mut cmd_args = args.clone();
            cmd_args.insert(0, &v.path);
            let full_cmd = format!("{} {}", cmd, cmd_args.join(" "));
            self.add_log(format!("执行: {}", full_cmd));
            let out = Command::new(cmd)
                .args(&cmd_args)
                .output()
                .unwrap_or_else(|e| {
                    eprintln!("执行 {} 命令失败: {}", cmd, e);
                    std::process::Output {
                        status: std::process::ExitStatus::from_raw(1),
                        stdout: vec![],
                        stderr: e.to_string().into_bytes(),
                    }
                });
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            if out.status.success() {
                output_text.push_str(&stdout);
                self.add_log("密文目录读取成功".to_string());
            } else {
                output_text.push_str(&format!("读取失败:\n{}\n", stderr));
                self.add_log(format!("密文目录读取失败: {}", stderr));
            }

            self.dir_content = Some(DirContent {
                text: output_text,
                dir_type: if use_tree {
                    "tree".to_string()
                } else {
                    "list".to_string()
                },
            });
            self.dir_scroll = ScrollState::default();
            self.status = format!(
                "已获取 {} 的目录内容 ({} 模式)",
                v.name,
                if use_tree { "树状" } else { "列表" }
            );
        }
    }

    fn list_vault(&mut self) {
        self.show_dir(false);
    }

    fn tree_vault(&mut self) {
        let status = Command::new("tree")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if status.is_ok() && status.unwrap().success() {
            self.show_dir(true);
        } else {
            let msg = "tree 命令未安装，请安装后重试".to_string();
            self.modal = Some(ModalState::Info(msg.clone()));
            self.status = "tree 命令未安装".to_string();
            self.add_log(msg);
        }
    }

    fn scroll_focus(&mut self, delta_v: i16, delta_h: i16) {
        match self.focus {
            Focus::Dir => {
                let new_v = self.dir_scroll.v as i16 + delta_v;
                let new_h = self.dir_scroll.h as i16 + delta_h;
                self.dir_scroll.v = if new_v < 0 { 0 } else { new_v as u16 };
                self.dir_scroll.h = if new_h < 0 { 0 } else { new_h as u16 };
            }
            Focus::Log => {
                let new_v = self.log_scroll.v as i16 + delta_v;
                let new_h = self.log_scroll.h as i16 + delta_h;
                self.log_scroll.v = if new_v < 0 { 0 } else { new_v as u16 };
                self.log_scroll.h = if new_h < 0 { 0 } else { new_h as u16 };
            }
            _ => {}
        }
    }

    fn jump_focus(&mut self, to_top: bool) {
        match self.focus {
            Focus::Dir => {
                if let Some(content) = &self.dir_content {
                    let lines = content.text.lines().count();
                    if lines > 0 {
                        self.dir_scroll.v = if to_top {
                            0
                        } else {
                            lines.saturating_sub(1) as u16
                        };
                    }
                }
            }
            Focus::Log => {
                let lines = self.logs.len();
                if lines > 0 {
                    self.log_scroll.v = if to_top {
                        0
                    } else {
                        lines.saturating_sub(1) as u16
                    };
                }
            }
            _ => {}
        }
    }

    fn jump_list(&mut self, to_top: bool) {
        if self.focus != Focus::List {
            self.status = "请先切换到列表区 (按 1 或 Tab)".to_string();
            return;
        }
        if !self.vaults.is_empty() {
            let idx = if to_top { 0 } else { self.vaults.len() - 1 };
            self.list_state.select(Some(idx));
            self.dir_content = None;
            self.dir_scroll = ScrollState::default();
        }
    }

    fn next_focus(&mut self) {
        self.focus = match self.focus {
            Focus::List => Focus::Dir,
            Focus::Dir => Focus::Log,
            Focus::Log => Focus::List,
        };
    }

    fn set_focus(&mut self, focus: Focus) {
        self.focus = focus;
    }

    fn next(&mut self) {
        if self.focus != Focus::List {
            self.status = "请先切换到列表区 (按 1 或 Tab)".to_string();
            return;
        }
        if self.vaults.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.vaults.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
        self.dir_content = None;
        self.dir_scroll = ScrollState::default();
    }

    fn previous(&mut self) {
        if self.focus != Focus::List {
            self.status = "请先切换到列表区 (按 1 或 Tab)".to_string();
            return;
        }
        if self.vaults.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.vaults.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
        self.dir_content = None;
        self.dir_scroll = ScrollState::default();
    }

    fn remove_vault(&mut self) {
        if self.focus != Focus::List {
            self.status = "请先切换到列表区 (按 1 或 Tab)".to_string();
            return;
        }
        if let Some(idx) = self.selected_index() {
            self.modal = Some(ModalState::ConfirmRemove { vault_index: idx });
        }
    }

    fn confirm_remove(&mut self, idx: usize) {
        if idx < self.vaults.len() {
            let name = self.vaults[idx].name.clone();
            self.vaults.remove(idx);
            self.list_state.select(Some(0));
            self.status = format!("已移除卷: {}", name);
            self.save_state();
            self.dir_content = None;
            self.dir_scroll = ScrollState::default();
            self.add_log(format!("已移除卷: {}", name));
        }
        self.modal = None;
    }

    fn add_vault(&mut self) {
        if self.focus != Focus::List {
            self.status = "请先切换到列表区 (按 1 或 Tab)".to_string();
            return;
        }
        self.modal = Some(ModalState::AddVault);
        self.status = "添加已有卷（模拟）...".to_string();
        self.add_log("添加已有卷（模拟）".to_string());
    }

    fn new_vault(&mut self) {
        if self.focus != Focus::List {
            self.status = "请先切换到列表区 (按 1 或 Tab)".to_string();
            return;
        }
        self.modal = Some(ModalState::NewVault);
        self.status = "新建卷（模拟）...".to_string();
        self.add_log("新建卷（模拟）".to_string());
    }

    fn close_modal(&mut self) {
        self.modal = None;
        self.status = "就绪".to_string();
    }

    fn confirm_quit(&mut self) {
        self.modal = Some(ModalState::ConfirmQuit);
        self.status = "确认退出？".to_string();
    }
}

// ---------- UI 渲染 ----------
fn ui(f: &mut Frame, app: &App) {
    let area = f.size();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(5),
            Constraint::Length(2),
        ])
        .split(area);

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[0]);

    // ------ 左栏 ------
    let items: Vec<ListItem> = app
        .vaults
        .iter()
        .map(|v| {
            let status_icon = if v.mounted { "●" } else { "○" };
            let color = if v.mounted {
                Color::Green
            } else {
                Color::Gray
            };
            let lock_icon = if v.locked { "🔒" } else { " " };
            let content = Line::from(vec![
                Span::styled(format!("{} ", status_icon), Style::default().fg(color)),
                Span::raw(&v.name),
                Span::raw(" "),
                Span::styled(lock_icon.to_string(), Style::default().fg(Color::Yellow)),
            ]);
            ListItem::new(content)
        })
        .collect();

    let list_focused = app.focus == Focus::List;
    let list_border_style = if list_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" [1] 📂 加密卷列表 ")
                .border_style(list_border_style),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");

    let mut list_state = app.list_state.clone();
    f.render_stateful_widget(list, main_chunks[0], &mut list_state);

    // ------ 右栏 ------
    let right_area = main_chunks[1];
    let (info_area, dir_area) = if app.dir_content.is_some() {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(right_area);
        (chunks[0], Some(chunks[1]))
    } else {
        (right_area, None)
    };

    // ---- 详情面板：三态显示 ----
    let detail_text = if let Some(v) = app.selected_vault() {
        let status_str = if v.mounted {
            "🔓 已挂载"
        } else {
            "🔒 未挂载"
        };
        let status_color = if v.mounted {
            Color::Green
        } else {
            Color::Red
        };
        // 三态：已挂载 / 未挂载+已锁定(555) / 未挂载+未锁定(755)
        let (lock_str, lock_color) = if v.mounted {
            ("🔓 已挂载 (gocryptfs 接管)", Color::Green)
        } else if v.locked {
            ("🔒 只读锁定 (555)", Color::Yellow)
        } else {
            ("🔓 未锁定 (755)", Color::Gray)
        };
        Text::from(vec![
            Line::from(vec![
                Span::styled("ID: ", Style::default().fg(Color::Cyan)),
                Span::raw(v.id.to_string()),
            ]),
            Line::from(vec![
                Span::styled("名称: ", Style::default().fg(Color::Cyan)),
                Span::raw(&v.name),
            ]),
            Line::from(vec![
                Span::styled("加密路径: ", Style::default().fg(Color::Cyan)),
                Span::raw(&v.path),
            ]),
            Line::from(vec![
                Span::styled("挂载点: ", Style::default().fg(Color::Cyan)),
                Span::raw(&v.mount_point),
            ]),
            Line::from(vec![
                Span::styled("状态: ", Style::default().fg(Color::Cyan)),
                Span::styled(status_str, Style::default().fg(status_color)),
            ]),
            Line::from(vec![
                Span::styled("保护: ", Style::default().fg(Color::Cyan)),
                Span::styled(lock_str, Style::default().fg(lock_color)),
            ]),
        ])
    } else {
        Text::from("请选择一个卷")
    };
    let detail_paragraph = Paragraph::new(detail_text)
        .block(Block::default().borders(Borders::ALL).title(" 📋 详情 "))
        .wrap(Wrap { trim: true });
    f.render_widget(detail_paragraph, info_area);

    if let Some(content) = &app.dir_content {
        if let Some(area) = dir_area {
            let dir_focused = app.focus == Focus::Dir;
            let dir_border_style = if dir_focused {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::White)
            };
            let title = match content.dir_type.as_str() {
                "tree" => " [2] 📂 目录内容 (树状) ",
                _ => " [2] 📂 目录内容 (列表) ",
            };
            let dir_paragraph = Paragraph::new(content.text.clone())
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title)
                        .border_style(dir_border_style),
                )
                .scroll((app.dir_scroll.v, app.dir_scroll.h))
                .wrap(Wrap { trim: false });
            f.render_widget(dir_paragraph, area);
        }
    }

    // ------ 输出区 ------
    let log_text = if app.logs.is_empty() {
        "".to_string()
    } else {
        app.logs.iter().cloned().collect::<Vec<_>>().join("\n")
    };
    let log_focused = app.focus == Focus::Log;
    let log_border_style = if log_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    let log_paragraph = Paragraph::new(log_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" [3] 📋 输出区 ")
                .border_style(log_border_style),
        )
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .scroll((app.log_scroll.v, app.log_scroll.h))
        .wrap(Wrap { trim: false });
    f.render_widget(log_paragraph, chunks[1]);

    // ------ 状态栏 ------
    let focus_indicator = match app.focus {
        Focus::List => "[1]列表",
        Focus::Dir => "[2]目录",
        Focus::Log => "[3]输出",
    };
    let status_line = match app.focus {
        Focus::List => Line::from(vec![
            Span::raw(" 焦点: "),
            Span::styled(focus_indicator, Style::default().fg(Color::Yellow)),
            Span::raw(" │ "),
            Span::raw("挂载["),
            Span::styled("m", Style::default().fg(Color::Yellow)),
            Span::raw("] 卸载["),
            Span::styled("u", Style::default().fg(Color::Yellow)),
            Span::raw("] 列表["),
            Span::styled("l", Style::default().fg(Color::Yellow)),
            Span::raw("] 树状["),
            Span::styled("t", Style::default().fg(Color::Yellow)),
            Span::raw("] 编辑["),
            Span::styled("e", Style::default().fg(Color::Yellow)),
            Span::raw("] 添加["),
            Span::styled("a", Style::default().fg(Color::Yellow)),
            Span::raw("] 新建["),
            Span::styled("n", Style::default().fg(Color::Yellow)),
            Span::raw("] 删除["),
            Span::styled("r", Style::default().fg(Color::Yellow)),
            Span::raw("] 首["),
            Span::styled("g", Style::default().fg(Color::Yellow)),
            Span::raw("] 尾["),
            Span::styled("G", Style::default().fg(Color::Yellow)),
            Span::raw("] │ 状态: "),
            Span::raw(&app.status),
        ]),
        Focus::Dir => Line::from(vec![
            Span::raw(" 焦点: "),
            Span::styled(focus_indicator, Style::default().fg(Color::Yellow)),
            Span::raw(" │ "),
            Span::raw("滚动(↑↓←→) 首["),
            Span::styled("g", Style::default().fg(Color::Yellow)),
            Span::raw("] 尾["),
            Span::styled("G", Style::default().fg(Color::Yellow)),
            Span::raw("] 上页["),
            Span::styled("PgUp", Style::default().fg(Color::Yellow)),
            Span::raw("] 下页["),
            Span::styled("PgDn", Style::default().fg(Color::Yellow)),
            Span::raw("] │ 状态: "),
            Span::raw(&app.status),
        ]),
        Focus::Log => Line::from(vec![
            Span::raw(" 焦点: "),
            Span::styled(focus_indicator, Style::default().fg(Color::Yellow)),
            Span::raw(" │ "),
            Span::raw("滚动(↑↓←→) 首["),
            Span::styled("g", Style::default().fg(Color::Yellow)),
            Span::raw("] 尾["),
            Span::styled("G", Style::default().fg(Color::Yellow)),
            Span::raw("] 上页["),
            Span::styled("PgUp", Style::default().fg(Color::Yellow)),
            Span::raw("] 下页["),
            Span::styled("PgDn", Style::default().fg(Color::Yellow)),
            Span::raw("] │ 状态: "),
            Span::raw(&app.status),
        ]),
    };
    let footer = Paragraph::new(status_line)
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .alignment(Alignment::Left)
        .block(Block::default());
    f.render_widget(footer, chunks[2]);

    // ------ 模态对话框 ------
    if let Some(modal_state) = &app.modal {
        let modal_area = centered_rect(60, 30, area);
        f.render_widget(Clear, modal_area);
        let mask = Block::default()
            .style(Style::default().bg(Color::Black))
            .borders(Borders::NONE);
        f.render_widget(mask, modal_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" ⚡ 操作 ")
            .style(Style::default().bg(Color::Rgb(0, 0, 139)).fg(Color::White));

        let text = match modal_state {
            ModalState::PasswordInput {
                password, error, ..
            } => {
                let stars = "*".repeat(password.chars().count());
                let mut text = format!("请输入密码:\n{}\n", stars);
                if let Some(err) = error {
                    text = format!("{}\n{}", err, text);
                }
                text.push_str("\n(Enter 确认, Esc 取消)");
                text
            }
            ModalState::ConfirmRemove { vault_index: idx } => {
                let name = app
                    .vaults
                    .get(*idx)
                    .map(|v| v.name.as_str())
                    .unwrap_or("未知");
                format!("确认移除卷 '{}' ? (y/n)", name)
            }
            ModalState::AddVault => "添加已有卷 (模拟)\n按任意键关闭".to_string(),
            ModalState::NewVault => "新建卷 (模拟)\n按任意键关闭".to_string(),
            ModalState::Info(msg) => msg.clone(),
            ModalState::ConfigError { error } => {
                format!(
                    "{}\n\n[{}] 默认配置  [{}] 编辑配置  [{}] 退出程序",
                    error,
                    Span::styled("d", Style::default().fg(Color::Yellow)),
                    Span::styled("e", Style::default().fg(Color::Yellow)),
                    Span::styled("x", Style::default().fg(Color::Yellow)),
                )
            }
            ModalState::ConfirmQuit => {
                "确认退出？\n\n按 Enter 确认退出，按 ESC 取消".to_string()
            }
        };

        let paragraph = Paragraph::new(text)
            .block(block)
            .style(Style::default().bg(Color::Rgb(0, 0, 139)).fg(Color::White))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
        f.render_widget(paragraph, modal_area);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

// ---------- 主循环 ----------
fn run_tui() -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    loop {
        terminal.draw(|f| ui(f, &app))?;

        if let Event::Key(key) = read()? {
            if key.kind == KeyEventKind::Press {
                // ---------- 模态框处理 ----------
                if let Some(modal) = app.modal.take() {
                    match modal {
                        ModalState::PasswordInput {
                            vault_index: idx,
                            mut password,
                            error,
                        } => {
                            match key.code {
                                KeyCode::Enter => {
                                    app.do_mount(idx, &password);
                                }
                                KeyCode::Esc => {
                                    app.status = "已取消挂载".to_string();
                                }
                                KeyCode::Backspace
                                | KeyCode::Char('\x7f')
                                | KeyCode::Char('\x08')
                                | KeyCode::Delete => {
                                    password.pop();
                                }
                                KeyCode::Char('h')
                                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    password.pop();
                                }
                                KeyCode::Char(c)
                                    if !c.is_control()
                                        && !key.modifiers.contains(KeyModifiers::CONTROL)
                                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                                {
                                    password.push(c);
                                }
                                _ => {}
                            }
                            if !matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
                                app.modal = Some(ModalState::PasswordInput {
                                    vault_index: idx,
                                    password,
                                    error,
                                });
                            }
                        }
                        ModalState::ConfirmRemove { vault_index: idx } => {
                            match key.code {
                                KeyCode::Char('y') => app.confirm_remove(idx),
                                KeyCode::Char('n') | KeyCode::Esc => {
                                    app.status = "已取消删除".to_string();
                                }
                                _ => {
                                    app.modal =
                                        Some(ModalState::ConfirmRemove { vault_index: idx });
                                }
                            }
                        }
                        ModalState::AddVault | ModalState::NewVault | ModalState::Info(_) => {
                            app.close_modal();
                        }
                        ModalState::ConfigError { error } => {
                            match key.code {
                                KeyCode::Char('d') => {
                                    app.modal = None;
                                    app.status = "已切换到内置默认配置".to_string();
                                    app.add_log("已切换到内置默认配置".to_string());
                                }
                                KeyCode::Char('x') => {
                                    app.save_state();
                                    break;
                                }
                                KeyCode::Char('e') => {
                                    app.modal = None;
                                    let config_path = app.get_config_path();
                                    if !config_path.exists() {
                                        if let Err(e) = app.save_config_yaml() {
                                            app.status = format!("创建配置文件失败: {}", e);
                                            app.add_log(format!("创建配置文件失败: {}", e));
                                            continue;
                                        }
                                    }
                                    let editor = if Command::new("nano")
                                        .arg("--version")
                                        .stdout(Stdio::null())
                                        .status()
                                        .is_ok()
                                    {
                                        "nano"
                                    } else if Command::new("vi")
                                        .arg("--version")
                                        .stdout(Stdio::null())
                                        .status()
                                        .is_ok()
                                    {
                                        "vi"
                                    } else {
                                        app.status = "未找到编辑器".to_string();
                                        app.add_log("未找到编辑器".to_string());
                                        continue;
                                    };

                                    disable_raw_mode()?;
                                    execute!(
                                        terminal.backend_mut(),
                                        LeaveAlternateScreen,
                                        DisableMouseCapture
                                    )?;
                                    terminal.show_cursor()?;

                                    let status = Command::new(editor).arg(&config_path).status();
                                    match status {
                                        Ok(s) if s.success() => {
                                            println!("配置文件已保存，重新加载...");
                                        }
                                        Ok(_) => {
                                            eprintln!("编辑器退出异常");
                                        }
                                        Err(e) => {
                                            eprintln!("无法启动编辑器: {}", e);
                                        }
                                    }

                                    enable_raw_mode()?;
                                    let mut stdout = io::stdout();
                                    execute!(
                                        stdout,
                                        EnterAlternateScreen,
                                        EnableMouseCapture
                                    )?;
                                    let backend = CrosstermBackend::new(stdout);
                                    terminal = Terminal::new(backend)?;

                                    if let Err(e) = app.reload_config() {
                                        app.status = format!("重新加载配置失败: {}", e);
                                        app.add_log(format!("重新加载配置失败: {}", e));
                                    } else {
                                        app.status = "配置已更新".to_string();
                                    }
                                }
                                _ => {
                                    app.modal = Some(ModalState::ConfigError { error });
                                }
                            }
                        }
                        ModalState::ConfirmQuit => match key.code {
                            KeyCode::Enter => {
                                app.save_state();
                                break;
                            }
                            KeyCode::Esc => {
                                app.close_modal();
                                app.status = "已取消退出".to_string();
                            }
                            _ => {
                                app.modal = Some(ModalState::ConfirmQuit);
                            }
                        },
                    }
                    continue;
                }

                // ---------- 全局按键 ----------
                match key.code {
                    KeyCode::Esc => continue,
                    KeyCode::Char('q') => {
                        app.confirm_quit();
                        continue;
                    }
                    KeyCode::Tab => {
                        app.next_focus();
                        continue;
                    }
                    KeyCode::Char('1') => {
                        app.set_focus(Focus::List);
                        continue;
                    }
                    KeyCode::Char('2') => {
                        app.set_focus(Focus::Dir);
                        continue;
                    }
                    KeyCode::Char('3') => {
                        app.set_focus(Focus::Log);
                        continue;
                    }
                    _ => {}
                }

                // ---------- 焦点区专属 ----------
                match app.focus {
                    Focus::List => match key.code {
                        KeyCode::Char('m') => app.toggle_mount(),
                        KeyCode::Char('u') => app.do_unmount(),
                        KeyCode::Char('l') => app.list_vault(),
                        KeyCode::Char('t') => app.tree_vault(),
                        KeyCode::Char('e') => {
                            app.save_state();
                            let config_path = app.get_config_path();
                            if !config_path.exists() {
                                if let Err(e) = app.save_config_yaml() {
                                    app.status = format!("创建配置文件失败: {}", e);
                                    app.add_log(format!("创建配置文件失败: {}", e));
                                    continue;
                                }
                            }
                            let editor = if Command::new("nano")
                                .arg("--version")
                                .stdout(Stdio::null())
                                .status()
                                .is_ok()
                            {
                                "nano"
                            } else if Command::new("vi")
                                .arg("--version")
                                .stdout(Stdio::null())
                                .status()
                                .is_ok()
                            {
                                "vi"
                            } else {
                                disable_raw_mode()?;
                                execute!(
                                    terminal.backend_mut(),
                                    LeaveAlternateScreen,
                                    DisableMouseCapture
                                )?;
                                terminal.show_cursor()?;
                                eprintln!("错误: 未找到 nano 或 vi 编辑器");
                                return Err("未找到编辑器".into());
                            };

                            disable_raw_mode()?;
                            execute!(
                                terminal.backend_mut(),
                                LeaveAlternateScreen,
                                DisableMouseCapture
                            )?;
                            terminal.show_cursor()?;

                            let status = Command::new(editor).arg(&config_path).status();
                            match status {
                                Ok(s) if s.success() => {
                                    println!("配置文件已保存，重新加载...");
                                }
                                Ok(_) => {
                                    eprintln!("编辑器退出异常");
                                }
                                Err(e) => {
                                    eprintln!("无法启动编辑器: {}", e);
                                }
                            }

                            enable_raw_mode()?;
                            let mut stdout = io::stdout();
                            execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
                            let backend = CrosstermBackend::new(stdout);
                            terminal = Terminal::new(backend)?;

                            if let Err(e) = app.reload_config() {
                                app.status = format!("重新加载配置失败: {}", e);
                                app.add_log(format!("重新加载配置失败: {}", e));
                            } else {
                                app.status = "配置已更新".to_string();
                            }
                        }
                        KeyCode::Char('a') => app.add_vault(),
                        KeyCode::Char('n') => app.new_vault(),
                        KeyCode::Char('r') => app.remove_vault(),
                        KeyCode::Char('g') => app.jump_list(true),
                        KeyCode::Char('G') => app.jump_list(false),
                        KeyCode::Down | KeyCode::Char('j') => app.next(),
                        KeyCode::Up | KeyCode::Char('k') => app.previous(),
                        _ => {}
                    },
                    Focus::Dir => {
                        let delta_v = match key.code {
                            KeyCode::Up => -1,
                            KeyCode::Down => 1,
                            _ => 0,
                        };
                        let delta_h = match key.code {
                            KeyCode::Left => -1,
                            KeyCode::Right => 1,
                            _ => 0,
                        };
                        if delta_v != 0 || delta_h != 0 {
                            app.scroll_focus(delta_v, delta_h);
                        } else {
                            match key.code {
                                KeyCode::Char('g') => app.jump_focus(true),
                                KeyCode::Char('G') => app.jump_focus(false),
                                KeyCode::PageUp => app.scroll_focus(-5, 0),
                                KeyCode::PageDown => app.scroll_focus(5, 0),
                                _ => {}
                            }
                        }
                    }
                    Focus::Log => {
                        let delta_v = match key.code {
                            KeyCode::Up => -1,
                            KeyCode::Down => 1,
                            _ => 0,
                        };
                        let delta_h = match key.code {
                            KeyCode::Left => -1,
                            KeyCode::Right => 1,
                            _ => 0,
                        };
                        if delta_v != 0 || delta_h != 0 {
                            app.scroll_focus(delta_v, delta_h);
                        } else {
                            match key.code {
                                KeyCode::Char('g') => app.jump_focus(true),
                                KeyCode::Char('G') => app.jump_focus(false),
                                KeyCode::PageUp => app.scroll_focus(-5, 0),
                                KeyCode::PageDown => app.scroll_focus(5, 0),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    run_tui()
}