// i18n.rs — 国际化（i18n）支持
//
// 设计：
//   * 语言代码：`zh-CN`（简体中文，默认）与 `en-US`（English）。
//   * 翻译表按 `key -> 文案` 存放于 `src/i18n/<locale>.rs`，键名按 `<区域>.<语义>`
//     命名（如 `status.ready`、`error.invalid_vault_umount`）。
//   * 文案里的占位符统一用 `{}`（按顺序替换）或 `{name}`（具名替换），
//     `{{` / `}}` 表示字面量花括号。
//   * 调用方式：
//         t!("status.ready")                     -> &'static str
//         t!("output.loaded_vaults", n)          -> String
//         tn!("help.export_markdown", "app" => ..) -> String（具名占位符）
//   * 语言选择优先级（见 `resolve`）：
//         --lang  >  GOCRYPTFS_TUI_LANG  >  配置 language:  >  系统 locale  >  默认 zh-CN
//     系统 locale 按 POSIX 取 LC_ALL > LC_MESSAGES > LANG，再按 GNU 约定让
//     LANGUAGE（冒号分隔偏好列表）优先；C / POSIX 表示不本地化，回落到默认 zh-CN。
//   * TUI 内可用 `L` 键或设置浮层 `l` 键在运行期切换（仅当前会话有效）。

mod en_us;
mod zh_cn;

use std::sync::atomic::{AtomicU8, Ordering};

/// 支持的语言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    /// 简体中文（默认语言，保持历史行为）
    ZhCn,
    /// English (US)
    EnUs,
}

/// 未显式指定语言时使用的默认语言。
pub const DEFAULT_LANG: Lang = Lang::ZhCn;

impl Lang {
    /// 全部受支持的语言（顺序即 TUI 切换顺序）。
    pub const ALL: [Lang; 2] = [Lang::ZhCn, Lang::EnUs];

