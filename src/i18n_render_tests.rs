// i18n_render_tests.rs — 渲染层 i18n 测试（不参与生产构建）
//
// 通过 ratatui 的 TestBackend 直接渲染各界面/浮层，验证：
//   * 界面文案随语言切换；
//   * 设置浮层「语言」行与帮助浮层「切换语言」说明存在；
//   * 导出帮助（Markdown）随语言本地化。
//
// 注：本文件含有中文断言，故刻意不纳入 tools/i18n_check.py 与
// `no_hardcoded_cjk_outside_catalogs` 的源码扫描范围。

use crate::i18n::test_support::lock_lang;
use crate::*;
use ratatui::backend::TestBackend;

fn test_app() -> App {
    let dir = std::env::temp_dir().join(format!("gocryptfs-tui-i18n-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    App::new(
        String::new(), // 空配置：跳过 gocryptfs-cli 调用
        dir.clone(),
        dir.join("app.log.jsonl"),
        dir.join("history.jsonl"),
        dir.join("HELP.md"),
    )
}

/// 去掉全部空白：宽字符在缓冲区里会占用一个跟随的空白单元，比较前先压缩。
fn nows(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 忽略空白差异的子串匹配。
fn has(haystack: &str, needle: &str) -> bool {
    nows(haystack).contains(&nows(needle))
}

/// 按显示宽度把缓冲区还原成文本行：宽字符只输出一次（跳过其后的占位单元），
/// 这样打印出来与真实终端的列对齐一致。
fn render_lines(app: &mut App) -> Vec<String> {
    render_lines_at(app, 140, 40)
}

/// 指定终端尺寸渲染，便于验证窄终端下的排版。
fn render_lines_at(app: &mut App, width: u16, height: u16) -> Vec<String> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui(f, app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        let mut x = 0u16;
        while x < buffer.area.width {
            let symbol = buffer.content()[buffer.index_of(x, y)].symbol();
            let w = Line::from(symbol).width();
            if w == 0 {
                x += 1;
                continue;
            }
            line.push_str(symbol);
            x += w as u16;
        }
        lines.push(line.trim_end().to_string());
    }
    lines
}

/// 把当前 App 渲染到 140x40 的测试后端，返回整屏文本。
fn render_text(app: &mut App) -> String {
    render_lines(app).join("\n")
}

#[test]
fn renders_localized_ui_in_both_languages() {
    let _guard = lock_lang();

    // 简体中文：整个 App 在中文环境下初始化
    i18n::set_lang(i18n::Lang::ZhCn);
    let mut app = test_app();
    let zh = render_text(&mut app);
    assert!(has(&zh, "挂载/卸载"), "中文标签缺失:\n{}", zh);
    assert!(has(&zh, "卷列表"), "中文列表标题缺失");
    assert!(has(&zh, "就绪"), "中文状态缺失");

    // English：另建一个 App，确保初始状态文案也是英文
    i18n::set_lang(i18n::Lang::EnUs);
    let mut app = test_app();
    let en = render_text(&mut app);
    assert!(has(&en, "Mount/unmount"), "英文标签缺失:\n{}", en);
    assert!(has(&en, "Vault list"), "英文列表标题缺失:\n{}", en);
    assert!(has(&en, "Ready"), "英文状态缺失:\n{}", en);
    assert!(!has(&en, "挂载"), "英文界面残留中文");
}

#[test]
fn overlays_and_runtime_switch_are_localized() {
    let _guard = lock_lang();
    let mut app = test_app();

    // 中文设置浮层 + 运行期切换
    i18n::set_lang(i18n::Lang::ZhCn);
    app.scope_names = vec![t!("common.global").to_string()];
    app.overlay = Some(Overlay::Settings {
        tab: SettingsTab::Gocryptfs,
        scope_index: 0,
    });
    let zh_settings = render_text(&mut app);
    assert!(
        has(&zh_settings, "语言: 简体中文"),
        "设置浮层语言行缺失:\n{}",
        zh_settings
    );
    assert!(has(&zh_settings, "语言[l]"), "设置浮层快捷键提示缺失");

    app.switch_lang();
    assert_eq!(i18n::lang(), i18n::Lang::EnUs);
    assert_eq!(app.scope_names[0], "Global");
    let en_settings = render_text(&mut app);
    assert!(
        has(&en_settings, "Language: English"),
        "切换后英文设置浮层缺失:\n{}",
        en_settings
    );

    // 帮助浮层（英文）
    app.overlay = Some(Overlay::Help);
    let en_help = render_text(&mut app);
    assert!(has(&en_help, "Global keys"), "英文帮助浮层缺失");
    assert!(
        has(&en_help, "Switch language[L]"),
        "英文帮助缺少语言键说明"
    );

    // 历史浮层（中文）
    i18n::set_lang(i18n::Lang::ZhCn);
    app.overlay = Some(Overlay::History);
    let zh_history = render_text(&mut app);
    assert!(
        has(&zh_history, "📜 历史记录"),
        "中文历史浮层缺失:\n{}",
        zh_history
    );
    assert!(has(&zh_history, "无匹配记录"), "中文历史空态缺失");
}

#[test]
fn help_markdown_is_localized() {
    let _guard = lock_lang();
    let app = test_app();

    i18n::set_lang(i18n::Lang::ZhCn);
    let zh = app.help_markdown();
    assert!(
        zh.starts_with("# gocryptfs-tui 帮助"),
        "中文帮助导出标题错误"
    );
    assert!(zh.contains("-l, --lang <CODE>"), "中文帮助缺少 --lang");
    assert!(zh.contains("## 全局键"), "中文帮助缺少全局键章节");

    i18n::set_lang(i18n::Lang::EnUs);
    let en = app.help_markdown();
    assert!(
        en.starts_with("# gocryptfs-tui Help"),
        "英文帮助导出标题错误"
    );
    assert!(en.contains("## Global keys"), "英文帮助缺少全局键章节");
    assert!(!en.contains("帮助"), "英文帮助残留中文");
}

// ------------------------------------------------------------
// 回归测试：运行期切换语言不得留下旧语言的缓存（对抗性评审发现）
// ------------------------------------------------------------

/// 切换语言后，目录区（加载时已本地化的缓存）必须按当前语言重建。
#[test]
fn runtime_switch_refreshes_cached_dir_view() {
    let _guard = lock_lang();
    i18n::set_lang(i18n::Lang::ZhCn);
    let mut app = test_app();

    let dir = std::env::temp_dir();
    let path = dir.display().to_string();
    app.page = Page::Mount;
    app.vaults = vec![Vault {
        id: 1,
        name: "t".to_string(),
        path: path.clone(),
        mount_point: path.clone(),
        mounted: true,
        locked: false,
        valid: true,
    }];
    app.vault_list_state.select(Some(0));
    // 伪造一份「旧语言」的目录区缓存
    app.dir_view = DirView {
        sections: vec![DirSection {
            title: "【加密路径】stale".to_string(),
            lines: vec!["（未挂载）".to_string()],
        }],
        scroll: 0,
        mode: "ls".to_string(),
    };

    app.switch_lang();
    assert_eq!(i18n::lang(), i18n::Lang::EnUs);
    assert!(!app.dir_view.sections.is_empty());
    let expected = i18n::trf_in(
        i18n::Lang::EnUs,
        "dir.cipher_path",
        std::slice::from_ref(&path),
    );
    assert_eq!(
        app.dir_view.sections[0].title, expected,
        "目录区标题未随语言重建: {}",
        app.dir_view.sections[0].title
    );
    let stale = i18n::trf_in(i18n::Lang::ZhCn, "dir.cipher_path", &[path]);
    assert_ne!(app.dir_view.sections[0].title, stale);
}

/// 任务描述在渲染时取当前语言，切换后状态栏不应残留旧语言。
#[test]
fn task_label_follows_language_switch() {
    let _guard = lock_lang();
    i18n::set_lang(i18n::Lang::ZhCn);
    let mut app = test_app();
    let (_tx, rx) = std::sync::mpsc::channel();
    app.task = Some(BackgroundTask {
        kind: TaskKind::Mount("/mnt/x".to_string()),
        action: "mount".to_string(),
        status: TaskStatus::Running,
        pid: None,
        rx,
        started_at: std::time::Instant::now(),
    });

    let zh = render_text(&mut app);
    assert!(has(&zh, "运行中: 挂载 /mnt/x"), "中文任务描述缺失:\n{}", zh);

    app.switch_lang();
    let en = render_text(&mut app);
    assert!(
        has(&en, "Running: Mount /mnt/x"),
        "任务描述未随语言切换:\n{}",
        en
    );
}

/// 帮助/设置/历史浮层内按 `L`（设置内还支持 `l`）应能切换语言且浮层保持打开。
#[test]
fn language_key_works_inside_overlays() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let _guard = lock_lang();
    let mut app = test_app();

    // 帮助浮层
    i18n::set_lang(i18n::Lang::ZhCn);
    app.overlay = Some(Overlay::Help);
    handle_key(&mut app, KeyCode::Char('L'), KeyModifiers::empty());
    assert_eq!(i18n::lang(), i18n::Lang::EnUs);
    assert!(matches!(app.overlay, Some(Overlay::Help)), "帮助浮层被关闭");

    // 帮助浮层内可再次切回
    handle_key(&mut app, KeyCode::Char('L'), KeyModifiers::empty());
    assert_eq!(i18n::lang(), i18n::Lang::ZhCn);

    // 设置浮层：l 与 L 都有效
    app.overlay = Some(Overlay::Settings {
        tab: SettingsTab::Gocryptfs,
        scope_index: 0,
    });
    handle_key(&mut app, KeyCode::Char('l'), KeyModifiers::empty());
    assert_eq!(i18n::lang(), i18n::Lang::EnUs);
    assert!(matches!(
        app.overlay,
        Some(Overlay::Settings {
            tab: SettingsTab::Gocryptfs,
            ..
        })
    ));
    handle_key(&mut app, KeyCode::Char('L'), KeyModifiers::empty());
    assert_eq!(i18n::lang(), i18n::Lang::ZhCn);

    // 历史浮层
    app.overlay = Some(Overlay::History);
    handle_key(&mut app, KeyCode::Char('L'), KeyModifiers::empty());
    assert_eq!(i18n::lang(), i18n::Lang::EnUs);
    assert!(matches!(app.overlay, Some(Overlay::History)));

    // 设置浮层分类标签也已本地化：gocryptfs 为专名，权限为译文
    i18n::set_lang(i18n::Lang::ZhCn);
    app.overlay = Some(Overlay::Settings {
        tab: SettingsTab::Gocryptfs,
        scope_index: 0,
    });
    let zh_g = render_text(&mut app);
    assert!(has(&zh_g, "[g]gocryptfs"), "gocryptfs 分类缺失");
    app.overlay = Some(Overlay::Settings {
        tab: SettingsTab::Perm,
        scope_index: 0,
    });
    let zh_p = render_text(&mut app);
    assert!(has(&zh_p, "[p]权限"), "权限分类缺失:\n{}", zh_p);
}

