# Changelog

本项目遵循 [Semantic Versioning](https://semver.org/)。

## [Unreleased]

### Testing
- 新增渲染测试：页签行不再包含分页提示；页面行 `Page: [n]` 为黄色、动作提示为白色（中英双语）

### 变更
- 底部全局行的分页提示写作 `换页[1/2/3/[/]]`（英文 `Page[1/2/3/[/]]`）：
  把顺序换页键 `[` `]` 合进同一组括号，与直接换页键 `1/2/3` 并列
- 页签行不再显示分页提示（`换页[Tab] 或 [ ]` / `page[Tab] or [ ]`）：
  该操作已在底部全局行的 `Page[1/2/3] [/]` 中列出，避免重复
- 底部状态栏页面行的 `Page: [n]` 改为**黄色**（与全局行标签一致），其后的动作提示仍为白色

## [0.5.0] - 2026-10-04

### Documentation
- 更新 CHANGELOG
- 更新 CHANGELOG

## [0.4.2] - 2026-10-04

### Documentation
- 更新 CHANGELOG

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
