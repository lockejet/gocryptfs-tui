# Changelog

本项目遵循 [Semantic Versioning](https://semver.org/)。

## [Unreleased]

### Bug Fixes
- `gocryptfs-cli list` 在缺少 `yq`/`jq`/`mountpoint` 时不再静默返回 0 个卷，改为明确报错
  （此前 TUI 只会显示一个空列表，无从判断原因）
- TUI 列表为空时直接显示原因：加载失败（红色，含 CLI 报错原文与「按 r 重试」提示）
  或「配置里没有卷: <路径>」（含「按 e 编辑配置」提示）；此前只有一行灰色「（无卷）」
- 错误输出在缺少 `jq` 时也能正常打印（JSON 模式自动降级为纯文本）

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
