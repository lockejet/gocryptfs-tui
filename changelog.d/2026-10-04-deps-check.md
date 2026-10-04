### Features
- 新增运行时依赖检查：`gocryptfs-tui --check-deps` 列出缺失依赖、用途与 Debian/Ubuntu 安装命令，
  并识别 Debian 源里的「Python 版 yq」（提示改用 mikefarah Go v4）；缺必需依赖时退出码 1
- TUI 启动时若缺必需依赖（或 yq 版本不对），直接把上述提示写进输出区，
  不再只表现为「列表为空」「执行 CLI 失败」
- `install.sh` 安装完成后自动执行依赖检查并给出安装提示

### Documentation
- README「依赖」章节补充 `mountpoint`、Go 版 yq 说明、`--check-deps` 用法与安装阶段自动检查
