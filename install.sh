#!/bin/bash
set -euo pipefail

PREFIX="${PREFIX:-/usr/local}"
LIBDIR="$PREFIX/lib/gocryptfs-tui"
BINDIR="$PREFIX/bin"

# ---- 必须在非 root 下运行（编译需要用户 cargo）----
if [ "$EUID" -eq 0 ]; then
    echo "[!] 请勿用 sudo 运行本脚本。"
    echo "    正确用法：先 cargo build --release，再 ./install.sh"
    echo "    或：bash install.sh（脚本内部会按需 sudo）"
    exit 1
fi

# ---- 依赖检查 ----
echo "==> 检查依赖"
for cmd in gocryptfs fusermount rsync yq jq; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "[!] 缺少: $cmd" >&2
        exit 1
    fi
done

# ---- 编译（用户身份）----
if [ ! -f target/release/gocryptfs-tui ]; then
    echo "==> 编译 gocryptfs-tui"
    cargo build --release
else
    echo "==> 使用已存在的编译产物 (如需重新编译，先 cargo build --release)"
fi

# ---- 检查产物 ----
if [ ! -f target/release/gocryptfs-tui ]; then
    echo "[!] 未找到 target/release/gocryptfs-tui" >&2
    exit 1
fi
if [ ! -f shell/gocryptfs-cli ] || [ ! -f shell/lib/gocryptfs-lib.sh ]; then
    echo "[!] 缺少 shell/ 或 shell/lib/" >&2
    exit 1
fi

# ---- 清理旧安装（sudo）----
echo "==> 清理旧安装"
sudo rm -rf "$LIBDIR"
sudo rm -f "$BINDIR/gocryptfs-cli"
sudo rm -f "$BINDIR/gocryptfs-tui"

# ---- 安装 shell CLI（sudo）----
echo "==> 安装 shell CLI 到 $LIBDIR"
sudo mkdir -p "$LIBDIR/lib"
sudo cp shell/lib/*.sh "$LIBDIR/lib/"
sudo cp shell/gocryptfs-cli "$LIBDIR/"
sudo chmod +x "$LIBDIR/gocryptfs-cli"

# ---- 创建符号链接（sudo）----
echo "==> 创建 $BINDIR/gocryptfs-cli 符号链接"
sudo ln -sf "$LIBDIR/gocryptfs-cli" "$BINDIR/gocryptfs-cli"

# ---- 安装 TUI 二进制（sudo）----
echo "==> 安装 TUI 二进制到 $BINDIR"
sudo cp target/release/gocryptfs-tui "$BINDIR/"
sudo chmod +x "$BINDIR/gocryptfs-tui"

# ---- 验证 ----
echo ""
echo "[✔] 安装完成"
echo "  CLI:  $BINDIR/gocryptfs-cli"
echo "  TUI:  $BINDIR/gocryptfs-tui"
echo ""
echo "==> 文件清单"
find "$LIBDIR" -type f
echo ""
echo "直接运行:  gocryptfs-tui"
