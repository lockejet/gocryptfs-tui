# Changelog

本项目遵循 [Semantic Versioning](https://semver.org/)。

## [0.2.2] - 2026-10-04

### Features
- 新增运行时依赖检查：`gocryptfs-tui --check-deps` 列出缺失依赖、用途与 Debian/Ubuntu 安装命令，
  并识别 Debian 源里的「Python 版 yq」（提示改用 mikefarah Go v4）；缺必需依赖时退出码 1
- TUI 启动时若缺必需依赖（或 yq 版本不对），直接把上述提示写进输出区，
  不再只表现为「列表为空」「执行 CLI 失败」
- `install.sh` 安装完成后自动执行依赖检查并给出安装提示

### Bug Fixes
- `gocryptfs-cli list` 在缺少 `yq`/`jq`/`mountpoint` 时不再静默返回 0 个卷，改为明确报错
  （此前 TUI 只会显示一个空列表，无从判断原因）
- TUI 列表为空时直接显示原因：加载失败（红色，含 CLI 报错原文与「按 r 重试」提示）
  或「配置里没有卷: <路径>」（含「按 e 编辑配置」提示）；此前只有一行灰色「（无卷）」
- 错误输出在缺少 `jq` 时也能正常打印（JSON 模式自动降级为纯文本）
- `gocryptfs-cli` 缺参数/缺选项值时给出用法或明确报错，不再因 `set -u` 直接崩溃
  （`$1: unbound variable`）：受影响命令 `info`/`ls`/`tree`/`mount`，以及
  `--name`/`--cipher`/`--target`/`--limit`/`--src`/`--action`/`--result`/`--since` 缺值

### Documentation
- README「依赖」章节补充 `mountpoint`、Go 版 yq 说明、`--check-deps` 用法与安装阶段自动检查

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
