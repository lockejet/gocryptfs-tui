# Changelog

本项目遵循 [Semantic Versioning](https://semver.org/)。

## [0.4.0] - 2026-10-04

### Features
- `install.sh` 支持直接从 GitHub Release 安装：`--from-release` / `--version vX.Y.Z` /
  `--release-version`，**不需要 Rust 工具链与源码**；自动判定目标三元组、校验 sha256，
  并从同一 Release 的 `source.tar.gz` 取 Shell 后端，保证 TUI 与后端版本配套
- 新增 `--system`（等价 `--prefix /usr/local`，并默认链接 `gocryptfs-cli`）、
  `--no-link-cli`、`--no-deps-check` 选项
- 安装写清单 `<prefix>/lib/gocryptfs-tui/INSTALLED.json` 与 `INSTALLED.files`
  （版本/来源/文件列表/时间）；`--uninstall` 按清单精确删除，只允许删除前缀内的路径，
  前缀里的其它文件不受影响
- 安装前的依赖检查降级为警告（真正的判定交给 `gocryptfs-tui --check-deps`），
  这样在依赖暂时缺失的机器上也能先完成安装
- 该脚本随 Release 发布为 `gocryptfs-tui-install.sh`（cargo-dist `extra-artifacts`），
  一条命令即可完成系统级安装：`curl .../gocryptfs-tui-install.sh | sh -s -- --system`

### Testing
- 新增 `make test-install`：19 项回归测试（用户级/系统级/清单内容/卸载安全性/参数解析）

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
