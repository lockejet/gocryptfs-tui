# Accessing the Web UI from other devices

The Web UI is a plain browser client: the server only has to run on the **Linux host that owns
the FUSE mounts**. Windows, macOS and phones just need a browser — no WSL, no MSYS2, no
Windows build of gocryptfs-ui.

中文版：[remote-access.zh-CN.md](remote-access.zh-CN.md)

## Topology

```
Windows / macOS / phone browser
        │  https://example.com
        ▼
reverse proxy (TLS)  ── Caddy / Nginx on the same Linux host
        │  http://127.0.0.1:30032
        ▼
gocryptfs-web  ──►  gocryptfs-cli  ──►  FUSE mounts
```

Keep `gocryptfs-web` bound to `127.0.0.1` and let the proxy terminate TLS. Never expose the
gocryptfs-web port directly to the internet: a session can mount/unmount every configured vault.

## Server side

Run the server with two extra flags when it sits behind a reverse proxy:

```bash
gocryptfs-web \
  --listen 127.0.0.1:30032 \
  --public-url https://example.com \
  --trust-proxy
```

| Flag | Purpose |
|---|---|
| `--public-url URL` | Base URL used in the terminal QR code and one-time login links (e.g. `https://example.com`). Without it those links use the listen address, which is useless from other devices. |
| `--trust-proxy` | Honor `X-Forwarded-Proto` / `X-Forwarded-Host` / `X-Forwarded-For` from the proxy: `https` adds the `Secure` cookie flag, forwarded host participates in the CSRF `Origin` check, and rate limiting uses the real client IP. Only enable it behind **your own** proxy. |

systemd user unit:

```bash
mkdir -p ~/.config/systemd/user
cp contrib/systemd/gocryptfs-web.service ~/.config/systemd/user/
# edit ExecStart to add --public-url https://example.com --trust-proxy
systemctl --user daemon-reload && systemctl --user enable --now gocryptfs-web
```

## Reverse proxy examples

### Caddy (automatic HTTPS)

```caddyfile
example.com {
    reverse_proxy 127.0.0.1:30032
}
```

Caddy obtains a Let's Encrypt certificate automatically (the host must be reachable on ports 80/443
and the DNS name must point at it). If you use a non-standard port, e.g. your router forwards
`44332` to this host:

```caddyfile
example.com:44332 {
    reverse_proxy 127.0.0.1:30032
}
```

and use `--public-url https://example.com:44332`.

For a LAN-only setup you can use Caddy's internal CA instead of Let's Encrypt:

```caddyfile
example.com {
    tls internal
    reverse_proxy 127.0.0.1:30032
}
```
then install Caddy's root CA (`~/.local/share/caddy/pki/authorities/local/root.crt`) on the Windows
client so the browser trusts it.

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
        proxy_buffering off;          # server-sent events (mount/umount progress)
        proxy_read_timeout 3600s;     # keep long SSE connections open
    }
}
```

Reload with `sudo nginx -t && sudo systemctl reload nginx`.

### Plain HTTP on a trusted LAN (not recommended)

HTTP sends tokens and passphrases in clear text. If you accept that risk for a quick test, either
`--listen 0.0.0.0:30032` (the server prints a warning) or add a proxy without TLS. Prefer a TLS
proxy even on a LAN — Caddy with `tls internal` is five lines.

## Windows client

1. Open Edge or Chrome → `https://example.com`.
2. Sign in: access token, or username/password (+ TOTP if you enabled 2FA).
3. Optional: install it as an app — Edge: *Settings → Apps → Install this site as an app*;
   Chrome: *⋮ → Cast, save and share → Install page as app*. It then has a Start-menu icon and its
   own window.
4. Theme and language (`EN` / `中文`) can be switched in the header; the choice is stored in the
   browser.

Phone (same network or internet):

- Header **Phone login** generates a one-time link (120 s, single use) with a QR code, or scan the
  QR printed at server startup. The link is built from `--public-url`, so it must be reachable from
  the phone.
- If the link is `http://` the cookie is not `Secure`; check `--public-url` and `--trust-proxy`.

## Security checklist

- [ ] TLS terminating proxy in front (`https://` only)
- [ ] `gocryptfs-web` still listening on `127.0.0.1`
- [ ] `--trust-proxy` only behind your own proxy
- [ ] strong password or long random token; TOTP 2FA enabled (Security page)
- [ ] firewall: expose only 443/44332, never 30032
- [ ] update to the latest release (`gocryptfs-web --version`)

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| Login works but the session does not stick | cookie is `Secure` while the browser is on `http://` (or vice versa) — make `--public-url` match the browser URL exactly |
| 403 `origin mismatch` | proxy must pass a correct `Host` (or `X-Forwarded-Host` with `--trust-proxy`) |
| QR / one-time link points at `http://127.0.0.1:30032` | set `--public-url`, restart the service |
| One wrong password locks everybody out | without `--trust-proxy` all clients share the proxy IP; enable it so rate limiting sees real IPs |
| Mount progress never appears | proxy buffers responses — `proxy_buffering off` (Nginx) / Caddy does not buffer SSE by default |
| Mixed content / broken login | make sure the page is loaded over HTTPS and `--public-url` is HTTPS too |
