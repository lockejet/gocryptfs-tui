// protocol.rs — Shell 后端协议行解析（TUI / Web UI 共用）

#[derive(Debug, Clone, PartialEq)]
pub enum CliEvent {
    Pid(u32),
    Stdout(String),
    Stderr(String),
    Progress { pct: u8, done: u64, total: u64 },
    Check { key: String, value: String },
    Done(i32),
}

pub fn parse_protocol_line(line: &str) -> Option<CliEvent> {
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
