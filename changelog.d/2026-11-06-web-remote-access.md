### Features
- **Web UI 远程访问（反向代理）支持**：新增 `--public-url URL`（终端二维码、一次性登录链接使用对外
  地址，例如 `https://example.com`）与 `--trust-proxy`（信任 `X-Forwarded-Proto/Host/For`：`https`
  时 cookie 自动加 `Secure`、CSRF `Origin` 校验接受转发 Host、登录限流按真实客户端 IP 计数）
- 新增 `docs/remote-access.md`（中英）：拓扑、Caddy/Nginx 反代示例（`example.com`）、Windows 客户端
  清单、安全核对表与排错；README「Web UI」补「从 Windows/macOS/手机访问」小节
- Web UI 信息页显示对外地址与代理信任状态
- 新增 `test/test-web-proxy.sh`（13 项反代场景测试）并纳入 `make verify`
