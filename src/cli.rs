// cli.rs — 命令行参数解析
//
// 手写解析，不引入 clap。
// 支持：-c/--config、-D/--data-dir、-h/--help、-V/--version
// 支持：--config=path、--data-dir=path
// 规则：
//   - -h/-V 优先于一切（即使和错误参数同时出现也先响应）
//   - 未知参数记录错误但不中断扫描，让后续 -h/-V 有机会生效
//   - -c/-D 缺值时无法恢复，立即中断
//   - 不接受位置参数

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Run,
    Help,
    Version,
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub config: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
    /// 原始参数（含程序名），供 TUI 顶部栏显示
    pub raw_args: Vec<String>,
}

pub struct ParseOutcome {
    pub action: Action,
    pub opts: Options,
    /// 仅当 action == Run 时才有意义
    pub error: Option<String>,
}

pub fn parse(raw: Vec<String>) -> ParseOutcome {
    let mut opts = Options {
        config: None,
        data_dir: None,
        raw_args: raw.clone(),
    };
    let mut action = Action::Run;
    let mut error: Option<String> = None;

    let mut iter = raw.into_iter();
    let _ = iter.next(); // 跳过程序名

    while let Some(a) = iter.next() {
        match a.as_str() {
            "-h" | "--help" => {
                // --help 优先，但继续扫描以便显示 -c/-D 的"当前值"
                action = Action::Help;
            }
            "-V" | "--version" => {
                // --version 直接中断扫描（不需要解析路径）
                action = Action::Version;
                break;
            }
            "-c" | "--config" => match iter.next() {
                Some(v) => opts.config = Some(PathBuf::from(v)),
                None => {
                    // 缺值无法恢复，覆盖已有 error（因为这是更严重的错误）
                    error = Some(format!("参数 {} 需要一个路径", a));
                    break;
                }
            },
            "-D" | "--data-dir" => match iter.next() {
                Some(v) => opts.data_dir = Some(PathBuf::from(v)),
                None => {
                    error = Some(format!("参数 {} 需要一个目录", a));
                    break;
                }
            },
            _ if a.starts_with("--config=") => {
                let v = &a["--config=".len()..];
                if v.is_empty() {
                    error = Some("--config= 需要非空路径".to_string());
                    break;
                }
                opts.config = Some(PathBuf::from(v));
            }
            _ if a.starts_with("--data-dir=") => {
                let v = &a["--data-dir=".len()..];
                if v.is_empty() {
                    error = Some("--data-dir= 需要非空目录".to_string());
                    break;
                }
                opts.data_dir = Some(PathBuf::from(v));
            }
            _ if a.starts_with('-') => {
                // 未知参数：记录错误，但**不 break**
                // 让后续可能出现的 -h/-V 有机会被识别
                if error.is_none() {
                    error = Some(format!("未知参数: {}", a));
                }
            }
            _ => {
                // 位置参数：同理，记录错误但不 break
                if error.is_none() {
                    error = Some(format!("不接受位置参数: {}", a));
                }
            }
        }
    }

    // -h / -V 优先：即使扫描过程中产生 error，也忽略
    if matches!(action, Action::Help | Action::Version) {
        error = None;
    }

    ParseOutcome {
        action,
        opts,
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        let mut a = vec!["gocryptfs-tui".to_string()];
        a.extend(v.iter().map(|s| s.to_string()));
        a
    }

    #[test]
    fn test_no_args() {
        let r = parse(args(&[]));
        assert!(matches!(r.action, Action::Run));
        assert!(r.opts.config.is_none());
        assert!(r.opts.data_dir.is_none());
        assert!(r.error.is_none());
    }

    #[test]
    fn test_help() {
        assert!(matches!(parse(args(&["-h"])).action, Action::Help));
        assert!(matches!(parse(args(&["--help"])).action, Action::Help));
    }

    #[test]
    fn test_version() {
        assert!(matches!(parse(args(&["-V"])).action, Action::Version));
        assert!(matches!(
            parse(args(&["--version"])).action,
            Action::Version
        ));
    }

    #[test]
    fn test_config_short() {
        let r = parse(args(&["-c", "/tmp/x.yaml"]));
        assert_eq!(
            r.opts.config.as_deref().unwrap().to_str().unwrap(),
            "/tmp/x.yaml"
        );
    }

    #[test]
    fn test_config_long() {
        let r = parse(args(&["--config", "/tmp/x.yaml"]));
        assert_eq!(
            r.opts.config.as_deref().unwrap().to_str().unwrap(),
            "/tmp/x.yaml"
        );
    }

    #[test]
    fn test_config_eq() {
        let r = parse(args(&["--config=/tmp/x.yaml"]));
        assert_eq!(
            r.opts.config.as_deref().unwrap().to_str().unwrap(),
            "/tmp/x.yaml"
        );
    }

    #[test]
    fn test_data_dir() {
        let r = parse(args(&["-D", "/tmp/d"]));
        assert_eq!(
            r.opts.data_dir.as_deref().unwrap().to_str().unwrap(),
            "/tmp/d"
        );

        let r = parse(args(&["--data-dir=/tmp/d"]));
        assert_eq!(
            r.opts.data_dir.as_deref().unwrap().to_str().unwrap(),
            "/tmp/d"
        );
    }

    #[test]
    fn test_unknown() {
        let r = parse(args(&["--bogus"]));
        assert!(r.error.is_some());
    }

    #[test]
    fn test_positional_rejected() {
        let r = parse(args(&["foo"]));
        assert!(r.error.is_some());
    }

    #[test]
    fn test_help_wins_over_missing_config() {
        // --help 即使和 -c 同时出现也不报错
        let r = parse(args(&["-c", "/nonexistent", "--help"]));
        assert!(matches!(r.action, Action::Help));
        assert!(r.error.is_none());
    }

    #[test]
    fn test_help_wins_over_unknown() {
        let r = parse(args(&["--bogus", "--help"]));
        assert!(matches!(r.action, Action::Help));
        assert!(r.error.is_none());
    }

    #[test]
    fn test_version_wins_over_unknown() {
        let r = parse(args(&["--bogus", "-V"]));
        assert!(matches!(r.action, Action::Version));
        assert!(r.error.is_none());
    }

    #[test]
    fn test_config_missing_value() {
        let r = parse(args(&["-c"]));
        assert!(r.error.is_some());
        assert!(matches!(r.action, Action::Run));
    }

    #[test]
    fn test_combined() {
        let r = parse(args(&["-c", "/a.yaml", "-D", "/tmp/d"]));
        assert_eq!(
            r.opts.config.as_deref().unwrap().to_str().unwrap(),
            "/a.yaml"
        );
        assert_eq!(
            r.opts.data_dir.as_deref().unwrap().to_str().unwrap(),
            "/tmp/d"
        );
        assert!(r.error.is_none());
    }

    #[test]
    fn test_raw_args_preserved() {
        let r = parse(args(&["-c", "/a.yaml"]));
        assert_eq!(r.opts.raw_args.len(), 3);
        assert_eq!(r.opts.raw_args[0], "gocryptfs-tui");
    }

    #[test]
    fn test_unknown_before_known_still_reports_error() {
        // 没有 -h/-V 时，未知参数仍要报错
        let r = parse(args(&["--bogus", "-c", "/a.yaml"]));
        assert!(matches!(r.action, Action::Run));
        assert!(r.error.is_some());
        // -c 仍被正确解析
        assert_eq!(
            r.opts.config.as_deref().unwrap().to_str().unwrap(),
            "/a.yaml"
        );
    }

    #[test]
    fn test_multiple_unknown_reports_first() {
        let r = parse(args(&["--bogus1", "--bogus2"]));
        assert!(r.error.is_some());
        assert!(r.error.unwrap().contains("bogus1"));
    }
}
