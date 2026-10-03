// backend.rs — 内嵌 Shell 后端（gocryptfs-cli + lib/*.sh）
//
// 背景：cargo-dist 只打包 Rust 构建产物，`gocryptfs-cli` 及其 Shell 库不在发行包里，
// 用户从 GitHub Release 安装后会遇到 "执行 CLI 失败"。这里把后端源码编译进二进制，
// 首次运行时释放到数据目录（`<data_dir>/backend/`），从而：
//   * 安装一次即可用，TUI 与后端版本始终配套；
//   * 不再受系统里旧版 `gocryptfs-cli` 影响；
//   * 环境变量 `GOCRYPTFS_CLI` 仍可覆盖（自定义/调试后端）。
//
// 释放是幂等的：内容一致则不动，二进制升级（内容变化）时自动覆盖。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// 内嵌的后端脚本（与二进制同版本、同源码）
pub const CLI_SCRIPT: &str = include_str!("../shell/gocryptfs-cli");
pub const LIB_SCRIPT: &str = include_str!("../shell/lib/gocryptfs-lib.sh");
pub const I18N_SCRIPT: &str = include_str!("../shell/lib/i18n.sh");

/// (相对路径, 内容, 是否可执行)
const SCRIPTS: [(&str, &str, bool); 3] = [
    ("gocryptfs-cli", CLI_SCRIPT, true),
    ("lib/gocryptfs-lib.sh", LIB_SCRIPT, false),
    ("lib/i18n.sh", I18N_SCRIPT, false),
];

static EMBEDDED_CLI: OnceLock<PathBuf> = OnceLock::new();
static ERROR: OnceLock<String> = OnceLock::new();

/// 后端释放目录：`<data_dir>/backend`
/// （`gocryptfs-cli` 按自身位置找 `<同目录>/lib/gocryptfs-lib.sh`）
pub fn extract_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backend")
}

/// 把内嵌后端释放到指定目录（不含全局状态，便于测试）
pub fn install_to(dir: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    for (rel, content, executable) in SCRIPTS {
        write_if_changed(&dir.join(rel), content, executable)?;
    }
    Ok(dir.join("gocryptfs-cli"))
}

/// 释放内嵌后端到 `<data_dir>/backend`，成功后记录路径供 [`embedded_path`] 使用。
///
/// 返回释放后的 `gocryptfs-cli` 路径；失败时返回错误描述（调用方应提示用户，
/// 并回退到 PATH 中的后端）。
pub fn install_embedded(data_dir: &Path) -> Result<PathBuf, String> {
    let cli = install_to(&extract_dir(data_dir))?;
    let _ = EMBEDDED_CLI.set(cli.clone());
    Ok(cli)
}

/// 仅当内容变化时写入（先写临时文件再 rename，避免留下半个脚本）
fn write_if_changed(path: &Path, content: &str, executable: bool) -> Result<(), String> {
    if let Ok(existing) = fs::read_to_string(path) {
        if existing == content {
            return Ok(());
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {}", parent.display(), e))?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    {
        let mut f = fs::File::create(&tmp).map_err(|e| format!("{}: {}", tmp.display(), e))?;
        f.write_all(content.as_bytes())
            .map_err(|e| format!("{}: {}", tmp.display(), e))?;
        let _ = f.sync_all();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if executable { 0o755 } else { 0o644 };
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(mode));
    }
    fs::rename(&tmp, path).map_err(|e| format!("{}: {}", path.display(), e))
}

/// 若后端此前已释放则记录其路径（只读探测，不写盘）。
/// 用于让 `--help`/`--version` 这类只读输出也显示真实的后端路径。
pub fn peek_embedded(data_dir: &Path) {
    let cli = extract_dir(data_dir).join("gocryptfs-cli");
    if cli.is_file() {
        let _ = EMBEDDED_CLI.set(cli);
    }
}

/// 已释放的内嵌后端路径（`gocryptfs-cli`）
pub fn embedded_path() -> Option<PathBuf> {
    EMBEDDED_CLI.get().cloned()
}

/// `cli` 是否指向内嵌释放的后端
pub fn is_embedded(cli: &str) -> bool {
    EMBEDDED_CLI
        .get()
        .map(|p| p.to_string_lossy() == cli)
        .unwrap_or(false)
}

/// 构造调用后端的命令。
///
/// 内嵌释放的脚本用 `bash <路径>` 执行：既不受数据目录可能 `noexec` 挂载影响，
/// 也不依赖脚本自身权限位（与它的 `#!/bin/bash` shebang 要求一致）。
pub fn cli_command(cli: &str) -> Command {
    if is_embedded(cli) {
        let mut cmd = Command::new("bash");
        cmd.arg(cli);
        cmd
    } else {
        Command::new(cli)
    }
}

/// 记录释放失败原因（启动时提示一次）
pub fn set_error(msg: String) {
    let _ = ERROR.set(msg);
}

/// 释放失败原因
pub fn error() -> Option<&'static str> {
    ERROR.get().map(|s| s.as_str())
}
