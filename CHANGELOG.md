# Changelog

本项目遵循 [Semantic Versioning](https://semver.org/)。

## [0.7.1] - 2026-11-06

**安装 / Install** — 用户级装到 `~/.local/bin`（默认，无需 sudo），系统级加 `--system` 装到 `/usr/local/bin`：

```bash
curl -LsSf https://github.com/lockejet/gocryptfs-ui/releases/latest/download/gocryptfs-ui-install.sh | sh
curl -LsSf https://github.com/lockejet/gocryptfs-ui/releases/latest/download/gocryptfs-ui-install.sh | sh -s -- --system
```

预编译包（x86_64/aarch64 glibc、x86_64 musl）、`source.tar.gz` 与 sha256 校验和见下方 Assets；git clone 与源码编译两种方式见 [README](https://github.com/lockejet/gocryptfs-ui#installation)。

### Features
- **Web UI 支持中英文切换**：全部界面文案 i18n 化（163 个键，中英目录自动校验一致性），
  默认跟随浏览器语言、选择持久化；顶栏与登录页均有语言切换按钮；挂载/卸载任务的
  CLI 日志也会按界面语言本地化（`-l` 传给后端）
- **2FA 默认关闭**：登录后若当前账户未启用两步验证，界面显示提示（可「立即启用」跳到安全页
  或「稍后」），安全页显示启用状态与剩余备份码；token 登录且未配置用户时提示可用方式

### Bug Fixes
- 修复**英文界面混入中文**的问题（`手机登录` 等硬编码中文），改为统一走 i18n 词表

## [0.7.0] - 2026-11-05

### Documentation
- 更新 CHANGELOG

## [0.6.1] - 2026-10-31

### Documentation
- 更新 CHANGELOG

## [0.6.0] - 2026-10-31

### Documentation
- 更新 CHANGELOG

## [0.5.3] - 2026-10-10

### Documentation
- 更新 CHANGELOG

## [0.5.2] - 2026-10-04

### Documentation
- 更新 CHANGELOG

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
