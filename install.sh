#!/bin/bash
# install.sh — 安装 / 卸载 gocryptfs-tui（TUI 二进制 + Shell 后端）
#
# 用法：
#   ./install.sh                     安装到 /usr/local（按需 sudo）
#   ./install.sh --prefix ~/.local   安装到指定前缀（无需 sudo）
#   ./install.sh --uninstall         卸载（默认 /usr/local）
#   ./install.sh --uninstall --prefix ~/.local
#   ./install.sh --no-build          跳过编译，直接用已有产物安装
#
# 说明：
#   * TUI 与 Shell 后端必须成套安装，否则 TUI 可能调用到旧版 gocryptfs-cli，
#     出现「界面已是英文、输出区仍是中文」这类不一致。
#   * 目标不可写时才使用 sudo；请勿直接用 sudo 运行本脚本（会用你的 cargo 编译）。
set -euo pipefail

PREFIX="${PREFIX:-/usr/local}"
DO_BUILD=1
UNINSTALL=0

usage() {
    sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
    case "$1" in
        --uninstall|-u)  UNINSTALL=1 ;;
        --prefix)        PREFIX="${2:?--prefix 需要目录}"; shift ;;
        --prefix=*)      PREFIX="${1#--prefix=}" ;;
        --no-build)      DO_BUILD=0 ;;
        -h|--help)       usage; exit 0 ;;
        *) echo "[!] 未知参数: $1" >&2; usage >&2; exit 2 ;;
    esac
    shift
done

case "$PREFIX" in
    ""|"/") echo "[!] 非法的 PREFIX: '${PREFIX}'" >&2; exit 1 ;;
esac
PREFIX="${PREFIX%/}"

LIBDIR="$PREFIX/lib/gocryptfs-tui"
BINDIR="$PREFIX/bin"

# 只删除固定的 gocryptfs-tui 目录，避免误删
case "$LIBDIR" in
    */lib/gocryptfs-tui) ;;
    *) echo "[!] 拒绝操作异常路径: $LIBDIR" >&2; exit 1 ;;
esac

# ---- 仅在目标不可写时提权 ----
probe="$PREFIX"
while [ ! -e "$probe" ] && [ "$probe" != "/" ]; do
    probe="$(dirname "$probe")"
done
SUDO=()
if [ ! -w "$probe" ]; then
    if [ "$EUID" -eq 0 ]; then
        echo "[!] 检测到以 root 运行；本脚本需要普通用户身份编译。" >&2
        echo "    正确用法: ./install.sh（脚本内部会按需 sudo）" >&2
        exit 1
    fi
    command -v sudo >/dev/null 2>&1 || {
        echo "[!] $PREFIX 不可写且未安装 sudo" >&2
        exit 1
    }
    SUDO=(sudo)
fi

