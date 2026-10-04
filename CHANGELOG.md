# Changelog

本项目遵循 [Semantic Versioning](https://semver.org/)。

## [Unreleased]

### Bug Fixes
- `gocryptfs-tui-install.sh`（及 `install.sh`）**默认模式改为自动判断**：
  `curl … | sh`（无源码树）→ 从 Release 安装；在源码仓库里执行 `./install.sh` → 本地编译。
  此前一律按本地源码处理，没有 Rust 工具链的机器会直接报 `cargo: not found`；
  新增 `--local` 可强制本地模式
- 本地模式缺少 cargo 时给出可操作提示（改用 `--from-release`），而不是 `cargo: not found`
- 用法文本只保留在 `--help`（heredoc）一处，头部注释不再重复，避免文档与实现漂移
- 新增 `GOCRYPTFS_TUI_REPO` 环境变量可覆盖仓库地址（镜像/测试用）

### Testing
- `make test-install` 扩展到 29 项：新增模式自动判断用例（管道执行走 Release、
  不会尝试编译、无 cargo 时的提示、`--local` 强制本地模式）

## [0.4.1] - 2026-10-04

### Documentation
- 更新 CHANGELOG

## [0.4.0] - 2026-10-04

### Documentation
- 更新 CHANGELOG
- 更新 CHANGELOG

## [0.3.0] - 2026-10-04

### Documentation
- 更新 CHANGELOG

## [0.2.2] - 2026-10-04

### Documentation
- 更新 CHANGELOG

## [0.2.1] - 2026-10-03

### Documentation
- 更新 CHANGELOG

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
