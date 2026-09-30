# Changelog

本项目遵循 [Semantic Versioning](https://semver.org/)。

## [Unreleased]

### Build

- **版本注入**：新增 `build.rs`，编译期从 git tag 注入
  `APP_VERSION` / `APP_COMMIT` / `APP_BUILD_TIME`
- **构建入口**：新增 `Makefile`，提供 `build` / `check` / `release` /
  `dist` / `install-local` 等目标
- **发布流程**：`cargo-release` + `git-cliff` + `cargo-dist` 组合。
  bump 版本、生成 CHANGELOG、打 tag、CI 多平台构建
  （gnu/musl × amd64/arm64）、创建 GitHub Release
- **交叉编译**：新增 `Cross.toml`，本地可用 `cross` 编译 arm64 / musl

### Features

- **CLI 参数**：TUI 新增 `-c/--config`、`-D/--data-dir`、
  `-h/--help`、`-V/--version`。手写解析，不引入 clap
- **统一日志**：新增 `src/logger.rs`，TUI 与 CLI 共用
  `app.log.jsonl`。三个级别 `operation` / `interactive` / `debug`。
  按 `max_size` 轮转，保留 `max_files` 个历史文件
- **CLI `log` 子命令**：支持 `--limit` / `--src` / `--action` /
  `--result` / `--since` / `--follow` / `--json`
- **TUI 帮助菜单**：帮助浮层加入版本信息、路径、启动参数段。
  `H` 键导出到 `<data_dir>/HELP.md`
- **TUI 历史浮层**：读取 `app.log.jsonl`，支持按来源 `s` /
  结果 `r` / 操作 `a` 循环过滤
- **旧历史迁移**：首次启动时自动把 `history.jsonl`
  迁移到 `app.log.jsonl`

### Bug Fixes

- **详情栏状态**：创建后详情栏根据 `vaults` 动态判断
  「已创建 / 待创建」
- **禁止重复创建**：`enter_create_wizard` 双重检查；
  CLI `cmd_create` 增加 `mount_point` 冲突检查
- **非加密卷标记**：缺少 `gocryptfs.conf` 的条目在列表标红，
  禁止挂载 / 卸载 / 删除
- **向导覆盖**：向导渲染前 `Clear`，避免与底层列表重叠
- **删除向导卡住**：`poll_task` 完成后清空 `task`
- **参数解析**：`cli.rs` 未知参数不提前 `break`，
  确保 `--help` / `--version` 优先生效

### Documentation

- **README 更新**：新增"命令行参数"、"日志与历史"、"发布"、
  "cargo-dist 安装方式"章节
- **RELEASING.md**：新增发版流程文档

### Revert

- **撤回配置文件 git 版本管理**：移除 `gocryptfs-cli` 的
  `git-init` / `git-log` / `git-diff` / `git-rollback` / `git-status`
  子命令。它们管理配置文件而非源码，与项目定位不符
