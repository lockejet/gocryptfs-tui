### 变更
- **遵循 XDG 状态目录**：日志 `app.log.jsonl`（含 `.1`…`.N` 轮转）与历史 `history.jsonl`
  从数据目录迁到 `${XDG_STATE_HOME:-~/.local/state}/gocryptfs-tui/`；`--print-paths` 与 `--help`
  新增「状态目录」，目录分类与 XDG 规范的 Config / Data / State 一致
- 新增 `GOCRYPTFS_STATE_DIR` 覆盖状态目录；显式 `-D`/`GOCRYPTFS_DATA_DIR` 时状态目录与数据目录合并
  （隔离调试仍是"所有运行期文件在一处"），`LOG_FILE`/`HISTORY_FILE` 依旧最高优先
- **旧布局自动迁移**：首次运行（TUI 或 `gocryptfs-cli log`）把旧数据目录中的日志/历史/轮转文件
  搬到状态目录；新路径优先、失败保留旧文件、可重复执行
- **修复 `XDG_CONFIG_HOME` 被忽略**：后端不可用时 TUI 的配置路径回退写死 `$HOME/.config`，
  现在始终优先 `dirs::config_dir()`（即 `XDG_CONFIG_HOME` → `~/.config`）
- Shell 后端锁文件从固定 `/tmp/gocryptfs-tui.lock` 改为优先
  `$XDG_RUNTIME_DIR/gocryptfs-tui.lock`（0700，避免固定路径的 symlink 风险），
  回退 `/tmp/gocryptfs-tui.<uid>.lock`
- 文档同步：中英文 README 的配置/数据/状态目录与优先级、SECURITY 的日志路径、
  安装脚本卸载提示里的目录清单

### Testing
- 新增 Rust 测试：旧布局 → 状态目录迁移（轮转文件、幂等、`HELP.md` 留在数据目录）；
  `--print-paths` 断言状态目录行
- `test-batch1` 新增 3 项：默认日志位置跟随 `XDG_STATE_HOME`、`GOCRYPTFS_STATE_DIR` 覆盖、
  锁文件优先 `XDG_RUNTIME_DIR`
