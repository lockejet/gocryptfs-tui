# Changelog

本项目遵循 [Semantic Versioning](https://semver.org/)。

## [Unreleased]

### Features
- TUI 二进制内嵌 Shell 后端（`gocryptfs-cli` + `lib/{gocryptfs-lib.sh,i18n.sh}`）：
  首次运行时释放到 `~/.local/share/gocryptfs-tui/backend/`（内容未变不重写，升级自动更新），
  安装一次即可用，且后端版本始终与 TUI 配套，不再受系统旧后端影响
- 后端路径优先级：`GOCRYPTFS_CLI` > 内嵌释放副本 > PATH 中的 `gocryptfs-cli`；
  `--help` 的「CLI 可执行文件路径」改为显示实际使用的后端

### Bug Fixes
- 修复从 GitHub Release 安装后启动报「执行 CLI 失败」：发行包（cargo-dist）只含 Rust 二进制，
  `gocryptfs-cli` 与 Shell 库并未随之分发

## [0.2.0] - 2026-10-03

### Documentation
- 更新 CHANGELOG

## [0.1.2] - 2026-09-30

### Documentation
- 更新 README 与 CHANGELOG 键位说明


### Features
- 换页与换区键位对齐主流 TUI 惯例 (tui)

## [0.1.1] - 2026-09-30

### Documentation
- 更新 README 并生成 CHANGELOG


### Features
- 新增命令行参数解析 (cli)
- 新增统一 JSONL 日志模块 (logger)
- 统一日志格式并新增 log 子命令 (cli)
- 接入参数解析与日志，新增帮助导出和历史过滤 (tui)


### Miscellaneous
- 初始化 cargo-dist


### Build
- 新增版本注入与 cargo-release/cargo-dist 发布流程


### Revert
- 撤回配置文件 git 版本管理
