# Changelog

本项目遵循 [Semantic Versioning](https://semver.org/)。

## [Unreleased]

### Features
- **新增 Web UI（`gocryptfs-web`）**：浏览器管理加密卷——卷列表与实时挂载状态、挂载/卸载
  （口令弹窗 + SSE 实时进度）、统一 JSONL 日志、生效路径/依赖查看；默认仅监听 `127.0.0.1:30032`
  （端口约定：反代 44332 → 本机 30032）
- 主题：亮 / 暗 / 跟随系统三态切换（持久化、防闪烁）；手机端自适应布局（卡片化、底部抽屉、44px 触控）
- 认证：一次性 token（SHA-256 哈希存储，支持多 token / 标签 / 过期）、用户名+密码
  （Argon2id、失败限流与锁定）、可选 TOTP 二次验证（含 8 个一次性备份码）、
  终端二维码 / 一次性链接扫码登录；`gocryptfs-web token|user` 管理命令，改动即时生效
- 新增 `make web-build`（Svelte + Vite，构建产物内嵌进二进制，CI 无需 Node）
- systemd 用户单元 `contrib/systemd/gocryptfs-web.service`（**刻意不带 mount-namespace 沙箱**：
  gocryptfs 挂载需在 host 命名空间，`fusermount` 需 setuid；已写入注释说明）

### Documentation
- README（中英）新增「Web UI」章节（启动、登录方式、凭证管理、反代示例、systemd）；
  SECURITY（中英）新增「Web UI 威胁模型」

### Testing
- `test-batch1` 新增 3 项目录迁移用例；`test-install` 路径断言更新；`test-i18n` 帮助文案同步

### 变更
- **目录名统一为 `gocryptfs-ui`**：配置/数据/状态目录由 `gocryptfs-tui` 改为
  `gocryptfs-ui`（`~/.config/gocryptfs-ui`、`~/.local/share/gocryptfs-ui`、
  `~/.local/state/gocryptfs-ui`），安装目录由 `<prefix>/lib/gocryptfs-tui` 改为
  `<prefix>/lib/gocryptfs-ui`；**命令名不变**（`gocryptfs-tui` / `gocryptfs-cli` / `gocryptfs-web`）
- **旧目录自动迁移**：首次启动（TUI / CLI / Web 任一端）把旧的 `gocryptfs-tui` 目录整体搬到新位置，
  仅在新目录不存在时执行；旧安装的 `<prefix>/lib/gocryptfs-tui` 在安装/卸载时清理
- 文档（中英 README / SECURITY）同步新路径与升级说明；shell 帮助、systemd 单元同步
- 仓库更名为 **`gocryptfs-ui`**（原 `gocryptfs-tui`），统一承载三种前端：
  sh 后端（`gocryptfs-cli`）、TUI（`gocryptfs-tui`）、Web UI（`gocryptfs-web`，开发中）；
  旧仓库地址由 GitHub 自动 301 跳转，**命令名与配置/数据/状态目录名保持不变**（`gocryptfs-tui`），
  已安装用户无需任何迁移

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
