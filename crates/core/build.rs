// build.rs — 编译期注入版本信息
//
// 优先级：环境变量 > git 命令 > "dev"
// 环境变量适合 CI（cargo-dist / Makefile 传值），
// git 命令适合本地开发。

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn main() {
    let version = std::env::var("BUILD_VERSION")
        .ok()
        .or_else(|| git(&["describe", "--tags", "--always", "--dirty"]))
        .unwrap_or_else(|| "dev".to_string());

    let commit = std::env::var("GIT_COMMIT")
        .ok()
        .or_else(|| git(&["rev-parse", "--short", "HEAD"]))
        .unwrap_or_else(|| "none".to_string());

    let build_time = std::env::var("BUILD_TIME").ok().unwrap_or_else(|| {
        Command::new("date")
            .arg("+%Y-%m-%d")
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "unknown".to_string())
    });

    println!("cargo:rustc-env=APP_VERSION={}", version);
    println!("cargo:rustc-env=APP_COMMIT={}", commit);
    println!("cargo:rustc-env=APP_BUILD_TIME={}", build_time);

    // 触发重编译
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-env-changed=BUILD_VERSION");
    println!("cargo:rerun-if-env-changed=GIT_COMMIT");
    println!("cargo:rerun-if-env-changed=BUILD_TIME");
}
