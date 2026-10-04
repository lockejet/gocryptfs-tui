# Changelog

本项目遵循 [Semantic Versioning](https://semver.org/)。

## [0.4.1] - 2026-10-04

### Bug Fixes
- `gocryptfs-tui-install.sh` 改为 **POSIX sh 兼容**：原来用了 bash 数组等语法，
  在 Debian/Ubuntu 的 `/bin/sh`（dash）下会直接报 `Syntax error: "(" unexpected`，
  导致 README 推荐的 `curl ... | sh -s -- --system` 无法使用（`| bash -s --` 可绕过）
- `usage()` 改为内嵌文本，不再依赖 `$0`；管道执行（`curl | sh`）时卸载提示给出 URL 形式
- 卸载时顺手清理空的 `<prefix>/bin`、`<prefix>/lib` 目录（非空则自动跳过）

### Testing
- `make test-install` 扩展为 24 项：全部改用 `sh`（dash）执行，新增 `dash -n` / `sh -n`
  语法检查与 `cat install.sh | sh -s --` 管道用例，防止再次引入 bash 专有语法

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
