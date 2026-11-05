### 变更
- **目录名统一为 `gocryptfs-ui`**：配置/数据/状态目录由 `gocryptfs-tui` 改为
  `gocryptfs-ui`（`~/.config/gocryptfs-ui`、`~/.local/share/gocryptfs-ui`、
  `~/.local/state/gocryptfs-ui`），安装目录由 `<prefix>/lib/gocryptfs-tui` 改为
  `<prefix>/lib/gocryptfs-ui`；**命令名不变**（`gocryptfs-tui` / `gocryptfs-cli` / `gocryptfs-web`）
- **旧目录自动迁移**：首次启动（TUI / CLI / Web 任一端）把旧的 `gocryptfs-tui` 目录整体搬到新位置，
  仅在新目录不存在时执行；旧安装的 `<prefix>/lib/gocryptfs-tui` 在安装/卸载时清理
- 文档（中英 README / SECURITY）同步新路径与升级说明；shell 帮助、systemd 单元同步

### Testing
- `test-batch1` 新增 3 项目录迁移用例；`test-install` 路径断言更新；`test-i18n` 帮助文案同步
