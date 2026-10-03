### Bug Fixes
- 修复从 GitHub Release 安装后启动报「执行 CLI 失败」：发行包（cargo-dist）只含 Rust 二进制，
  `gocryptfs-cli` 与 Shell 库并未随之分发

### Features
- TUI 二进制内嵌 Shell 后端（`gocryptfs-cli` + `lib/{gocryptfs-lib.sh,i18n.sh}`）：
  首次运行时释放到 `~/.local/share/gocryptfs-tui/backend/`（内容未变不重写，升级自动更新），
  安装一次即可用，且后端版本始终与 TUI 配套，不再受系统旧后端影响
- 后端路径优先级：`GOCRYPTFS_CLI` > 内嵌释放副本 > PATH 中的 `gocryptfs-cli`；
  `--help` 的「CLI 可执行文件路径」改为显示实际使用的后端
