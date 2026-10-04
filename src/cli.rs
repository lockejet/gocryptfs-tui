// cli.rs — 命令行参数解析
//
// 手写解析，不引入 clap。
// 支持：-c/--config、-D/--data-dir、-l/--lang、-h/--help、-V/--version、--check-deps
// 支持：--config=path、--data-dir=path、--lang=code
// 规则：
//   - -h/-V 优先于一切（即使和错误参数同时出现也先响应）
//   - 未知参数记录错误但不中断扫描，让后续 -h/-V 有机会生效
//   - -c/-D/-l 缺值时无法恢复，立即中断
//   - 不接受位置参数
//   - 错误以结构化形式返回，由调用方按最终语言渲染（见 `CliError::render`）

use crate::i18n::Lang;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Run,
    Help,
    Version,
    /// 检查运行时依赖（供安装脚本/自检使用）
    CheckDeps,
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub config: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
    /// `--lang` / `-l` 指定的界面语言代码
    pub lang: Option<String>,
    /// 原始参数（含程序名），供 TUI 顶部栏显示
    pub raw_args: Vec<String>,
}

/// 参数解析错误（延迟到语言确定后再渲染文案）。
#[derive(Debug, Clone, PartialEq)]
pub enum CliError {
    /// `-c/--config` 缺少路径
    MissingPath(String),
    /// `-D/--data-dir` 缺少目录
    MissingDir(String),
    /// `--config=` 值为空
    EmptyConfigPath,
    /// `--data-dir=` 值为空
    EmptyDataDir,
    /// `-l/--lang` 缺少语言代码
    MissingLang(String),
    /// `--lang` 值不是受支持的语言
    UnsupportedLang(String),
    /// 未知（以 `-` 开头）参数
    UnknownArg(String),
    /// 不接受的位置参数
    Positional(String),
}

impl CliError {
    /// 按当前语言渲染错误文案。
    pub fn render(&self) -> String {
        match self {
            CliError::MissingPath(a) => t!("cli.missing_value_path", a),
            CliError::MissingDir(a) => t!("cli.missing_value_dir", a),
            CliError::EmptyConfigPath => t!("cli.empty_config_path").to_string(),
            CliError::EmptyDataDir => t!("cli.empty_data_dir").to_string(),
            CliError::MissingLang(a) => t!("cli.missing_value_lang", a),
            CliError::UnsupportedLang(v) => t!("cli.unsupported_lang", v),
            CliError::UnknownArg(a) => t!("cli.unknown_arg", a),
            CliError::Positional(a) => t!("cli.positional_arg", a),
        }
    }
}

pub struct ParseOutcome {
    pub action: Action,
    pub opts: Options,
    /// 仅当 action == Run 时才有意义
    pub error: Option<CliError>,
}

/// 校验语言代码是否受支持（`None` 表示不是有效代码）。
pub fn parse_lang(code: &str) -> Option<Lang> {
    Lang::from_code(code)
}

pub fn parse(raw: Vec<String>) -> ParseOutcome {
    let mut opts = Options {
        config: None,
        data_dir: None,
        lang: None,
        raw_args: raw.clone(),
    };
    let mut action = Action::Run;
    let mut error: Option<CliError> = None;

    let mut iter = raw.into_iter();
    let _ = iter.next(); // 跳过程序名

    while let Some(a) = iter.next() {
        match a.as_str() {
            "-h" | "--help" => {
                // --help 优先，但继续扫描以便显示 -c/-D 的"当前值"
                action = Action::Help;
            }
            "--check-deps" => {
                action = Action::CheckDeps;
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
                    error = Some(CliError::MissingPath(a));
                    break;
                }
            },
            "-D" | "--data-dir" => match iter.next() {
                Some(v) => opts.data_dir = Some(PathBuf::from(v)),
                None => {
                    error = Some(CliError::MissingDir(a));
                    break;
                }
            },
            "-l" | "--lang" => match iter.next() {
                Some(v) => opts.lang = Some(v),
                None => {
                    error = Some(CliError::MissingLang(a));
                    break;
                }
            },
            _ if a.starts_with("--config=") => {
                let v = &a["--config=".len()..];
                if v.is_empty() {
                    error = Some(CliError::EmptyConfigPath);
                    break;
                }
                opts.config = Some(PathBuf::from(v));
            }
            _ if a.starts_with("--data-dir=") => {
                let v = &a["--data-dir=".len()..];
                if v.is_empty() {
                    error = Some(CliError::EmptyDataDir);
                    break;
                }
                opts.data_dir = Some(PathBuf::from(v));
            }
            _ if a.starts_with("--lang=") => {
                let v = &a["--lang=".len()..];
                if v.is_empty() {
                    error = Some(CliError::MissingLang(a));
                    break;
                }
                opts.lang = Some(v.to_string());
            }
            _ if a.starts_with('-') => {
                // 未知参数：记录错误，但**不 break**
                // 让后续可能出现的 -h/-V 有机会被识别
                if error.is_none() {
                    error = Some(CliError::UnknownArg(a));
                }
            }
            _ => {
                // 位置参数：同理，记录错误但不 break
                if error.is_none() {
                    error = Some(CliError::Positional(a));
                }
            }
        }
    }

    // 语言代码校验（错误仍可被后面的 -h/-V 覆盖）
    if error.is_none() {
        if let Some(code) = &opts.lang {
            if parse_lang(code).is_none() {
                error = Some(CliError::UnsupportedLang(code.clone()));
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
        assert!(r.error.unwrap().render().contains("bogus1"));
    }

    #[test]
    fn test_lang_short_and_long() {
        for argv in [
            vec!["-l", "en-US"],
            vec!["--lang", "en-US"],
            vec!["--lang=en-US"],
        ] {
            let r = parse(args(&argv));
            assert_eq!(r.opts.lang.as_deref(), Some("en-US"));
            assert!(r.error.is_none());
        }
    }

    #[test]
    fn test_lang_missing_value() {
        let r = parse(args(&["--lang"]));
        assert!(matches!(r.error, Some(CliError::MissingLang(_))));
        assert!(matches!(r.action, Action::Run));

        let r = parse(args(&["--lang="]));
        assert!(matches!(r.error, Some(CliError::MissingLang(_))));
    }

    #[test]
    fn test_lang_unsupported() {
        let r = parse(args(&["--lang", "fr-FR"]));
        assert!(matches!(r.error, Some(CliError::UnsupportedLang(_))));
    }

    #[test]
    fn test_help_wins_over_bad_lang() {
        let r = parse(args(&["--lang", "fr-FR", "--help"]));
        assert!(matches!(r.action, Action::Help));
        assert!(r.error.is_none());
    }

    #[test]
    fn test_error_render_uses_current_language() {
        // 默认语言为简体中文；文案与 i18n 表中的渲染结果一致
        let _guard = crate::i18n::test_support::lock_lang();
        let r = parse(args(&["--bogus"]));
        let msg = r.error.unwrap().render();
        let expected = crate::i18n::trf_in(
            crate::i18n::DEFAULT_LANG,
            "cli.unknown_arg",
            &["--bogus".to_string()],
        );
        assert_eq!(msg, expected);
        assert!(msg.contains("bogus"), "{}", msg);
    }
}
