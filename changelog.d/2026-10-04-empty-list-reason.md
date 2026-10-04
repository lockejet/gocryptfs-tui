### Bug Fixes
- `gocryptfs-cli list` 在缺少 `yq`/`jq`/`mountpoint` 时不再静默返回 0 个卷，改为明确报错
  （此前 TUI 只会显示一个空列表，无从判断原因）
- TUI 列表为空时直接显示原因：加载失败（红色，含 CLI 报错原文与「按 r 重试」提示）
  或「配置里没有卷: <路径>」（含「按 e 编辑配置」提示）；此前只有一行灰色「（无卷）」
- 错误输出在缺少 `jq` 时也能正常打印（JSON 模式自动降级为纯文本）
- `gocryptfs-cli` 缺参数/缺选项值时给出用法或明确报错，不再因 `set -u` 直接崩溃
  （`$1: unbound variable`）：受影响命令 `info`/`ls`/`tree`/`mount`，以及
  `--name`/`--cipher`/`--target`/`--limit`/`--src`/`--action`/`--result`/`--since` 缺值
