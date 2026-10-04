### 变更
- 安装脚本（`install.sh` / Release 附件 `gocryptfs-tui-install.sh`）的**所有提示改为英文**
  （用法、参数错误、下载与 sha256 校验、自检、卸载、收尾提示），任意 locale 下都可读
- 英文界面全局行去掉 `]` 与 `[` 之间的空格：`Focus[Tab/Shift+Tab][Alt+1/2/3/4]`（与中文写法一致）
- **TAB2/TAB3 取消 `Enter` 触发创建/删除向导**，只保留 `c` / `d`；
  页面行与帮助浮层文案同步（`创建向导[c]` / `Open create wizard[c]`），导出的 Markdown 帮助一并更新

### Testing
- 新增渲染测试：TAB2/TAB3 按 `Enter` 不进入向导且不改状态、相关文案不含 `Enter`
- `make test-install` 新增：安装脚本 `--help` 与安装/卸载输出**不含中文**