    /// 规范语言代码（BCP-47 风格）。
    pub fn code(self) -> &'static str {
        match self {
            Lang::ZhCn => "zh-CN",
            Lang::EnUs => "en-US",
        }
    }

    /// 语言的自称（用于设置浮层展示，不做二次翻译）。
    pub fn display_name(self) -> &'static str {
        match self {
            Lang::ZhCn => "简体中文",
            Lang::EnUs => "English",
        }
    }

    /// 简短语言名（用于状态栏切换提示，如 `En/中文[L]`）。
    pub fn short_name(self) -> &'static str {
        match self {
            Lang::ZhCn => "中文",
            Lang::EnUs => "En",
        }
    }

    /// 解析语言代码，宽松匹配常见写法：
    /// `zh` / `zh-CN` / `zh_CN` / `zh-Hans` / `cn` / `en` / `en-US` / `en_US` …
    /// `C` / `POSIX` / 空串 / 未知语言返回 `None`。
    /// 暂无繁体中文表，`zh-Hant` / `zh-TW` 回退到简体中文。
    pub fn from_code(raw: &str) -> Option<Lang> {
        let normalized = raw.trim().to_ascii_lowercase().replace('_', "-");
        if normalized.is_empty() {
            return None;
        }
        // 去掉 .编码 与 @修饰符，得到如 "zh-cn"
        let stem = normalized.split(['.', '@']).next().unwrap_or_default();
        // 主语言子标签，如 "zh"
        let primary = stem.split('-').next().unwrap_or_default();
        match primary {
            "zh" | "cn" | "chs" | "chinese" => Some(Lang::ZhCn),
            "en" | "us" | "english" => Some(Lang::EnUs),
            // POSIX 中立区域：不强制切换，交给后续回退
            "c" | "posix" => None,
            _ => None,
        }
    }

    /// 解析 POSIX locale（如 `en_US.UTF-8`、`zh_CN.UTF-8@euro`）。
    pub fn from_locale(raw: &str) -> Option<Lang> {
        let head = raw.split('.').next()?.split('@').next()?.trim();
        Lang::from_code(head)
    }

    /// 解析 GNU `LANGUAGE` 偏好列表（冒号分隔，取第一个可识别的语言）。
    pub fn from_preference_list(raw: &str) -> Option<Lang> {
        raw.split(':').find_map(Lang::from_locale)
    }

    /// 当前语言的翻译表（按键升序，供二分查找）。
    fn table(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Lang::ZhCn => zh_cn::ENTRIES,
            Lang::EnUs => en_us::ENTRIES,
        }
    }

    /// 下一个语言（循环），用于运行期切换。
    pub fn next(self) -> Lang {
        let idx = Lang::ALL.iter().position(|l| *l == self).unwrap_or(0);
        Lang::ALL[(idx + 1) % Lang::ALL.len()]
    }

    fn index(self) -> u8 {
        match self {
            Lang::ZhCn => 0,
            Lang::EnUs => 1,
        }
    }

    fn from_index(i: u8) -> Lang {
        Lang::ALL.get(i as usize).copied().unwrap_or(DEFAULT_LANG)
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// 设置当前语言。
pub fn set_lang(lang: Lang) {
    CURRENT.store(lang.index(), Ordering::Relaxed);
}

/// 读取当前语言。
pub fn lang() -> Lang {
    Lang::from_index(CURRENT.load(Ordering::Relaxed))
}

/// 在支持的语言之间循环切换，返回切换后的语言。
pub fn toggle_lang() -> Lang {
    let next = lang().next();
    set_lang(next);
    next
}

/// 语言选择：`--lang` > `GOCRYPTFS_TUI_LANG` > 配置 `language:` > 系统 locale > 默认。
pub fn resolve(cli_lang: Option<&str>, config_lang: Option<&str>) -> Lang {
    if let Some(l) = cli_lang.and_then(Lang::from_code) {
        return l;
    }
    if let Some(l) = std::env::var("GOCRYPTFS_TUI_LANG")
        .ok()
        .and_then(|v| Lang::from_code(&v))
    {
        return l;
    }
    if let Some(l) = config_lang.and_then(Lang::from_code) {
        return l;
    }
    if let Some(l) = system_locale() {
        return l;
    }
    DEFAULT_LANG
}

/// 读取系统 locale（POSIX 语义）：
///   1. 有效 locale 取 `LC_ALL` > `LC_MESSAGES` > `LANG`（空值视为未设置）；
///   2. `C` / `POSIX` 表示「不本地化」，直接返回 `None`，并忽略 `LANGUAGE`；
///   3. 其余情况下 `LANGUAGE`（GNU 冒号分隔偏好列表）优先于有效 locale。
pub fn system_locale() -> Option<Lang> {
    let effective = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|var| std::env::var(var).ok().filter(|v| !v.trim().is_empty()))?;
    if is_neutral_locale(&effective) {
        return None;
    }
    if let Ok(list) = std::env::var("LANGUAGE") {
        if let Some(l) = Lang::from_preference_list(&list) {
            return Some(l);
        }
    }
    Lang::from_locale(&effective)
}

/// `C` / `POSIX` 等中立区域：要求使用无翻译的默认行为。
fn is_neutral_locale(raw: &str) -> bool {
    let base = raw
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-");
    base == "c" || base == "posix"
}

fn lookup(table: &'static [(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    table
        .binary_search_by(|(k, _)| k.cmp(&key))
        .ok()
        .map(|i| table[i].1)
}

