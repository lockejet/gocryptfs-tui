### Bug Fixes
- `gocryptfs-tui-install.sh` 改为 **POSIX sh 兼容**：原来用了 bash 数组等语法，
  在 Debian/Ubuntu 的 `/bin/sh`（dash）下会直接报 `Syntax error: "(" unexpected`，
  导致 README 推荐的 `curl ... | sh -s -- --system` 无法使用（`| bash -s --` 可绕过）
- `usage()` 改为内嵌文本，不再依赖 `$0`；管道执行（`curl | sh`）时卸载提示给出 URL 形式
- 卸载时顺手清理空的 `<prefix>/bin`、`<prefix>/lib` 目录（非空则自动跳过）

### Testing
- `make test-install` 扩展为 24 项：全部改用 `sh`（dash）执行，新增 `dash -n` / `sh -n`
  语法检查与 `cat install.sh | sh -s --` 管道用例，防止再次引入 bash 专有语法