/// 人工查看界面（默认忽略）：
///   cargo test --offline dump_screens -- --ignored --nocapture
#[test]
#[ignore]
fn dump_screens() {
    let _guard = lock_lang();
    let vault_a = Vault {
        id: 1,
        name: "my_vault".to_string(),
        path: "/srv/data/.cipher.d/my_vault".to_string(),
        mount_point: "/srv/data/my_vault".to_string(),
        mounted: true,
        locked: false,
        valid: true,
    };
    let vault_b = Vault {
        id: 2,
        name: "Work".to_string(),
        path: "/srv/data/.cipher.d/Work".to_string(),
        mount_point: "/srv/data/Work".to_string(),
        mounted: false,
        locked: true,
        valid: false,
    };
    for (code, lang) in [("zh-CN", i18n::Lang::ZhCn), ("en-US", i18n::Lang::EnUs)] {
        i18n::set_lang(lang);
        let mut app = test_app();
        // 用有代表性的路径，便于人工核对顶部栏排版
        app.config = "/home/user/.config/gocryptfs-tui/demo.yaml".to_string();
        app.vaults = vec![vault_a.clone(), vault_b.clone()];
        app.vault_list_state.select(Some(0));
        app.pending = vec!["/srv/data/photos".to_string()];
        app.pending_list_state.select(Some(0));
        app.update_scope_names();

        println!("\n########## {code} · TAB1（列表 + 详情） ##########");
        for l in render_lines(&mut app) {
            println!("|{l}");
        }

        app.page = Page::Create;
        println!("\n########## {code} · TAB2 详情（待处理目录） ##########");
        for l in render_lines(&mut app) {
            println!("|{l}");
        }

        app.wizard = Some(Wizard {
            direction: WizardDirection::Create,
            step: WizardStep::ConfigPaths,
            source: "/srv/data/photos".to_string(),
            name: "photos".to_string(),
            cipher: "/srv/data/.cipher.d/photos".to_string(),
            tmp_mount: "/srv/data/photos.mount_tmp".to_string(),
            vault_name: String::new(),
            dry_run: false,
            keep_source: true,
            keep_source_locked: false,
            restore: true,
            restore_locked: true,
            delete_cipher: false,
            delete_cipher_locked: true,
            password: String::new(),
            password_confirm: String::new(),
            confirming: false,
            delete_confirm_text: String::new(),
            progress: None,
            checks: Default::default(),
            error: None,
            selected: 0,
        });
        println!("\n########## {code} · 创建向导 step1 ##########");
        for l in render_lines(&mut app) {
            println!("|{l}");
        }
        app.wizard = None;

        app.overlay = Some(Overlay::Settings {
            tab: SettingsTab::Gocryptfs,
            scope_index: 0,
        });
        println!("\n########## {code} · 设置浮层 ##########");
        for l in render_lines(&mut app) {
            println!("|{l}");
        }
    }
}

