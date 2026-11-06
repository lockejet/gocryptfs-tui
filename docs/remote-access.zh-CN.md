# 从其他设备访问 Web UI

Web UI 是纯浏览器客户端：服务端只需要运行在**拥有 FUSE 挂载的 Linux 主机**上。Windows、macOS、
手机只需要浏览器——不需要 WSL，也不需要 MSYS2 或 Windows 版 gocryptfs-ui。

English: [remote-access.md](remote-access.md)

## 拓扑

```
Windows / macOS / 手机浏览器
        │  https://example.com
        ▼
反向代理（TLS）  ── 同一台 Linux 主机上的 Caddy / Nginx
        │  http://127.0.0.1:30032
        ▼
gocryptfs-web  ──►  gocryptfs-cli  ──►  FUSE 挂载
```

让 `gocryptfs-web` 保持监听 `127.0.0.1`，由反向代理终结 TLS。**不要把 gocryptfs-web 端口直接暴露到
公网**：一个会话就能挂载/卸载所有已配置的加密卷。

## 服务端

放在反向代理后面时，加两个参数启动：

```bash
gocryptfs-web \
  --listen 127.0.0.1:30032 \
  --public-url https://example.com \
  --trust-proxy
```

| 参数 | 作用 |
|---|---|
| `--public-url URL` | 终端二维码、一次性登录链接使用的对外地址（如 `https://example.com`）。不设置时这些链接会用监听地址，从其他设备打不开。 |
| `--trust-proxy` | 信任反代传来的 `X-Forwarded-Proto` / `X-Forwarded-Host` / `X-Forwarded-For`：`https` 时 cookie 自动加 `Secure`；转发 Host 参与 CSRF `Origin` 校验；限流按真实客户端 IP 计数。**只在你自己的反向代理后面启用**。 |

systemd 用户单元：

```bash
mkdir -p ~/.config/systemd/user
cp contrib/systemd/gocryptfs-web.service ~/.config/systemd/user/
# 编辑 ExecStart，追加 --public-url https://example.com --trust-proxy
systemctl --user daemon-reload && systemctl --user enable --now gocryptfs-web
```

## 反向代理示例

### Caddy（自动 HTTPS）

```caddyfile
example.com {
    reverse_proxy 127.0.0.1:30032
}
```

Caddy 会自动申请 Let's Encrypt 证书（主机需对外可达 80/443，且 DNS 指向它）。如果用非标准端口，
例如路由器把 `44332` 转发到本机：

```caddyfile
example.com:44332 {
    reverse_proxy 127.0.0.1:30032
}
```

同时 `--public-url https://example.com:44332`。

仅局域网使用可以用 Caddy 内部 CA：

```caddyfile
example.com {
    tls internal
    reverse_proxy 127.0.0.1:30032
}
```
然后把 Caddy 的根证书（`~/.local/share/caddy/pki/authorities/local/root.crt`）安装到 Windows 客户端，
浏览器才会信任。

### Nginx

```nginx
server {
    listen 443 ssl;
    server_name example.com;

    ssl_certificate     /etc/ssl/example.com/fullchain.pem;
    ssl_certificate_key /etc/ssl/example.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:30032;
        proxy_http_version 1.1;
        proxy_set_header Host              $host;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header X-Forwarded-Host  $host;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_buffering off;          # 挂载/卸载进度使用 SSE
        proxy_read_timeout 3600s;     # 保持长连接
    }
}
```

重载：`sudo nginx -t && sudo systemctl reload nginx`。

### 可信局域网里的纯 HTTP（不推荐）

HTTP 会明文传输 token 和口令。若仅为临时测试接受风险：`--listen 0.0.0.0:30032`（服务端会打印警告），
或加一个不带 TLS 的反代。**即使在局域网，也建议用 TLS**——Caddy `tls internal` 只要五行。

## Windows 客户端

1. 用 Edge 或 Chrome 打开 `https://example.com`。
2. 登录：access token，或用户名/密码（启用了 2FA 再加 TOTP）。
3. 可选：安装为应用 —— Edge：*设置 → 应用 → 将此站点作为应用安装*；
   Chrome：*⋮ → 投放、保存和共享 → 安装页面为应用*。之后开始菜单里有图标、独立窗口。
4. 语言（`EN` / `中文`）与主题可在顶栏切换，选择会保存在浏览器里。

手机（同网络或公网）：

- 顶栏 **手机登录** 生成一次性链接（120 秒、单次有效）与二维码；也可扫服务启动时终端打印的二维码。
  链接由 `--public-url` 生成，所以该地址必须能从手机访问。
- 如果链接是 `http://`，cookie 不会带 `Secure`；检查 `--public-url` 与 `--trust-proxy`。

## 安全检查清单

- [ ] 前面有 TLS 终结的反向代理（只走 `https://`）
- [ ] `gocryptfs-web` 仍然只监听 `127.0.0.1`
- [ ] `--trust-proxy` 只在你自己的反代后面启用
- [ ] 强密码或长随机 token；已在安全页启用 TOTP 2FA
- [ ] 防火墙只放行 443/44332，绝不放行 30032
- [ ] 使用最新版本（`gocryptfs-web --version`）

## 常见问题

| 现象 | 原因 / 处理 |
|---|---|
| 登录成功后会话不保持 | cookie 带了 `Secure` 但浏览器在用 `http://`（或反之）——让 `--public-url` 与浏览器地址完全一致 |
| 403 `origin mismatch` | 反代必须传正确的 `Host`（或配 `--trust-proxy` 并传 `X-Forwarded-Host`） |
| 二维码/一次性链接指向 `http://127.0.0.1:30032` | 设置 `--public-url` 并重启服务 |
| 一次输错密码就把所有人锁住 | 未开 `--trust-proxy` 时所有客户端共享反代 IP；开启后按真实 IP 限流 |
| 看不到挂载进度 | 反代缓冲了响应——Nginx 加 `proxy_buffering off`；Caddy 默认不缓冲 SSE |
| 混合内容 / 登录异常 | 确认页面走 HTTPS，且 `--public-url` 也是 HTTPS |
