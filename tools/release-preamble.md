**安装 / Install** — 用户级装到 `~/.local/bin`（默认，无需 sudo），系统级加 `--system` 装到 `/usr/local/bin`：

```bash
curl -LsSf https://github.com/lockejet/gocryptfs-ui/releases/latest/download/gocryptfs-tui-install.sh | sh
curl -LsSf https://github.com/lockejet/gocryptfs-ui/releases/latest/download/gocryptfs-tui-install.sh | sh -s -- --system
```

预编译包（x86_64/aarch64 glibc、x86_64 musl）、`source.tar.gz` 与 sha256 校验和见下方 Assets；git clone 与源码编译两种方式见 [README](https://github.com/lockejet/gocryptfs-ui#installation)。