// ------------------------------------------------------------
// 对齐回归：标签列宽必须在渲染时按显示宽度计算（翻译表不得内嵌对齐空格）
// ------------------------------------------------------------

/// 断言一组标签在屏幕上都被补齐到同一列。
fn assert_label_group_aligned(lines: &[String], keys: &[&'static str]) {
    let width = label_column(keys);
    for key in keys {
        let label = i18n::tr(key);
        assert_eq!(
            label,
            label.trim_end(),
            "键 {} 的译文不应包含行尾对齐空格: {:?}",
            key,
            label
        );
        let padded = pad_label(label, width);
        assert!(
            lines.iter().any(|l| l.contains(&padded)),
            "键 {} 未按 {} 列补齐（期望 {:?}）:\n{}",
            key,
            width,
            padded,
            lines.join("\n")
        );
    }
    let widths: Vec<usize> = keys
        .iter()
        .map(|k| Line::from(pad_label(i18n::tr(k), width)).width())
        .collect();
    assert!(
        widths.iter().all(|w| *w == widths[0]),
        "列宽不一致: {:?}",
        widths
    );
}

fn sample_vault() -> Vault {
    Vault {
        id: 1,
        name: "my_vault".to_string(),
        path: "/srv/data/.cipher.d/my_vault".to_string(),
        mount_point: "/srv/data/my_vault".to_string(),
        mounted: true,
        locked: false,
        valid: true,
    }
}

fn sample_wizard(direction: WizardDirection) -> Wizard {
    Wizard {
        direction,
        step: WizardStep::ConfigPaths,
        source: "/srv/data/photos".to_string(),
        name: "photos".to_string(),
        cipher: "/srv/data/.cipher.d/photos".to_string(),
        tmp_mount: "/srv/data/photos.mount_tmp".to_string(),
        vault_name: "photos".to_string(),
        dry_run: false,
        keep_source: true,
        keep_source_locked: false,
        restore: true,
        restore_locked: true,
        delete_cipher: false,
        delete_cipher_locked: true,
        password: String::new(),
        password_confirm: String::new(),
        confirming: false,
        delete_confirm_text: String::new(),
        progress: None,
        checks: Default::default(),
        error: None,
        selected: 0,
    }
}

#[test]
fn label_columns_align_in_both_languages() {
    let _guard = lock_lang();
    const VAULT_GROUP: &[&str] = &[
        "detail.label_id",
        "detail.label_name",
        "detail.label_cipher_path",
        "detail.label_mount_point",
        "detail.label_status",
        "detail.label_protection",
        "detail.label_valid",
    ];
    const PENDING_GROUP: &[&str] = &[
        "detail.label_task_name",
        "detail.label_source",
        "detail.label_cipher_dir",
        "detail.label_tmp_mount",
        "detail.label_target_mount",
        "detail.label_status",
    ];
    const WIZARD_CREATE: &[&str] = &[
        "wizard.label_source",
        "wizard.label_cipher_dir",
        "wizard.label_tmp_mount",
    ];
    const WIZARD_REMOVE: &[&str] = &[
        "wizard.label_vault",
        "wizard.label_cipher_dir",
        "wizard.label_mount_point",
    ];
    const CONFIRM: &[&str] = &["confirm.vault", "confirm.mount_point"];

    for lang in [i18n::Lang::ZhCn, i18n::Lang::EnUs] {
        i18n::set_lang(lang);
        let mut app = test_app();
        app.vaults = vec![sample_vault()];
        app.vault_list_state.select(Some(0));
        app.pending = vec!["/srv/data/photos".to_string()];
        app.pending_list_state.select(Some(0));
        app.update_scope_names();

        // TAB1：卷详情
        let lines = render_lines(&mut app);
        assert_label_group_aligned(&lines, VAULT_GROUP);

        // TAB2：待处理目录详情
        app.page = Page::Create;
        let lines = render_lines(&mut app);
        assert_label_group_aligned(&lines, PENDING_GROUP);

        // 创建向导 step1
        app.wizard = Some(sample_wizard(WizardDirection::Create));
        let lines = render_lines(&mut app);
        assert_label_group_aligned(&lines, WIZARD_CREATE);

        // 删除向导 step1
        app.wizard = Some(sample_wizard(WizardDirection::Remove));
        let lines = render_lines(&mut app);
        assert_label_group_aligned(&lines, WIZARD_REMOVE);
        app.wizard = None;

        // 卸载确认浮层
        app.page = Page::Mount;
        app.overlay = Some(Overlay::ConfirmUmount { vault_index: 0 });
        let lines = render_lines(&mut app);
        assert_label_group_aligned(&lines, CONFIRM);
        app.overlay = None;

        // 值列一致：详情面板内所有标签后的值必须从同一列开始
        let width = label_column(VAULT_GROUP);
        let starts: Vec<usize> = VAULT_GROUP
            .iter()
            .map(|key| {
                let label = pad_label(i18n::tr(key), width);
                let needle = format!("│{label}");
                let line = lines
                    .iter()
                    .find(|l| l.contains(&needle))
                    .unwrap_or_else(|| panic!("找不到 {} 所在行:\n{}", key, lines.join("\n")));
                // 用显示宽度而不是字节偏移（中文一个字 3 字节但占 2 列）
                Line::from(&line[..line.find(&needle).unwrap() + needle.len()]).width()
            })
            .collect();
        assert!(
            starts.iter().all(|s| *s == starts[0]),
            "详情面板值起始列不一致: {:?}",
            starts
        );
    }
}

/// TUI 启动 CLI 子进程时必须把当前界面语言通过 GOCRYPTFS_LANG 传下去，
/// 否则输出区里 CLI 的提示会停留在另一门语言。
#[test]
fn cli_child_process_receives_ui_language() {
    let _guard = lock_lang();
    let dir = std::env::temp_dir();
    for (lang, expected) in [(i18n::Lang::EnUs, "en-US"), (i18n::Lang::ZhCn, "zh-CN")] {
        i18n::set_lang(lang);
        let rx = spawn_cli_task_with(
            "sh",
            i18n::lang().code(),
            vec![
                "-c".to_string(),
                "printf '%s\\n' \"$GOCRYPTFS_LANG\"".to_string(),
            ],
            None,
            &dir.join("i18n-child.log.jsonl"),
            &dir.join("i18n-child.hist.jsonl"),
        );
        let mut got: Vec<String> = Vec::new();
        while let Ok(evt) = rx.recv() {
            if let CliEvent::Stdout(line) = evt {
                got.push(line);
            }
        }
        assert_eq!(got, vec![expected.to_string()], "子进程收到的语言不正确");
    }
}

/// 旧版 Shell 后端不认识 `--lang`，TUI 必须能探测出来并在输出区提示。
#[test]
fn detects_cli_without_lang_support() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("gocryptfs-tui-cli-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let new_cli = dir.join("new-cli");
    let old_cli = dir.join("old-cli");
    std::fs::write(
        &new_cli,
        "#!/bin/sh\necho '  -l, --lang CODE    UI language'\n",
    )
    .unwrap();
    std::fs::write(&old_cli, "#!/bin/sh\necho 'gocryptfs-cli - 用法'\n").unwrap();
    for f in [&new_cli, &old_cli] {
        let mut perms = std::fs::metadata(f).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(f, perms).unwrap();
    }
    assert!(
        cli_supports_lang(new_cli.to_str().unwrap()),
        "应识别出支持 --lang"
    );
    assert!(
        !cli_supports_lang(old_cli.to_str().unwrap()),
        "应识别出旧版不支持 --lang"
    );
    // 探测失败（命令不存在）时不误报
    assert!(cli_supports_lang("/nonexistent/gocryptfs-cli-xyz"));
}

// ------------------------------------------------------------
// 界面调整回归：去掉 o/打开、顶栏全路径、底栏无背景色
// ------------------------------------------------------------

/// `o`（打开挂载点）功能已移除：按键不再产生任何状态变化。
#[test]
fn open_key_is_removed() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let _guard = lock_lang();
    i18n::set_lang(i18n::Lang::ZhCn);
    let mut app = test_app();
    app.vaults = vec![sample_vault()];
    app.vault_list_state.select(Some(0));
    app.page = Page::Mount;
    app.focus = Focus::List;
    app.status = "SENTINEL".to_string();
    handle_key(&mut app, KeyCode::Char('o'), KeyModifiers::empty());
    assert_eq!(app.status, "SENTINEL", "o 键仍然触发了动作");
}

