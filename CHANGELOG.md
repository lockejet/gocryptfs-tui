# Changelog

本项目遵循 [Semantic Versioning](https://semver.org/)。

## [0.8.0] - 2026-11-06

**安装 / Install** — 用户级装到 `~/.local/bin`（默认，无需 sudo），系统级加 `--system` 装到 `/usr/local/bin`：

```bash
curl -LsSf https://github.com/lockejet/gocryptfs-ui/releases/latest/download/gocryptfs-ui-install.sh | sh
curl -LsSf https://github.com/lockejet/gocryptfs-ui/releases/latest/download/gocryptfs-ui-install.sh | sh -s -- --system
```

预编译包（x86_64/aarch64 glibc、x86_64 musl）、`source.tar.gz` 与 sha256 校验和见下方 Assets；git clone 与源码编译两种方式见 [README](https://github.com/lockejet/gocryptfs-ui#installation)。

### Features
- **Web UI 远程访问（反向代理）支持**：新增 `--public-url URL`（终端二维码、一次性登录链接使用对外
  地址，例如 `https://example.com`）与 `--trust-proxy`（信任 `X-Forwarded-Proto/Host/For`：`https`
  时 cookie 自动加 `Secure`、CSRF `Origin` 校验接受转发 Host、登录限流按真实客户端 IP 计数）
- 新增 `docs/remote-access.md`（中英）：拓扑、Caddy/Nginx 反代示例（`example.com`）、Windows 客户端
  清单、安全核对表与排错；README「Web UI」补「从 Windows/macOS/手机访问」小节
- Web UI 信息页显示对外地址与代理信任状态
- 新增 `test/test-web-proxy.sh`（13 项反代场景测试）并纳入 `make verify`

## [0.7.1] - 2026-11-06

### Documentation
- 更新 CHANGELOG

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