/// 按语言取文案：缺失时回退到默认语言，再缺失则原样返回键名（便于发现漏配）。
///
/// 键必须是 `&'static str`（实际调用点全是字面量），因此漏配时**不会**产生内存泄漏。
pub fn tr_in(lang: Lang, key: &'static str) -> &'static str {
    if let Some(v) = lookup(lang.table(), key) {
        return v;
    }
    if let Some(v) = lookup(DEFAULT_LANG.table(), key) {
        return v;
    }
    key
}

/// 取当前语言的文案。
pub fn tr(key: &'static str) -> &'static str {
    tr_in(lang(), key)
}

/// 取当前语言文案并做占位符替换（`{}` 按顺序、`{name}` 具名、`{{`/`}}` 转义）。
pub fn trf(key: &'static str, args: &[String]) -> String {
    render(tr(key), args, &[])
}

/// 指定语言 + 位置参数（便于测试，避免修改全局状态）。
pub fn trf_in(lang: Lang, key: &'static str, args: &[String]) -> String {
    render(tr_in(lang, key), args, &[])
}

/// 具名占位符替换，如 `{app}`、`{version}`。
pub fn tr_named(key: &'static str, named: &[(&str, String)]) -> String {
    render(tr(key), &[], named)
}

/// 具名占位符替换（指定语言）。
pub fn tr_named_in(lang: Lang, key: &'static str, named: &[(&str, String)]) -> String {
    render(tr_in(lang, key), &[], named)
}

/// 把任意 `Display` 值转成模板参数（按引用传入，避免调用点发生所有权移动）。
pub fn arg<T: std::fmt::Display + ?Sized>(value: &T) -> String {
    value.to_string()
}

fn render(template: &str, positional: &[String], named: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut next_pos = 0usize;
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let mut name = String::new();
                let mut closed = false;
                for c2 in chars.by_ref() {
                    if c2 == '}' {
                        closed = true;
                        break;
                    }
                    name.push(c2);
                }
                if !closed {
                    out.push('{');
                    out.push_str(&name);
                    continue;
                }
                if name.is_empty() {
                    if let Some(v) = positional.get(next_pos) {
                        out.push_str(v);
                    }
                    next_pos += 1;
                } else if let Ok(idx) = name.parse::<usize>() {
                    if let Some(v) = positional.get(idx) {
                        out.push_str(v);
                    }
                } else if let Some((_, v)) = named.iter().find(|(k, _)| *k == name) {
                    out.push_str(v);
                } else {
                    out.push('{');
                    out.push_str(&name);
                    out.push('}');
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// 供测试与工具使用的键清单（当前语言表）。
pub fn keys() -> impl Iterator<Item = &'static str> {
    lang().table().iter().map(|(k, _)| *k)
}

/// 测试辅助：需要读写全局语言的测试必须持有 `LANG_LOCK`，避免并行测试互相干扰。
#[cfg(test)]
pub mod test_support {
    use super::{set_lang, Lang, DEFAULT_LANG};
    use std::sync::{Mutex, MutexGuard};

    pub static LANG_LOCK: Mutex<()> = Mutex::new(());

    /// 获取语言锁并把语言重置为默认值。
    pub fn lock_lang() -> MutexGuard<'static, ()> {
        let guard = LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_lang(DEFAULT_LANG);
        guard
    }

    /// 在锁保护下执行闭包。
    pub fn with_lang<T>(lang: Lang, f: impl FnOnce() -> T) -> T {
        let _guard = lock_lang();
        set_lang(lang);
        f()
    }
}

// ------------------------------------------------------------
// 宏：t! 位置参数；tn! 具名参数
// ------------------------------------------------------------

/// `t!("key")` / `t!("key", arg1, arg2)`。
#[macro_export]
macro_rules! t {
    ($key:expr) => {
        $crate::i18n::tr($key)
    };
    ($key:expr, $($arg:expr),+ $(,)?) => {
        $crate::i18n::trf($key, &[$($crate::i18n::arg(&$arg)),+])
    };
}

/// `tn!("key", "name" => value, ...)`。
#[macro_export]
macro_rules! tn {
    ($key:expr $(, $name:expr => $value:expr)* $(,)?) => {
        $crate::i18n::tr_named($key, &[$(( $name, $crate::i18n::arg(&$value) )),*])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_translatable_cjk(s: &str) -> bool {
        s.chars().any(|c| {
            let u = c as u32;
            (0x3000..=0x303F).contains(&u)      // CJK 标点
                || (0x3040..=0x30FF).contains(&u) // 假名
                || (0x3400..=0x4DBF).contains(&u) // 扩展 A
                || (0x4E00..=0x9FFF).contains(&u) // 基本汉字
                || (0xF900..=0xFAFF).contains(&u) // 兼容汉字
                || (0xFF01..=0xFF5E).contains(&u) // 全角
        })
    }

    /// 从 Rust 源码里提取字符串字面量的值（跳过注释与字符字面量）。
    fn string_literals(src: &str) -> Vec<String> {
        let b: Vec<char> = src.chars().collect();
        let mut out = Vec::new();
        let mut i = 0usize;
        let n = b.len();
        while i < n {
            if b[i] == '/' && i + 1 < n && b[i + 1] == '/' {
                while i < n && b[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            if b[i] == '/' && i + 1 < n && b[i + 1] == '*' {
                i += 2;
                while i + 1 < n && !(b[i] == '*' && b[i + 1] == '/') {
                    i += 1;
                }
                i = (i + 2).min(n);
                continue;
            }
            if b[i] == '\'' {
                // 字符字面量（'a'、'\n'、'\''、'\x41'、'\u{1F600}'）或生命周期（'a）
                if i + 1 < n && b[i + 1] == '\\' {
                    let mut j = i + 2;
                    if j < n && b[j] == 'x' {
                        j += 3; // \xNN：跳过 'x' 与两位十六进制
                    } else if j < n && b[j] == 'u' {
                        while j < n && b[j] != '}' {
                            j += 1;
                        }
                        j += 1;
                    } else {
                        j += 1;
                    }
                    if j < n && b[j] == '\'' {
                        i = j + 1;
                        continue;
                    }
                    i += 1;
                    continue;
                }
                if i + 2 < n && b[i + 2] == '\'' {
                    i += 3;
                    continue;
                }
                i += 1;
                continue;
            }
            if b[i] == '"' {
                let mut val = String::new();
                i += 1;
                while i < n {
                    match b[i] {
                        '\\' => {
                            i += 1;
                            if i >= n {
                                break;
                            }
                            match b[i] {
                                'n' => val.push('\n'),
                                't' => val.push('\t'),
                                'r' => val.push('\r'),
                                '0' => val.push('\0'),
                                '\\' => val.push('\\'),
                                '"' => val.push('"'),
                                '\'' => val.push('\''),
                                '\n' => {
                                    // 行连接：吞掉换行与后续缩进
                                    while i + 1 < n && (b[i + 1] == ' ' || b[i + 1] == '\t') {
                                        i += 1;
                                    }
                                }
                                other => val.push(other),
                            }
                            i += 1;
                        }
                        '"' => {
                            i += 1;
                            break;
                        }
                        c => {
                            val.push(c);
                            i += 1;
                        }
                    }
                }
                out.push(val);
                continue;
            }
            i += 1;
        }
        out
    }

    /// 源码扫描器本身的自检：注释、字符字面量、生命周期、转义都不得干扰。
    #[test]
    fn string_literal_scanner_is_robust() {
        let src = "
            // 行注释里的 \"字符串\" 不算
            /* 块注释里的 \"字符串\" 也不算 */
            let a = \"中文\";
            let b = 'x';
            let c = '\\x7f';
            let d = '\\'';
            let e = '\"';
            let f: &'static str = \"a\\\"b中文\";
        ";
        let lits = string_literals(src);
        assert_eq!(lits, vec!["中文".to_string(), "a\"b中文".to_string()]);
    }

    #[test]
    fn catalogs_are_sorted_and_unique() {
        for lang in Lang::ALL {
            let table = lang.table();
            for w in table.windows(2) {
                assert!(
                    w[0].0 < w[1].0,
                    "{} 的翻译键未按升序排列或存在重复: {} >= {}",
                    lang.code(),
                    w[0].0,
                    w[1].0
                );
            }
            for (k, v) in table {
                assert!(!k.is_empty(), "{} 存在空键", lang.code());
                assert!(!v.is_empty(), "{} 的键 {} 文案为空", lang.code(), k);
            }
        }
    }

    #[test]
    fn catalogs_have_identical_keys() {
        let zh: Vec<&str> = zh_cn::ENTRIES.iter().map(|(k, _)| *k).collect();
        let en: Vec<&str> = en_us::ENTRIES.iter().map(|(k, _)| *k).collect();
        let missing_en: Vec<&&str> = zh.iter().filter(|k| !en.contains(k)).collect();
        let missing_zh: Vec<&&str> = en.iter().filter(|k| !zh.contains(k)).collect();
        assert!(missing_en.is_empty(), "en-US 缺少键: {:?}", missing_en);
        assert!(missing_zh.is_empty(), "zh-CN 缺少键: {:?}", missing_zh);
    }

    #[test]
    fn catalogs_have_matching_placeholders() {
        fn placeholders(tpl: &str) -> Vec<String> {
            let t = tpl.replace("{{", "").replace("}}", "");
            let mut out = Vec::new();
            let mut rest = t.as_str();
            while let Some(start) = rest.find('{') {
                match rest[start..].find('}') {
                    Some(end) => {
                        out.push(rest[start..start + end + 1].to_string());
                        rest = &rest[start + end + 1..];
                    }
                    None => break,
                }
            }
            out
        }
        for (key, zh) in zh_cn::ENTRIES {
            let en = lookup(en_us::ENTRIES, key).expect("en 表缺少键");
            assert_eq!(
                placeholders(zh),
                placeholders(en),
                "{} 的中英文占位符不一致",
                key
            );
        }
    }

    #[test]
    fn en_catalog_contains_no_cjk() {
        for (k, v) in en_us::ENTRIES {
            assert!(!has_translatable_cjk(v), "en-US 的 {} 仍含中文: {}", k, v);
        }
    }

    #[test]
    fn missing_key_falls_back_to_the_key_itself() {
        // 未配置的键原样返回，便于在界面上直接暴露漏配
        assert_eq!(tr_in(Lang::EnUs, "no.such.key"), "no.such.key");
        assert_eq!(tr_in(Lang::ZhCn, "no.such.key"), "no.such.key");
    }

    #[test]
    fn lang_code_parsing() {
        assert_eq!(Lang::from_code("zh-CN"), Some(Lang::ZhCn));
        assert_eq!(Lang::from_code("zh_CN.UTF-8"), Some(Lang::ZhCn));
        assert_eq!(Lang::from_code("zh"), Some(Lang::ZhCn));
        assert_eq!(Lang::from_code("cn"), Some(Lang::ZhCn));
        // 暂无繁体表：zh-Hant / zh-TW 回退简体
        assert_eq!(Lang::from_code("zh-Hant"), Some(Lang::ZhCn));
        assert_eq!(Lang::from_code("zh_TW.UTF-8"), Some(Lang::ZhCn));
        assert_eq!(Lang::from_code("en-US"), Some(Lang::EnUs));
        assert_eq!(Lang::from_code("English"), Some(Lang::EnUs));
        assert_eq!(Lang::from_code("en_GB.utf8"), Some(Lang::EnUs));
        assert_eq!(Lang::from_code("C"), None);
        assert_eq!(Lang::from_code("POSIX"), None);
        assert_eq!(Lang::from_code(""), None);
        assert_eq!(Lang::from_code("fr-FR"), None);
        assert_eq!(Lang::from_locale("en_US.UTF-8"), Some(Lang::EnUs));
        assert_eq!(Lang::from_locale("zh_CN.UTF-8@euro"), Some(Lang::ZhCn));
    }

    #[test]
    fn language_preference_list() {
        assert_eq!(Lang::from_preference_list("zh_CN:en_US"), Some(Lang::ZhCn));
        assert_eq!(Lang::from_preference_list("fr_FR:en_US"), Some(Lang::EnUs));
        assert_eq!(Lang::from_preference_list("en_US"), Some(Lang::EnUs));
        assert_eq!(Lang::from_preference_list("C:POSIX"), None);
        assert_eq!(Lang::from_preference_list(""), None);
    }

    #[test]
    fn neutral_locales_are_detected() {
        for raw in ["C", "C.UTF-8", "POSIX", "posix", "c.utf8"] {
            assert!(is_neutral_locale(raw), "{} 应视为中立区域", raw);
        }
        for raw in ["zh_CN.UTF-8", "en_US", "zh-CN", ""] {
            assert!(!is_neutral_locale(raw), "{} 不应视为中立区域", raw);
        }
    }

    #[test]
    fn lang_cycle() {
        assert_eq!(Lang::ZhCn.next(), Lang::EnUs);
        assert_eq!(Lang::EnUs.next(), Lang::ZhCn);
    }

    #[test]
    fn positional_and_named_templates() {
        // 用 catalog 中确实存在的键验证渲染逻辑
        assert_eq!(trf_in(Lang::ZhCn, "common.global", &[]), "全局");
        assert_eq!(
            trf_in(Lang::ZhCn, "output.loaded_vaults", &["3".into()]),
            "加载 3 个卷"
        );
        assert_eq!(
            trf_in(Lang::EnUs, "output.loaded_vaults", &["3".into()]),
            trf_in(Lang::EnUs, "output.loaded_vaults", &["3".into()])
        );
        assert_eq!(render("a{{b}}c", &[], &[]), "a{b}c");
        assert_eq!(
            render("{x}-{y}", &[], &[("x", "1".into()), ("y", "2".into())]),
            "1-2"
        );
        assert_eq!(render("{} {}", &["a".into(), "b".into()], &[]), "a b");
        assert_eq!(render("{0} {0}", &["z".into()], &[]), "z z");
        assert_eq!(render("{missing}", &[], &[]), "{missing}");
    }

    #[test]
    fn english_table_differs_from_chinese_for_key_samples() {
        assert_ne!(
            tr_in(Lang::ZhCn, "status.ready"),
            tr_in(Lang::EnUs, "status.ready")
        );
        assert_ne!(
            tr_in(Lang::ZhCn, "page.mount"),
            tr_in(Lang::EnUs, "page.mount")
        );
    }

    /// 所有 `t!` / `tn!` 引用的键都能在翻译表中找到。
    #[test]
    fn all_used_keys_are_defined() {
        // 收集 `t!(` / `tn!(` 之后（允许换行与空白）的第一个字符串字面量
        fn collect_keys(src: &str) -> Vec<String> {
            let chars: Vec<char> = src.chars().collect();
            let mut keys = Vec::new();
            let mut i = 0usize;
            while i < chars.len() {
                let ident_ok = chars[i] == 't'
                    && (i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_'));
                if !ident_ok {
                    i += 1;
                    continue;
                }
                // t! / tn!
                let bang = if chars.get(i + 1) == Some(&'n') && chars.get(i + 2) == Some(&'!') {
                    i + 2
                } else if chars.get(i + 1) == Some(&'!') {
                    i + 1
                } else {
                    i += 1;
                    continue;
                };
                if chars.get(bang + 1) != Some(&'(') {
                    i += 1;
                    continue;
                }
                let mut j = bang + 2;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if chars.get(j) != Some(&'"') {
                    i = bang + 1;
                    continue;
                }
                j += 1;
                let start = j;
                while j < chars.len() && chars[j] != '"' {
                    j += 1;
                }
                keys.push(chars[start..j].iter().collect());
                i = j + 1;
            }
            keys
        }

        let sources = [
            ("main.rs", include_str!("main.rs")),
            ("cli.rs", include_str!("cli.rs")),
        ];
        let mut missing: Vec<String> = Vec::new();
        let mut total = 0usize;
        for (name, src) in sources {
            for key in collect_keys(src) {
                total += 1;
                if lookup(zh_cn::ENTRIES, &key).is_none() && lookup(en_us::ENTRIES, &key).is_none()
                {
                    missing.push(format!("{}: {}", name, key));
                }
            }
        }
        assert!(
            total > 100,
            "解析到的 t! 调用过少（{}），扫描逻辑可能失效",
            total
        );
        assert!(missing.is_empty(), "以下键未定义: {:?}", missing);
    }

    /// 界面文案已全部迁入翻译表：业务源码里不应再残留中文串。
    #[test]
    fn no_hardcoded_cjk_outside_catalogs() {
        let sources = [
            ("main.rs", include_str!("main.rs")),
            ("cli.rs", include_str!("cli.rs")),
        ];
        let mut bad: Vec<String> = Vec::new();
        for (name, src) in sources {
            for lit in string_literals(src) {
                // 纯符号（箭头、emoji、省略号等）不属于需要翻译的范围
                if has_translatable_cjk(&lit) {
                    bad.push(format!("{}: {:?}", name, lit));
                }
            }
        }
        assert!(bad.is_empty(), "以下界面文案未接入 i18n: {:#?}", bad);
    }
}