priv() { if [ ${#SUDO[@]} -gt 0 ]; then sudo "$@"; else "$@"; fi; }

# ------------------------------------------------------------
# 卸载
# ------------------------------------------------------------
if [ "$UNINSTALL" -eq 1 ]; then
    echo "==> 卸载 (PREFIX=$PREFIX)"
    priv rm -f "$BINDIR/gocryptfs-cli"
    priv rm -f "$BINDIR/gocryptfs-tui"
    priv rm -rf "$LIBDIR"
    echo "[✔] 已卸载："
    echo "  $BINDIR/gocryptfs-tui"
    echo "  $BINDIR/gocryptfs-cli"
    echo "  $LIBDIR"
    remaining="$(command -v gocryptfs-cli 2>/dev/null || true)"
    if [ -n "$remaining" ]; then
        echo ""
        echo "[!] PATH 中仍能解析到: $remaining"
        echo "    若那是另一份旧安装，建议一并卸载，避免 TUI 调用到旧后端。"
    fi
    exit 0
fi

# ------------------------------------------------------------
# 安装
# ------------------------------------------------------------
# ---- 依赖检查（仅安装时）----
echo "==> 检查依赖"
for cmd in gocryptfs fusermount rsync yq jq; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "[!] 缺少: $cmd" >&2
        exit 1
    fi
done

# ---- 编译（用户身份）----
if [ "$DO_BUILD" -eq 1 ]; then
    echo "==> 编译 gocryptfs-tui"
    cargo build --release
else
    echo "==> 跳过编译（--no-build）"
fi

if [ ! -f target/release/gocryptfs-tui ]; then
    echo "[!] 未找到 target/release/gocryptfs-tui" >&2
    exit 1
fi
if [ ! -f shell/gocryptfs-cli ] || [ ! -f shell/lib/gocryptfs-lib.sh ] || [ ! -f shell/lib/i18n.sh ]; then
    echo "[!] 缺少 shell/ 或 shell/lib/（需要 gocryptfs-lib.sh 与 i18n.sh）" >&2
    exit 1
fi

# ---- 清理旧安装并安装 ----
echo "==> 安装到 $PREFIX"
priv mkdir -p "$LIBDIR/lib" "$BINDIR"
priv rm -rf "$LIBDIR"
priv mkdir -p "$LIBDIR/lib"
# 先复制到临时目录再移动，避免 sudo 与普通用户混用时权限错乱
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
cp shell/lib/*.sh "$STAGE/"
cp shell/gocryptfs-cli "$STAGE/"
chmod 0644 "$STAGE"/*.sh
chmod 0755 "$STAGE/gocryptfs-cli"
priv cp -f "$STAGE"/*.sh "$LIBDIR/lib/"
priv cp -f "$STAGE/gocryptfs-cli" "$LIBDIR/gocryptfs-cli"
priv cp -f target/release/gocryptfs-tui "$BINDIR/gocryptfs-tui"
priv chmod 0755 "$BINDIR/gocryptfs-tui" "$LIBDIR/gocryptfs-cli"
priv ln -sf "$LIBDIR/gocryptfs-cli" "$BINDIR/gocryptfs-cli"

# ---- 自检 ----
echo ""
echo "==> 自检"
if "$BINDIR/gocryptfs-cli" --help 2>&1 | grep -q -- '--lang'; then
    echo "[✔] Shell 后端支持 --lang（i18n 已启用）"
else
    echo "[!] Shell 后端未提供 --lang，请确认复制的是最新 shell/lib/"
fi
resolved="$(command -v gocryptfs-cli 2>/dev/null || true)"
if [ -z "$resolved" ]; then
    echo "[!] PATH 中没有 gocryptfs-cli；请把 $BINDIR 加入 PATH"
elif [ "$resolved" != "$BINDIR/gocryptfs-cli" ]; then
    echo "[!] PATH 优先解析到: $resolved"
    echo "    不是本次安装的 $BINDIR/gocryptfs-cli，TUI 可能调用到旧后端（语言/文案不一致）"
else
    echo "[✔] PATH 解析到本次安装的 gocryptfs-cli"
fi

echo ""
echo "==> 运行时依赖检查"
if "$BINDIR/gocryptfs-tui" --check-deps; then
    :
else
    rc=$?
    if [ "$rc" -eq 1 ]; then
        echo ""
        echo "[!] 缺少运行时依赖，请按上面的安装命令补齐后再运行 gocryptfs-tui"
    else
        # 旧版二进制不认识 --check-deps：退回 Shell 后端的检查
        echo "[·] 该 TUI 二进制不支持 --check-deps，改用 Shell 后端检查"
        "$BINDIR/gocryptfs-cli" --check-deps \
            || echo "[!] 缺少运行时依赖，请按上面的提示安装"
    fi
fi

echo ""
echo "[✔] 安装完成"
echo "  TUI:  $BINDIR/gocryptfs-tui"
echo "  CLI:  $BINDIR/gocryptfs-cli -> $LIBDIR/gocryptfs-cli"
echo ""
echo "==> 文件清单"
find "$LIBDIR" -type f | sort
echo ""
echo "直接运行:  gocryptfs-tui"
