### Features
- `install.sh` 支持直接从 GitHub Release 安装：`--from-release` / `--version vX.Y.Z` /
  `--release-version`，**不需要 Rust 工具链与源码**；自动判定目标三元组、校验 sha256，
  并从同一 Release 的 `source.tar.gz` 取 Shell 后端，保证 TUI 与后端版本配套
- 新增 `--system`（等价 `--prefix /usr/local`，并默认链接 `gocryptfs-cli`）、
  `--no-link-cli`、`--no-deps-check` 选项
- 安装写清单 `<prefix>/lib/gocryptfs-tui/INSTALLED.json` 与 `INSTALLED.files`
  （版本/来源/文件列表/时间）；`--uninstall` 按清单精确删除，只允许删除前缀内的路径，
  前缀里的其它文件不受影响
- 安装前的依赖检查降级为警告（真正的判定交给 `gocryptfs-tui --check-deps`），
  这样在依赖暂时缺失的机器上也能先完成安装
- 该脚本随 Release 发布为 `gocryptfs-tui-install.sh`（cargo-dist `extra-artifacts`），
  一条命令即可完成系统级安装：`curl .../gocryptfs-tui-install.sh | sh -s -- --system`

### Testing
- 新增 `make test-install`：19 项回归测试（用户级/系统级/清单内容/卸载安全性/参数解析）