/// 取出字符串尾部 n 个字符（按显示宽度近似），用于验证"保留尾部"截断。
fn tail_of(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars[chars.len().saturating_sub(n)..].iter().collect()
}

/// 顶栏第 1 行：Bin + CLI；第 2 行：Config + Data；均为完整路径（不做 ~ 缩写）。
#[test]
fn top_bar_shows_bin_cli_config_and_data() {
    let _guard = lock_lang();
    let mut app = test_app();
    app.config = "/home/user/.config/gocryptfs-tui/config.local.yaml".to_string();
    // 用足够宽的终端：开发构建的版本串形如 `v0.2.0-1-g<sha>-dirty`，
    // 140 列时两条路径会按比例截断，无法断言"完整路径"（截断行为见下一个用例）。
    let lines = render_lines_at(&mut app, 240, 24);

    // 第 1 行：Bin 与 CLI 的完整路径
    assert!(
        lines[0].contains(&bin_path()),
        "顶栏 Bin 未显示完整路径: {}",
        lines[0]
    );
    assert!(
        lines[0].contains(&resolved_cli_path()),
        "顶栏 CLI 未显示完整路径: {}",
        lines[0]
    );
    // 第 2 行：配置与数据的完整路径
    assert!(
        lines[1].contains(&app.config),
        "顶栏配置未显示完整路径: {}",
        lines[1]
    );
    assert!(
        lines[1].contains(&app.data_dir.to_string_lossy().to_string()),
        "顶栏数据目录未显示完整路径: {}",
        lines[1]
    );
    for line in [&lines[0], &lines[1]] {
        assert!(!line.contains("~/"), "顶栏不应使用 ~ 缩写: {}", line);
    }
    // 不再显示日志/历史文件
    assert!(
        !lines[1].contains("jsonl"),
        "顶栏不应再显示日志/历史文件: {}",
        lines[1]
    );
    // 顶栏只占两行：第 3 行应是页签
    assert!(
        has(&lines[2], "挂载/卸载") || has(&lines[2], "Mount/unmount"),
        "顶栏高度不是 2 行: {}",
        lines[2]
    );
}

