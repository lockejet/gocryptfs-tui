# Changelog

本项目遵循 [Semantic Versioning](https://semver.org/)。

## [0.3.0] - 2026-10-04

### Features
- Shell 后端与 Rust 侧路径规则对齐：支持 `XDG_CONFIG_HOME` / `XDG_DATA_HOME`
  （此前硬编码 `$HOME/.config`、`$HOME/.local/share`，设了 XDG 变量时会读到不同配置）
- `gocryptfs-cli` 补齐与 TUI 同款的全局选项：`-V/--version`、`-D/--data-dir`；
  路径优先级统一为 `-D/-c` > `GOCRYPTFS_DATA_DIR`/`GOCRYPTFS_CONFIG` >
  旧变量（`LOG_FILE`/`HISTORY_FILE`/`CONFIG_FILE`）> XDG > 默认
- TUI 释放内嵌后端时同时写入 `VERSION`，`gocryptfs-cli --version` 与 TUI 版本一致
- `install.sh` 默认**不再**把 `gocryptfs-cli` 软链到 `<prefix>/bin`（三种方式一致：
  规范位置为数据目录下的内嵌副本），需要时用 `--link-cli` 显式开启

### Documentation
- README「安装」改为"三种方式 → 同一套路径"对照表 + 统一自检三连
  （`command -v` / `--version` / `--check-deps`）+ 卸载清理说明；
  配置/数据目录补上 XDG 与环境变量优先级
- 运行时依赖清单（安装前请确认）：`gocryptfs`、`fusermount`（`fuse3`）、`rsync`、
  `yq`（**mikefarah Go 版 v4**）、`jq`、`mountpoint`（`util-linux`）；可选 `tree`。
  检查：`gocryptfs-tui --check-deps`（缺必需依赖时退出码非 0，并打印安装命令）
- 新增 `gocryptfs-tui --print-paths`：一次打印实际使用的二进制/后端/配置/数据/日志路径（排障）

### 变更
- **统一三种安装方式的默认落点**：官方安装脚本改用 `install-path = "~/.local/bin/"`，
  与 `install.sh`/`make install`、手动解压一致（此前官方脚本装到 `~/.cargo/bin`）；
  从 0.2.x 升级的用户需清理 `~/.cargo/bin/gocryptfs-tui*` 残留（README 有说明）
- 关闭 cargo-dist 的 `install-updater`：不再产出 `gocryptfs-tui-update` 与安装 receipt
  （`~/.config/gocryptfs-tui/gocryptfs-tui-receipt.json`），三种方式的升级方式统一为
  "重跑对应安装方式"

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
