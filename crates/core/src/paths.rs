// paths.rs — 路径解析与运行期文件迁移（TUI / Web UI 共用）
//
// 纯逻辑，不依赖任何渲染层：配置、数据、状态目录的解析与旧布局迁移。

use std::path::{Path, PathBuf};

use crate::{backend, APP_NAME, CLI_NAME, LEGACY_APP_NAME};

/// 后端路径优先级：`GOCRYPTFS_CLI` > 内嵌释放副本 > PATH 中的 `gocryptfs-cli`。
pub fn cli_path_from(env: Option<&str>, embedded: Option<&str>) -> String {
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

pub fn cli_path() -> String {
    let env = std::env::var("GOCRYPTFS_CLI").ok();
    let embedded = backend::embedded_path().map(|p| p.to_string_lossy().to_string());
    cli_path_from(env.as_deref(), embedded.as_deref())
}

/// CLI 的实际路径：便于发现「调用到了旧的已安装后端」这类问题
/// （例如 /usr/local/bin 里的旧 gocryptfs-cli 不认识 GOCRYPTFS_LANG）。
pub fn resolved_cli_path() -> String {
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

/// 展开 `~` 并把相对路径转成绝对路径（不解析符号链接，保持用户可预期）。
/// 用于顶栏/帮助里展示配置与数据目录：`-c demo.yaml` 也应显示完整路径。
pub fn absolute_path(path: &str) -> String {
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

pub fn resolve_config(cli_opt: Option<PathBuf>) -> String {
    let raw = if let Some(p) = cli_opt {
        p.to_string_lossy().to_string()
    } else if let Some(p) = backend_config_path() {
        // 旧版 Shell 后端（仍叫 gocryptfs-tui）会返回旧目录；归一化到新目录名
        normalize_legacy_app_dir(&p)
    } else if let Some(cfg) = dirs::config_dir() {
        // dirs::config_dir() 已遵循 $XDG_CONFIG_HOME，其次才是 ~/.config
        cfg.join(APP_NAME)
            .join("config.yaml")
            .to_string_lossy()
            .to_string()
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home)
            .join(".config")
            .join(APP_NAME)
            .join("config.yaml")
            .to_string_lossy()
            .to_string()
    } else {
        String::new()
    };
    absolute_path(&raw)
}

/// 把路径里的旧应用目录名（`gocryptfs-tui`）换成新的 `gocryptfs-ui`。
/// 只替换「父目录名恰好等于旧名」的情况，不影响用户自定义路径。
pub fn normalize_legacy_app_dir(p: &str) -> String {
    let path = PathBuf::from(p);
    let parent_is_legacy = path
        .parent()
        .and_then(|d| d.file_name())
        .map(|n| n == LEGACY_APP_NAME)
        .unwrap_or(false);
    if !parent_is_legacy {
        return p.to_string();
    }
    let parent = match path.parent() {
        Some(d) => d,
        None => return p.to_string(),
    };
    let mut new_path = parent.with_file_name(APP_NAME);
    if let Some(name) = path.file_name() {
        new_path.push(name);
    }
    new_path.to_string_lossy().to_string()
}

/// 问 Shell 后端要配置路径（`gocryptfs-cli config`），失败返回 None。
pub fn backend_config_path() -> Option<String> {
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

pub fn resolve_data_dir(cli_opt: Option<PathBuf>) -> PathBuf {
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

/// 0.7 起应用目录名从 `gocryptfs-tui` 改为 `gocryptfs-ui`：
/// 首次启动时把旧目录整体搬到新位置（目标已存在则不动，等价于保留新目录）。
/// 只处理默认路径（尾部是 APP_NAME 的目录）；显式 `-D`/`-c` 不受影响。
pub fn migrate_legacy_dirs(config: &str, data_dir: &Path, state_dir: &Path) {
    if let Some(cfg_dir) = Path::new(config).parent() {
        if cfg_dir.file_name().map(|n| n == APP_NAME).unwrap_or(false) {
            move_legacy_dir(&cfg_dir.with_file_name(LEGACY_APP_NAME), cfg_dir);
        }
    }
    for dir in [data_dir, state_dir] {
        if dir.file_name().map(|n| n == APP_NAME).unwrap_or(false) {
            move_legacy_dir(&dir.with_file_name(LEGACY_APP_NAME), dir);
        }
    }
}

/// 旧目录存在且新目录不存在时搬迁；rename 失败（跨设备）退回递归复制。
fn move_legacy_dir(legacy: &Path, new: &Path) {
    if !legacy.is_dir() || new.exists() {
        return;
    }
    if let Some(parent) = new.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::rename(legacy, new).is_ok() {
        return;
    }
    if copy_dir_all(legacy, new).is_ok() {
        let _ = std::fs::remove_dir_all(legacy);
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else if ty.is_symlink() {
            #[cfg(unix)]
            {
                let target = std::fs::read_link(entry.path())?;
                let _ = std::os::unix::fs::symlink(target, &to);
            }
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// 运行期状态目录（日志/历史）。
///
/// 默认遵循 XDG：`$XDG_STATE_HOME/gocryptfs-tui`（通常是 `~/.local/state/gocryptfs-tui`）。
/// 显式指定数据目录（`-D` / `GOCRYPTFS_DATA_DIR`）时与数据目录合并——便于
/// `-D /tmp/dbg` 这类隔离调试把所有运行期文件放在同一处，也保持旧行为不变。
pub fn resolve_state_dir(data_dir: &Path, data_overridden: bool) -> PathBuf {
    if let Some(p) = std::env::var_os("GOCRYPTFS_STATE_DIR") {
        return PathBuf::from(absolute_path(&p.to_string_lossy()));
    }
    if data_overridden {
        return data_dir.to_path_buf();
    }
    match dirs::state_dir() {
        Some(d) => d.join(APP_NAME),
        None => data_dir.to_path_buf(),
    }
}

/// 旧布局（日志/历史在数据目录）→ 状态目录的一次性迁移；新路径优先，失败则保留旧文件。
pub fn migrate_runtime_files(data_dir: &Path, state_dir: &Path) {
    if data_dir == state_dir {
        return;
    }
    for name in ["app.log.jsonl", "history.jsonl"] {
        let src = data_dir.join(name);
        let dst = state_dir.join(name);
        if src.exists() && !dst.exists() {
            move_into(&src, &dst);
        }
    }
    // 轮转日志 app.log.jsonl.1 … .N
    if let Ok(entries) = std::fs::read_dir(data_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("app.log.jsonl.") {
                continue;
            }
            let dst = state_dir.join(&name);
            if !dst.exists() {
                move_into(&entry.path(), &dst);
            }
        }
    }
}

fn move_into(src: &Path, dst: &Path) {
    if std::fs::rename(src, dst).is_ok() {
        return;
    }
    if std::fs::copy(src, dst).is_ok() {
        let _ = std::fs::remove_file(src);
    }
}

pub fn migrate_old_history(data_dir: &Path) {
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

/// 当前运行的可执行文件路径（真实绝对路径），用于顶栏 `Bin:`。
pub fn bin_path() -> String {
    if let Ok(p) = std::env::current_exe() {
        return p.to_string_lossy().to_string();
    }
    match std::env::args().next() {
        Some(a) if !a.is_empty() => absolute_path(&a),
        _ => "?".to_string(),
    }
}

pub fn shorten_home(s: &str) -> String {
    if let Some(home) = std::env::var_os("HOME") {
        let h = home.to_string_lossy();
        if !h.is_empty() && s.starts_with(h.as_ref()) {
            return format!("~{}", &s[h.len()..]);
        }
    }
    s.to_string()
}