/// 窄终端下按比例分配并保留尾部：宽屏完整，窄屏至少能看到文件名/目录尾部。
#[test]
fn top_bar_paths_keep_tail_on_narrow_terminals() {
    let _guard = lock_lang();
    let mut app = test_app();
    app.config = "/home/user/.config/gocryptfs-tui/config.local.yaml".to_string();

    // 宽屏：两个路径都完整
    let wide = render_lines_at(&mut app, 140, 24);
    assert!(
        wide[1].contains(&app.config),
        "140 列配置被截断: {}",
        wide[1]
    );
    assert!(
        wide[1].contains(&app.data_dir.to_string_lossy().to_string()),
        "140 列数据目录被截断: {}",
        wide[1]
    );

    // 窄屏：标签仍在，且两个路径的尾部都可见（保留了各自文件名）
    for width in [80u16, 100] {
        let lines = render_lines_at(&mut app, width, 24);
        assert!(
            has(&lines[1], "Config:") || has(&lines[1], "配置:"),
            "{} 缺少标签: {}",
            width,
            lines[1]
        );
        assert!(
            has(&lines[1], &tail_of(&app.config, 12)),
            "宽 {} 时配置尾部不可见: {}",
            width,
            lines[1]
        );
        assert!(
            has(&lines[1], &tail_of(&app.data_dir.to_string_lossy(), 10)),
            "宽 {} 时数据目录尾部不可见: {}",
            width,
            lines[1]
        );
        assert!(
            Line::from(lines[1].as_str()).width() <= width as usize,
            "宽 {} 时顶栏第 2 行超宽: {}",
            width,
            lines[1]
        );
    }
}

