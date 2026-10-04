# Changelog

本项目遵循 [Semantic Versioning](https://semver.org/)。

## [0.5.2] - 2026-10-04

### Testing
- 新增渲染测试：TAB2/TAB3 按 `Enter` 不进入向导且不改状态、相关文案不含 `Enter`
- `make test-install` 新增：安装脚本 `--help` 与安装/卸载输出**不含中文**

### 变更
- 安装脚本（`install.sh` / Release 附件 `gocryptfs-tui-install.sh`）的**所有提示改为英文**
  （用法、参数错误、下载与 sha256 校验、自检、卸载、收尾提示），任意 locale 下都可读
- 英文界面全局行去掉 `]` 与 `[` 之间的空格：`Focus[Tab/Shift+Tab][Alt+1/2/3/4]`（与中文写法一致）
- **TAB2/TAB3 取消 `Enter` 触发创建/删除向导**，只保留 `c` / `d`；
  页面行与帮助浮层文案同步（`创建向导[c]` / `Open create wizard[c]`），导出的 Markdown 帮助一并更新

## [0.5.1] - 2026-10-04

### Documentation
- 更新 CHANGELOG

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
