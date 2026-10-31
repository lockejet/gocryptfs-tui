### Documentation
- **Release 说明自动附带安装命令**：每次发布的 GitHub Release 正文开头会插入
  `tools/release-preamble.md` 的内容（用户级 / 系统级两条 `curl | sh` 命令 + 资产提示），
  改该文件即可全局调整；`RELEASE_PREAMBLE=<path>` 可换用其他文件
- **支持无人值守发布**：`make release LEVEL=patch CONFIRM=0` 会传 `--no-confirm`
  跳过 `cargo-release` 的 y/N 确认（CI / 定时任务用），默认仍保留交互确认