/// 右上角版本号必须完整可见——早期英文标签（Command: ）比中文宽，
/// 硬编码宽度会把 "v0.1.2" 裁成 "v0."。
#[test]
fn top_right_version_is_not_clipped() {
    let _guard = lock_lang();
    for lang in [i18n::Lang::ZhCn, i18n::Lang::EnUs] {
        for width in [80u16, 100, 140] {
            i18n::set_lang(lang);
            let mut app = test_app();
            let backend = TestBackend::new(width, 24);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal.draw(|f| ui(f, &mut app)).unwrap();
            let buffer = terminal.backend().buffer();
            let mut line1 = String::new();
            let mut x = 0u16;
            while x < width {
                let symbol = buffer.content()[buffer.index_of(x, 0)].symbol();
                let w = Line::from(symbol).width();
                if w == 0 {
                    x += 1;
                    continue;
                }
                line1.push_str(symbol);
                x += w as u16;
            }
            assert!(
                line1.trim_end().ends_with(&short_version()),
                "{} 宽 {} 时版本被裁剪: {:?}",
                lang.code(),
                width,
                line1
            );
            // 右对齐：版本号最后一个字符应落在最后一列
            let last = buffer.content()[buffer.index_of(width - 1, 0)].symbol();
            let ver_tail: String = short_version()
                .chars()
                .last()
                .map(String::from)
                .unwrap_or_default();
            assert_eq!(
                last.trim_end(),
                ver_tail,
                "{} 宽 {} 时版本未右对齐: {:?}",
                lang.code(),
                width,
                line1
            );
        }
    }
}

