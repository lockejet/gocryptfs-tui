// deps.rs — 运行时依赖检查
//
// 目的：依赖（gocryptfs / yq / jq / rsync …）缺失时，要在**安装阶段或首次运行**就
// 明确告诉用户缺什么、怎么装，而不是等到"列表为空""执行失败"这类间接现象。
//
// 三处共用同一套判断与文案：
//   * `gocryptfs-tui --check-deps`（安装脚本 / 自检 / CI 可用）
//   * TUI 启动时（依赖不全就写进输出区）
//   * `install.sh` 安装完成后

use crate::i18n::{arg, tr, trf};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 一条运行时依赖
pub struct Dep {
    /// 可执行文件名
    pub cmd: &'static str,
    /// 用途（i18n 键）
    pub purpose_key: &'static str,
    /// Debian/Ubuntu 包名
    pub apt: &'static str,
    /// false = 可选（缺失只影响个别功能）
    pub required: bool,
}

pub const DEPS: &[Dep] = &[
    Dep {
        cmd: "gocryptfs",
        purpose_key: "deps.purpose_gocryptfs",
        apt: "gocryptfs",
        required: true,
    },
    Dep {
        cmd: "fusermount",
        purpose_key: "deps.purpose_fusermount",
        apt: "fuse3",
        required: true,
    },
    Dep {
        cmd: "rsync",
        purpose_key: "deps.purpose_rsync",
        apt: "rsync",
        required: true,
    },
    Dep {
        cmd: "yq",
        purpose_key: "deps.purpose_yq",
        apt: "yq",
        required: true,
    },
    Dep {
        cmd: "jq",
        purpose_key: "deps.purpose_jq",
        apt: "jq",
        required: true,
    },
    Dep {
        cmd: "mountpoint",
        purpose_key: "deps.purpose_mountpoint",
        apt: "util-linux",
        required: true,
    },
    Dep {
        cmd: "tree",
        purpose_key: "deps.purpose_tree",
        apt: "tree",
        required: false,
    },
];

fn is_executable(p: &Path) -> bool {
    match std::fs::metadata(p) {
        Ok(m) if m.is_file() => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                m.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            {
                true
            }
        }
        _ => false,
    }
}

/// 在这些目录里查找可执行文件（可注入，便于测试）
pub fn exists_in(paths: &[PathBuf], cmd: &str) -> bool {
    paths.iter().any(|d| is_executable(&d.join(cmd)))
}

/// 当前进程的 PATH 目录列表
pub fn search_paths() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default()
}

/// 指定 PATH 下缺失的依赖
pub fn missing_in(paths: &[PathBuf]) -> Vec<&'static Dep> {
    DEPS.iter().filter(|d| !exists_in(paths, d.cmd)).collect()
}

/// 当前 PATH 下缺失的依赖
pub fn missing() -> Vec<&'static Dep> {
    missing_in(&search_paths())
}

/// 检查 yq 是否 mikefarah Go 版 v4。
///
/// Debian/Ubuntu 源里的 `yq` 是 Python 包装版，语法不同，会让后端读不出卷列表，
/// 因此除了"存在"之外还要看版本输出。返回 None 表示 yq 不存在或无法执行。
pub fn yq_flavor_ok() -> Option<bool> {
    if !exists_in(&search_paths(), "yq") {
        return None;
    }
    let out = Command::new("yq").arg("--version").output().ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .to_lowercase();
    Some(text.contains("mikefarah") || text.contains("v4."))
}

/// 有必需依赖缺失，或 yq 版本不对 → 需要提示用户
pub fn has_problems(missing: &[&Dep], yq_bad: Option<bool>) -> bool {
    missing.iter().any(|d| d.required) || yq_bad == Some(false)
}

/// 生成提示行（TUI 输出区与 `--check-deps` 共用）
pub fn report_lines(missing: &[&Dep], yq_bad: Option<bool>) -> Vec<String> {
    let req: Vec<&Dep> = missing.iter().copied().filter(|d| d.required).collect();
    let opt: Vec<&Dep> = missing.iter().copied().filter(|d| !d.required).collect();
    let mut lines: Vec<String> = Vec::new();

    if !req.is_empty() {
        lines.push(tr("deps.missing_header").to_string());
        for d in &req {
            lines.push(trf("deps.item", &[arg(d.cmd), arg(tr(d.purpose_key))]));
        }
        let apt: Vec<&str> = req.iter().map(|d| d.apt).collect();
        lines.push(trf("deps.apt_hint", &[arg(&apt.join(" "))]));
        if req.iter().any(|d| d.cmd == "yq") {
            lines.push(tr("deps.yq_hint").to_string());
        }
    }
    if yq_bad == Some(false) {
        lines.push(tr("deps.yq_wrong_flavor").to_string());
        lines.push(tr("deps.yq_hint").to_string());
    }
    if !opt.is_empty() {
        let names: Vec<&str> = opt.iter().map(|d| d.cmd).collect();
        lines.push(trf("deps.optional_missing", &[arg(&names.join(" "))]));
    }
    if lines.is_empty() {
        lines.push(tr("deps.ok").to_string());
    }
    lines
}
