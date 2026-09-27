#!/bin/bash
# install.sh: 安装 gocryptfs-tui 与 gocryptfs-cli

set -euo pipefail

PREFIX="${PREFIX:-/usr/local}"
LIBDIR="$PREFIX/lib/gocryptfs-tui"
BINDIR="$PREFIX/bin"

# 检查依赖
echo "==> 检查依赖"
for cmd in gocryptfs fusermount rsync yq jq; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "[!] 缺少: $cmd" >&2
        echo "    Debian/Ubuntu: apt install gocryptfs rsync jq" >&2
        echo "    yq: https://github.com/mikefarah/yq" >&2
        exit 1
    fi
done

# 编译 Rust
if [ -f Cargo.toml ]; then
    echo "==> 编译 gocryptfs-tui"
    cargo build --release
fi

# 安装 shell
echo "==> 安装 shell CLI 到 $LIBDIR"
sudo mkdir -p "$LIBDIR/lib"
sudo cp -r shell/lib/* "$LIBDIR/lib/"
sudo cp shell/gocryptfs-cli "$LIBDIR/"
sudo chmod +x "$LIBDIR/gocryptfs-cli"

# 创建符号链接
echo "==> 创建 /usr/local/bin/gocryptfs-cli 符号链接"
sudo ln -sf "$LIBDIR/gocryptfs-cli" "$BINDIR/gocryptfs-cli"

# 安装 Rust 二进制
if [ -f target/release/gocryptfs-tui ]; then
    echo "==> 安装 gocryptfs-tui 到 $BINDIR"
    sudo cp target/release/gocryptfs-tui "$BINDIR/"
    sudo chmod +x "$BINDIR/gocryptfs-tui"
fi

echo ""
echo "[✔] 安装完成"
echo "  CLI:  $BINDIR/gocryptfs-cli"
echo "  TUI:  $BINDIR/gocryptfs-tui"
echo ""
echo "首次使用请创建配置文件:"
echo "  mkdir -p ~/.config/gocryptfs-tui"
echo "  cp examples/config.yaml.example ~/.config/gocryptfs-tui/config.yaml"
