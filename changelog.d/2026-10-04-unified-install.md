### 变更
- **统一三种安装方式的默认落点**：官方安装脚本改用 `install-path = "~/.local/bin/"`，
  与 `install.sh`/`make install`、手动解压一致（此前官方脚本装到 `~/.cargo/bin`）；
  从 0.2.x 升级的用户需清理 `~/.cargo/bin/gocryptfs-tui*` 残留（README 有说明）
- 关闭 cargo-dist 的 `install-updater`：不再产出 `gocryptfs-tui-update` 与安装 receipt
  （`~/.config/gocryptfs-tui/gocryptfs-tui-receipt.json`），三种方式的升级方式统一为
  "重跑对应安装方式"

### Features
- Shell 后端与 Rust 侧路径规则对齐：支持 `XDG_CONFIG_HOME` / `XDG_DATA_HOME`
  （此前硬编码 `$HOME/.config`、`$HOME/.local/share`，设了 XDG 变量时会读到不同配置）
- `gocryptfs-cli` 补齐与 TUI 同款的全局选项：`-V/--version`、`-D/--data-dir`；
  路径优先级统一为 `-D/-c` > `GOCRYPTFS_DATA_DIR`/`GOCRYPTFS_CONFIG` >
  旧变量（`LOG_FILE`/`HISTORY_FILE`/`CONFIG_FILE`）> XDG > 默认
- TUI 释放内嵌后端时同时写入 `VERSION`，`gocryptfs-cli --version` 与 TUI 版本一致
- `install.sh` 默认**不再**把 `gocryptfs-cli` 软链到 `<prefix>/bin`（三种方式一致：
  规范位置为数据目录下的内嵌副本），需要时用 `--link-cli` 显式开启

### Documentation
- README「安装」改为"三种方式 → 同一套路径"对照表 + 统一自检三连
  （`command -v` / `--version` / `--check-deps`）+ 卸载清理说明；
  配置/数据目录补上 XDG 与环境变量优先级
