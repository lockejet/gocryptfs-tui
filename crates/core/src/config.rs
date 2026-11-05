// config.rs — 配置文件读值与界面语言解析（TUI / Web UI 共用）

pub fn read_setting(config: &str, key: &str, default: &str) -> String {
    if let Ok(content) = std::fs::read_to_string(config) {
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

/// 解析界面语言：`--lang` > `GOCRYPTFS_TUI_LANG` > 配置 `language:` > 系统 locale > 默认。
pub fn resolve_lang(cli_lang: Option<&str>, config: &str) -> crate::i18n::Lang {
    let config_lang = if config.is_empty() {
        String::new()
    } else {
        let primary = read_setting(config, "language", "");
        if primary.is_empty() {
            read_setting(config, "lang", "")
        } else {
            primary
        }
    };
    crate::i18n::resolve(cli_lang, Some(config_lang.as_str()))
}
