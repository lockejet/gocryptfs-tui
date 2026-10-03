### Build
- `make install-local` / `make install` 现在会连同 Shell 后端一起安装到 `~/.local`
  （此前只装 TUI 二进制，容易与系统里的旧 `gocryptfs-cli` 混用）
- Makefile 新增 `install` / `uninstall` / `install-system` / `uninstall-system`
  （`uninstall` 带路径护栏）；`install.sh` 支持 `--prefix` / `--uninstall` / `--no-build`、
  按需 sudo，并在安装后自检 PATH 中的 CLI 是否为本次安装
- TUI 启动输出打印解析后的 CLI 实际路径；英文界面下启动时探测后端是否支持 `--lang`，
  旧版后端会给出显式告警
- `gocryptfs-cli log` 跳过日志中的非 JSON 行（旧版后端曾向 `app.log.jsonl` 追加纯文本）
- 移除 cargo-release 的 `pre-release-hook`：`make dry-run` / `cargo release --dry-run`
  不再改写 `CHANGELOG.md`；新增 `changelog.d/` 片段机制（`make changelog-preview` 预览、
  `make changelog` 合并片段且片段保留，发布时才归档），手写内容随代码提交可长期保留
