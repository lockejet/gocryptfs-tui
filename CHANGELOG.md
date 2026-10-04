# Changelog

本项目遵循 [Semantic Versioning](https://semver.org/)。

## [0.5.0] - 2026-10-04

### Documentation
- README「安装」重写为三种模式 + 通用选项表；离线 tar.xz 降级为懒人模式附注；
  升级/卸载按三种模式统一，`make install*` 标注为模式③的封装
- RELEASING 发布检查改为"确认 Release 里只有一个安装脚本"+ `sh -n` 语法与 `--user` 冒烟

### Testing
- `make test-install` 扩展到 35 项：默认落点为 `~/.local`、`--user`、自动回退 Release、
  clone 模式默认取当前 tag、`--system` 链接 CLI、卸载安全性等

### 变更
- **安装入口收敛为三种模式**（① 懒人 / ② git clone / ③ 手动源码），三种共用一个安装脚本与
  同一套选项；**不再发布 cargo-dist 的 `gocryptfs-tui-installer.sh`**
  （它会修改 shell profile、默认落点与我们的脚本不一致、0.4.x 起也没有卸载入口），
  Release 里只保留 `gocryptfs-tui-install.sh`
- 安装脚本默认前缀改为**用户级 `~/.local`**（此前是 `/usr/local`）；系统级用 `--system`
  （装 `/usr/local` 并默认把 `gocryptfs-cli` 链接到 `/usr/local/bin`）；新增 `--user` 显式指定
- `--from-release` 在 git checkout 里默认取**当前 tag** 对应的 Release（与源码配套），
  否则退回 `latest`
- 自动模式加兜底：源码树里既没有 `target/release/gocryptfs-tui` 又没有 cargo 时，
  自动改用 Release 二进制并打印一行说明（不再因缺 cargo 直接失败）

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
