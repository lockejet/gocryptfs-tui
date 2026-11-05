//! `gocryptfs-tui` 可执行入口（真正的实现在 `gocryptfs-ui-tui` 库里）。
fn main() {
    if let Err(e) = gocryptfs_ui_tui::run() {
        // 与原先 `fn main() -> Result<...>` 的默认错误输出保持一致
        eprintln!("Error: {e:?}");
        std::process::exit(1);
    }
}
