### Documentation
- `RELEASING.md`、`SECURITY.md` 改为**英文默认**：中文版移至 `RELEASING.zh-CN.md`、
  `SECURITY.zh-CN.md`，两侧顶部都有 `English | 中文` 切换（与 README 一致）
- `changelog.d/README.md` 改为**中英双语同文件**（英文在前、中文在后）；文件名保持不变，
  因为片段工具按 `README.md` 名称跳过它
- README（中英）的目录树与文档链接同步：中文版指向 `*.zh-CN.md`
- 修正 SECURITY 文档里过时的日志路径（`/tmp/gocryptfs-tui.log` →
  `$XDG_DATA_HOME/gocryptfs-tui/app.log.jsonl`）