/// 底部 GLOBAL 行必须带语言切换提示 `En/中文[L]`。
#[test]
fn status_bar_shows_language_switch_hint() {
    let _guard = lock_lang();
    for lang in [i18n::Lang::ZhCn, i18n::Lang::EnUs] {
        i18n::set_lang(lang);
        let mut app = test_app();
        let lines = render_lines(&mut app);
        let global = lines
            .iter()
            .find(|l| l.contains(i18n::tr("statusbar.global")))
            .unwrap_or_else(|| panic!("找不到 GLOBAL 行:\n{}", lines.join("\n")));
        assert!(
            has(global, "En/中文[L]"),
            "{} 的 GLOBAL 行缺少语言提示: {}",
            lang.code(),
            global
        );
        // 窄终端（60 列）下提示也必须可见——因此把它放在 GLOBAL 行首而非行尾
        let narrow = render_lines_at(&mut app, 60, 24);
        let global = narrow
            .iter()
            .find(|l| l.contains(i18n::tr("statusbar.global")))
            .unwrap_or_else(|| panic!("窄终端下找不到 GLOBAL 行"));
        assert!(
            has(global, "En/中文[L]"),
            "{} 窄终端下语言提示被裁剪: {}",
            lang.code(),
            global
        );
    }
}

/// 底部栏不再设置背景色（与顶部及其他区域一致）。
#[test]
fn status_bar_has_no_background_color() {
    let _guard = lock_lang();
    let mut app = test_app();
    let backend = TestBackend::new(140, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui(f, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    // 布局最后 3 行是状态栏
    for y in 37..40 {
        for x in 0..140 {
            let cell = &buffer.content()[buffer.index_of(x, y)];
            assert_eq!(
                cell.bg,
                Color::Reset,
                "底栏 ({x},{y}) 仍带背景色: {:?}",
                cell.bg
            );
        }
    }
}

/// 配置/数据目录路径必须解析成绝对路径后再展示：
/// `-c demo.yaml` 这类相对路径此前会在顶栏原样显示成文件名。
#[test]
fn paths_are_absolutized_for_display() {
    let cwd = std::env::current_dir().unwrap();
    // 绝对路径保持不变
    assert_eq!(
        absolute_path("/etc/gocryptfs-tui/config.yaml"),
        "/etc/gocryptfs-tui/config.yaml"
    );
    // 相对路径基于当前工作目录
    assert_eq!(
        absolute_path("demo.yaml"),
        cwd.join("demo.yaml").to_string_lossy().to_string()
    );
    // ~ 展开
    if let Some(home) = std::env::var_os("HOME") {
        assert_eq!(
            absolute_path("~/cfg.ini"),
            std::path::PathBuf::from(home)
                .join("cfg.ini")
                .to_string_lossy()
                .to_string()
        );
    }
    assert_eq!(absolute_path(""), "");
    // resolve_config 对相对路径同样解析成绝对路径（不调用后端）
    assert_eq!(
        resolve_config(Some(std::path::PathBuf::from("demo.yaml"))),
        cwd.join("demo.yaml").to_string_lossy().to_string()
    );
    // 顶栏展示的就是解析后的绝对路径
    let mut app = test_app();
    app.config = cwd.join("demo.yaml").to_string_lossy().to_string();
    let lines = render_lines(&mut app);
    assert!(
        lines[1].contains(&app.config) && app.config.starts_with('/'),
        "顶栏配置未显示绝对路径: {}",
        lines[1]
    );
}

/// TAB1：`m` 只挂载——已挂载的卷给灰色提示，不启动任务、不弹卸载确认（toggle 已取消）。
#[test]
fn m_key_only_mounts_and_hints_dim_when_already_mounted() {
    let _guard = lock_lang();
    for lang in [i18n::Lang::ZhCn, i18n::Lang::EnUs] {
        i18n::set_lang(lang);
        let mut app = test_app();
        app.vaults = vec![sample_vault()]; // mounted: true
        app.vault_list_state.select(Some(0));
        app.vault_confirmed = Some(0);

        handle_key(&mut app, KeyCode::Char('m'), KeyModifiers::empty());

        assert!(app.status_dim, "{} 已挂载提示应为灰色状态", lang.code());
        assert!(
            app.status.contains("my_vault"),
            "{} 灰色提示应带卷名: {}",
            lang.code(),
            app.status
        );
        assert!(app.task.is_none(), "已挂载时按 m 不应启动任务");
        assert!(app.overlay.is_none(), "已挂载时按 m 不应弹卸载确认");
        assert!(app.password_input.is_none());
    }
}

/// TAB1：`u` 只卸载——未挂载的卷给灰色提示。
#[test]
fn u_key_only_umounts_and_hints_dim_when_not_mounted() {
    let _guard = lock_lang();
    for lang in [i18n::Lang::ZhCn, i18n::Lang::EnUs] {
        i18n::set_lang(lang);
        let mut app = test_app();
        let mut v = sample_vault();
        v.mounted = false;
        app.vaults = vec![v];
        app.vault_list_state.select(Some(0));

        handle_key(&mut app, KeyCode::Char('u'), KeyModifiers::empty());

        assert!(app.status_dim, "{} 未挂载提示应为灰色状态", lang.code());
        assert!(
            app.status.contains("my_vault"),
            "{} 灰色提示应带卷名: {}",
            lang.code(),
            app.status
        );
        assert!(app.task.is_none(), "未挂载时按 u 不应启动任务");
        assert!(app.overlay.is_none(), "未挂载时按 u 不应弹卸载确认");
    }
}

/// TAB1：`Enter` 不再参与挂载/卸载。（TAB2/TAB3 的 Enter 保持进入向导）
#[test]
fn enter_key_no_longer_mounts_on_tab1() {
    let _guard = lock_lang();
    let mut app = test_app();
    let mut v = sample_vault();
    v.mounted = false;
    app.vaults = vec![v];
    app.vault_list_state.select(Some(0));
    app.vault_confirmed = Some(0);
    let before = app.status.clone();

    handle_key(&mut app, KeyCode::Enter, KeyModifiers::empty());

    assert!(app.task.is_none(), "Enter 不应再触发挂载");
    assert!(app.password_input.is_none(), "Enter 不应再弹出密码输入");
    assert!(app.overlay.is_none());
    assert_eq!(app.status, before, "Enter 不应改变状态文案");
    assert!(!app.status_dim);
}

/// 灰色提示是"一次性"的：下一次按键后恢复普通样式。
#[test]
fn dim_hint_clears_on_next_key() {
    let _guard = lock_lang();
    let mut app = test_app();
    app.vaults = vec![sample_vault()];
    app.vault_list_state.select(Some(0));
    app.vault_confirmed = Some(0);

    handle_key(&mut app, KeyCode::Char('m'), KeyModifiers::empty());
    assert!(app.status_dim);

    handle_key(&mut app, KeyCode::Char('j'), KeyModifiers::empty());
    assert!(!app.status_dim, "下一次按键后应恢复普通样式");
}

/// 发行包（cargo-dist）不含 Shell 后端：TUI 二进制必须自带并在运行前释放它，
/// 否则 GitHub 安装后一启动就是 "执行 CLI 失败"。
#[test]
fn embedded_backend_is_materialized_and_runnable() {
    let dir = std::env::temp_dir().join(format!("gocryptfs-tui-backend-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let cli = backend::install_to(&dir).expect("释放内嵌后端失败");
    assert!(cli.is_file(), "缺少 gocryptfs-cli: {}", cli.display());
    assert!(
        dir.join("lib/gocryptfs-lib.sh").is_file(),
        "缺少 lib/gocryptfs-lib.sh"
    );
    assert!(dir.join("lib/i18n.sh").is_file(), "缺少 lib/i18n.sh");
    assert_eq!(
        std::fs::read_to_string(&cli).unwrap(),
        backend::CLI_SCRIPT,
        "释放出来的后端与二进制内嵌内容不一致"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&cli).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o111,
            0o111,
            "gocryptfs-cli 缺少可执行位: {:o}",
            mode
        );

        // 幂等：内容一致时不重写（重写会换 inode）
        let ino = std::fs::metadata(&cli).unwrap().ino();
        backend::install_to(&dir).expect("二次释放失败");
        assert_eq!(
            std::fs::metadata(&cli).unwrap().ino(),
            ino,
            "内容未变化时不应重写后端文件"
        );
    }

    // 释放出来的目录布局能独立运行：lib 就近解析成功 → 打印用法
    let out = std::process::Command::new("bash")
        .arg(&cli)
        .arg("--help")
        .output()
        .expect("执行内嵌后端失败");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "内嵌后端 --help 失败: {text}");
    assert!(text.contains("Usage:"), "内嵌后端输出异常: {text}");

    // 自愈：文件被破坏／旧版本残留时，重新释放会覆盖回去
    std::fs::write(&cli, "#!/bin/bash\nexit 0\n").unwrap();
    backend::install_to(&dir).expect("自愈失败");
    assert_eq!(
        std::fs::read_to_string(&cli).unwrap(),
        backend::CLI_SCRIPT,
        "被篡改的后端未被重新释放覆盖"
    );
}

/// 后端路径优先级：`GOCRYPTFS_CLI` > 内嵌释放副本 > PATH 中的 `gocryptfs-cli`。
#[test]
fn cli_path_precedence() {
    assert_eq!(
        cli_path_from(Some("/custom/cli"), Some("/embedded/cli")),
        "/custom/cli"
    );
    assert_eq!(
        cli_path_from(Some("   "), Some("/embedded/cli")),
        "/embedded/cli",
        "空白环境变量应视为未设置"
    );
    assert_eq!(cli_path_from(None, Some("/embedded/cli")), "/embedded/cli");
    assert_eq!(cli_path_from(Some("/custom/cli"), None), "/custom/cli");
    assert_eq!(cli_path_from(None, None), CLI_NAME);
}
