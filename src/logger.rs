// logger.rs — TUI 侧的 JSONL 日志模块
//
// 与 shell 侧 gocryptfs-lib.sh 的 log_history 保持同一格式。
// 所有记录追加到 app.log.jsonl，超过 max_size 后轮转。
//
// 单行格式：
// {
//   "ts": "2026-09-30T15:00:00+08:00",
//   "src": "tui" | "cli",
//   "action": "mount" | "umount" | "create" | "remove" | ...,
//   "target": "卷名 / 页面名 / 向导步骤",
//   "result": "started" | "success" | "failed" | "cancelled" | "",
//   "detail": "附加信息（空串表示无）",
//   "pid": 12345 | null,
//   "duration_ms": 2000 | null
// }

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Operation,
    Interactive,
    Debug,
}

impl LogLevel {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "interactive" => Self::Interactive,
            "debug" => Self::Debug,
            _ => Self::Operation,
        }
    }

    pub fn allows(&self, kind: LogKind) -> bool {
        matches!(
            (self, kind),
            (_, LogKind::Operation) | (Self::Debug, _) | (Self::Interactive, LogKind::Interactive)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    Operation,
    Interactive,
    Debug,
}

#[derive(Debug, Serialize)]
pub struct LogEntry {
    pub ts: String,
    pub src: &'static str,
    pub action: String,
    pub target: String,
    pub result: String,
    pub detail: String,
    pub pid: Option<u32>,
    pub duration_ms: Option<u64>,
}

impl LogEntry {
    pub fn new(action: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            ts: now_iso(),
            src: "tui",
            action: action.into(),
            target: target.into(),
            result: String::new(),
            detail: String::new(),
            pid: None,
            duration_ms: None,
        }
    }

    pub fn result(mut self, r: impl Into<String>) -> Self {
        self.result = r.into();
        self
    }

    pub fn detail(mut self, d: impl Into<String>) -> Self {
        self.detail = d.into();
        self
    }

    pub fn pid(mut self, p: u32) -> Self {
        self.pid = Some(p);
        self
    }

    pub fn duration_ms(mut self, d: u64) -> Self {
        self.duration_ms = Some(d);
        self
    }
}

pub struct Logger {
    path: PathBuf,
    level: LogLevel,
    max_size: u64,
    max_files: usize,
}

impl Logger {
    pub fn new(path: PathBuf, level: LogLevel, max_size: u64, max_files: usize) -> Self {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        Self {
            path,
            level,
            max_size,
            max_files,
        }
    }

    pub fn log(&self, kind: LogKind, entry: LogEntry) {
        if !self.level.allows(kind) {
            return;
        }
        self.rotate_if_needed();
        let line = match serde_json::to_string(&entry) {
            Ok(s) => s,
            Err(_) => return,
        };
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let _ = writeln!(f, "{}", line);
        }
    }

    fn rotate_if_needed(&self) {
        let size = match fs::metadata(&self.path) {
            Ok(m) => m.len(),
            Err(_) => return,
        };
        if size < self.max_size {
            return;
        }
        // 从后往前轮转 .N → .N+1，避免覆盖
        for i in (1..self.max_files).rev() {
            let from = self.path.with_extension(format!("jsonl.{}", i));
            let to = self.path.with_extension(format!("jsonl.{}", i + 1));
            let _ = fs::rename(&from, &to);
        }
        let backup = self.path.with_extension("jsonl.1");
        let _ = fs::rename(&self.path, &backup);
    }
}

pub fn now_iso() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}
