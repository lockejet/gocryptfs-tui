#!/bin/sh
# install.sh — 安装 / 卸载 gocryptfs-tui（TUI 二进制 + Shell 后端）
#
# 两种来源：
#   * 本地源码：在仓库里执行 ./install.sh（需要 cargo）
#   * 官方 Release：./install.sh --from-release（不需要源码/工具链），
#     也可直接 `curl -LsSf <release>/gocryptfs-tui-install.sh | sh -s -- ...`
#
# 模式默认自动判断：curl|sh（无源码树）→ Release；仓库内执行 → 本地源码。
# 完整用法见 `./install.sh --help`（仅此一处，避免文档与实现漂移）。
set -eu

REPO="${GOCRYPTFS_TUI_REPO:-lockejet/gocryptfs-tui}"
PREFIX="${PREFIX:-/usr/local}"
DO_BUILD=1
UNINSTALL=0
LINK_CLI="${LINK_CLI:-0}"
LINK_CLI_SET=0
FROM_RELEASE=0
MODE=""          # 空 = 自动；local / release
RELEASE_VER=""
DEPS_CHECK=1

usage() {
    cat <<'USAGE_EOF'
install.sh — 安装 / 卸载 gocryptfs-tui（TUI 二进制 + Shell 后端）

用法：
  ./install.sh                         从本地源码安装（默认前缀 /usr/local，按需 sudo）
  ./install.sh --prefix ~/.local       安装到指定前缀（无需 sudo）
  ./install.sh --system                等价 --prefix /usr/local，并链接独立 CLI
  ./install.sh --local                 强制本地源码模式（需要 cargo）
  ./install.sh --from-release          不编译：从 GitHub Release 下载当前平台二进制
  ./install.sh --from-release v0.3.0   指定版本（等价 --version v0.3.0）
  ./install.sh --uninstall             按安装清单卸载
  ./install.sh --no-build              跳过编译，直接用已有产物安装
  ./install.sh --link-cli              把 gocryptfs-cli 软链到 <prefix>/bin
  ./install.sh --no-link-cli           即使 --system 也不链接
  ./install.sh --no-deps-check         跳过安装后的运行时依赖检查

  # 不需要源码/工具链时（Release 附件）：
  curl -LsSf https://github.com/lockejet/gocryptfs-tui/releases/latest/download/gocryptfs-tui-install.sh \
    | sh -s -- --system

模式默认自动判断：`curl … | sh`（无源码树）→ 从 Release 安装；
在源码仓库里执行 `./install.sh` → 本地编译。可用 `--local` / `--from-release` 强制。

兼容 POSIX sh（dash），因此 `curl ... | sh` 可以直接用。
USAGE_EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --uninstall|-u)  UNINSTALL=1 ;;
        --prefix)        PREFIX="${2:?--prefix 需要目录}"; shift ;;
        --prefix=*)      PREFIX="${1#--prefix=}" ;;
        --system)        PREFIX="/usr/local"
                         # 系统级默认链接独立 CLI（--no-link-cli 可关掉，与顺序无关）
                         if [ "$LINK_CLI_SET" -eq 0 ]; then LINK_CLI=1; fi ;;
        --from-release)  FROM_RELEASE=1; MODE="release"
                         case "${2:-}" in v[0-9]*) RELEASE_VER="$2"; shift ;; esac ;;
        --from-release=*) FROM_RELEASE=1; MODE="release"; RELEASE_VER="${1#--from-release=}" ;;
        --version|-V)    RELEASE_VER="${2:?--version 需要版本号，如 v0.3.0}"; FROM_RELEASE=1; MODE="release"; shift ;;
        --release-version) RELEASE_VER="${2:?--release-version 需要版本号}"; MODE="release"; shift ;;
        --local)         MODE="local" ;;
        --no-build)      DO_BUILD=0
                         if [ -z "$MODE" ]; then MODE="local"; fi ;;
        --link-cli)      LINK_CLI=1; LINK_CLI_SET=1 ;;
        --no-link-cli)   LINK_CLI=0; LINK_CLI_SET=1 ;;
        --no-deps-check) DEPS_CHECK=0 ;;
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
MANIFEST="$LIBDIR/INSTALLED.json"
MANIFEST_FILES="$LIBDIR/INSTALLED.files"

# 系统级前缀默认链接独立 CLI（可用 --no-link-cli 关掉）
if [ "$PREFIX" = "/usr/local" ] && [ "$LINK_CLI_SET" -eq 0 ]; then
    LINK_CLI=1
fi

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
SUDO=""
if [ ! -w "$probe" ]; then
    if [ "$(id -u)" -eq 0 ]; then
        echo "[!] 检测到以 root 运行；本脚本需要普通用户身份运行。" >&2
        echo "    正确用法: ./install.sh（脚本内部会按需 sudo）" >&2
        exit 1
    fi
    command -v sudo >/dev/null 2>&1 || {
        echo "[!] $PREFIX 不可写且未安装 sudo" >&2
        exit 1
    }
    SUDO="sudo"
fi

priv() { if [ -n "$SUDO" ]; then sudo "$@"; else "$@"; fi; }

STAGE="$(mktemp -d)"
DL=""
cleanup() { rm -rf "$STAGE"; if [ -n "$DL" ]; then rm -rf "$DL"; fi; }
trap cleanup EXIT

# ------------------------------------------------------------
# 工具函数
# ------------------------------------------------------------
have() { command -v "$1" >/dev/null 2>&1; }

download() { # download <url> <dest>
    if have curl; then
        curl -fsSL --proto '=https' --tlsv1.2 -o "$2" "$1"
    elif have wget; then
        wget -qO "$2" "$1"
    else
        echo "[!] 需要 curl 或 wget 才能 --from-release" >&2
        return 1
    fi
}

json_escape() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

# ------------------------------------------------------------
# 卸载：优先按安装清单精确删除
# ------------------------------------------------------------
if [ "$UNINSTALL" -eq 1 ]; then
    echo "==> 卸载 (PREFIX=$PREFIX)"
    removed=0
    if [ -f "$MANIFEST_FILES" ]; then
        echo "    按清单: $MANIFEST_FILES"
        while IFS= read -r f; do
            [ -n "$f" ] || continue
            case "$f" in
                "$PREFIX"/*) ;;   # 只允许删除前缀内的路径
                *) echo "[!] 跳过清单外的路径: $f" >&2; continue ;;
            esac
            if [ -e "$f" ] || [ -L "$f" ]; then
                priv rm -f "$f"
                echo "  删除 $f"
                removed=1
            fi
        done < "$MANIFEST_FILES"
        priv rm -f "$MANIFEST" "$MANIFEST_FILES"
    else
        echo "    （未找到安装清单，按默认路径卸载）"
    fi
    # 无论有无清单都清理已知路径，并删除空的 lib 目录
    priv rm -f "$BINDIR/gocryptfs-cli" "$BINDIR/gocryptfs-tui"
    priv rm -rf "$LIBDIR"
    # 只在确认为空时才删父目录（非空会失败，不影响其它文件）
    priv rmdir "$PREFIX/lib" >/dev/null 2>&1 || true
    priv rmdir "$BINDIR" >/dev/null 2>&1 || true
    echo "[✔] 已卸载："
    echo "  $BINDIR/gocryptfs-tui"
    echo "  $BINDIR/gocryptfs-cli"
    echo "  $LIBDIR"
    echo ""
    echo "[·] 配置与数据目录未删除（按需自行清理）："
    echo "  ${XDG_CONFIG_HOME:-$HOME/.config}/gocryptfs-tui/"
    echo "  ${XDG_DATA_HOME:-$HOME/.local/share}/gocryptfs-tui/"
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
# ---- 运行时依赖（仅警告；真正的判定交给 TUI 的 --check-deps）----
echo "==> 检查依赖"
missing_deps=""
for cmd in gocryptfs fusermount rsync yq jq mountpoint; do
    have "$cmd" || missing_deps="$missing_deps $cmd"
done
if [ -n "$missing_deps" ]; then
    echo "[!] 缺少运行时依赖:${missing_deps}"
    echo "    安装会继续；装完后按 'gocryptfs-tui --check-deps' 的提示补齐即可。"
else
    echo "[✔] 依赖齐全"
fi

# ---- 模式自动判断 ----
# curl|sh 时没有源码树（$0 也不是可读文件），必须走 Release；否则在源码目录里就本地编译。
if [ -z "$MODE" ]; then
    if [ -f Cargo.toml ] && [ -f src/main.rs ] && [ -f "$0" ] && [ -r "$0" ]; then
        MODE="local"
    else
        MODE="release"
    fi
fi
if [ "$MODE" = "release" ]; then FROM_RELEASE=1; fi

# ---- 取二进制与 Shell 后端：本地构建 或 官方 Release ----
BIN_SRC=""
SHELL_DIR=""
SOURCE_DESC=""
BIN_VERSION=""

if [ "$FROM_RELEASE" -eq 1 ]; then
    DL="$(mktemp -d)"

    # 目标三元组
    case "$(uname -m)" in
        x86_64|amd64)   arch="x86_64" ;;
        aarch64|arm64)  arch="aarch64" ;;
        *) echo "[!] 不支持的架构: $(uname -m)" >&2; exit 1 ;;
    esac
    if have ldd && ldd --version 2>&1 | grep -qi musl; then libc="musl"; else libc="gnu"; fi
    target="${arch}-unknown-linux-${libc}"
    asset="gocryptfs-tui-${target}.tar.xz"

    if [ -n "$RELEASE_VER" ]; then
        base="https://github.com/$REPO/releases/download/$RELEASE_VER"
        ver_label="$RELEASE_VER"
    else
        base="https://github.com/$REPO/releases/latest/download"
        # 解析 latest 实际指向的 tag（失败则退回 "latest"）
        ver_label="$(curl -fsSIL -o /dev/null -w '%{url_effective}' "$base/$asset" 2>/dev/null \
            | sed -n 's#.*/releases/download/\([^/]*\)/.*#\1#p' | head -1)"
        [ -n "$ver_label" ] || ver_label="latest"
    fi

    echo "==> 安装来源: Release（$ver_label, $target）"
    if ! download "$base/$asset" "$DL/$asset"; then
        echo "[!] 下载失败: $base/$asset" >&2
        echo "    可手动下载: https://github.com/$REPO/releases" >&2
        exit 1
    fi
    if download "$base/$asset.sha256" "$DL/$asset.sha256" 2>/dev/null; then
        ( cd "$DL" && sha256sum -c "$asset.sha256" >/dev/null ) \
            || { echo "[!] sha256 校验失败: $asset" >&2; exit 1; }
        echo "[✔] sha256 校验通过"
    else
        echo "[!] 未取到 .sha256，跳过校验" >&2
    fi
    tar -xJf "$DL/$asset" -C "$DL"
    BIN_SRC="$DL/${asset%.tar.xz}/gocryptfs-tui"
    [ -f "$BIN_SRC" ] || { echo "[!] 归档里没有 gocryptfs-tui" >&2; exit 1; }

    # 独立后端：从同一 Release 的源码包取 shell/
    echo "==> 获取 Shell 后端（source.tar.gz）"
    if download "$base/source.tar.gz" "$DL/source.tar.gz"; then
        if download "$base/source.tar.gz.sha256" "$DL/source.tar.gz.sha256" 2>/dev/null; then
            ( cd "$DL" && sha256sum -c "source.tar.gz.sha256" >/dev/null ) \
                || { echo "[!] source.tar.gz 校验失败" >&2; exit 1; }
        fi
        tar -xzf "$DL/source.tar.gz" -C "$DL"
        SHELL_DIR="$(find "$DL" -maxdepth 2 -type d -name shell | head -1)"
    fi
    if [ -z "$SHELL_DIR" ] || [ ! -f "$SHELL_DIR/gocryptfs-cli" ]; then
        echo "[!] 未能从源码包取到 shell/，将只安装 TUI（独立 CLI 缺失）" >&2
        SHELL_DIR=""
    fi
    SOURCE_DESC="release:$ver_label"
else
    if [ "$DO_BUILD" -eq 1 ]; then
        if ! have cargo; then
            echo "[!] 本地模式需要 Rust 工具链，但未找到 cargo。" >&2
            echo "    没有源码/工具链时请从 Release 安装：" >&2
            echo "      curl -LsSf https://github.com/$REPO/releases/latest/download/gocryptfs-tui-install.sh \\" >&2
            echo "        | sh -s -- --from-release --system" >&2
            exit 1
        fi
        echo "==> 编译 gocryptfs-tui（本地源码模式）"
        cargo build --release
    else
        echo "==> 跳过编译（--no-build）"
    fi
    BIN_SRC="target/release/gocryptfs-tui"
    SHELL_DIR="shell"
    SOURCE_DESC="local:$BIN_SRC"
    echo "==> 安装来源: 本地源码（$BIN_SRC）"
    [ -f "$BIN_SRC" ] || { echo "[!] 未找到 $BIN_SRC（先 make build 或去掉 --no-build）" >&2; exit 1; }
    if [ ! -f "$SHELL_DIR/gocryptfs-cli" ] || [ ! -f "$SHELL_DIR/lib/gocryptfs-lib.sh" ]; then
        echo "[!] 缺少 shell/ 或 shell/lib/（需要 gocryptfs-lib.sh 与 i18n.sh）" >&2
        exit 1
    fi
fi

BIN_VERSION="$("$BIN_SRC" --version 2>/dev/null | sed -n 's/^gocryptfs-tui //p' || true)"
[ -n "$BIN_VERSION" ] || BIN_VERSION="$SOURCE_DESC"

# ---- 清理旧安装并安装 ----
echo "==> 安装到 $PREFIX"
priv mkdir -p "$LIBDIR/lib" "$BINDIR"
priv rm -rf "$LIBDIR"
priv mkdir -p "$LIBDIR/lib"

# 已安装文件清单（供 INSTALLED.files / --uninstall 使用）
: > "$STAGE/files"
record() { printf '%s\n' "$1" >> "$STAGE/files"; }

if [ -n "$SHELL_DIR" ]; then
    cp "$SHELL_DIR/lib/"*.sh "$STAGE/"
    cp "$SHELL_DIR/gocryptfs-cli" "$STAGE/"
    chmod 0644 "$STAGE"/*.sh
    chmod 0755 "$STAGE/gocryptfs-cli"
    priv cp -f "$STAGE"/*.sh "$LIBDIR/lib/"
    priv cp -f "$STAGE/gocryptfs-cli" "$LIBDIR/gocryptfs-cli"
    record "$LIBDIR/gocryptfs-cli"
    for f in "$SHELL_DIR"/lib/*.sh; do record "$LIBDIR/lib/$(basename "$f")"; done
fi
priv cp -f "$BIN_SRC" "$BINDIR/gocryptfs-tui"
priv chmod 0755 "$BINDIR/gocryptfs-tui" "$LIBDIR/gocryptfs-cli" 2>/dev/null || true
record "$BINDIR/gocryptfs-tui"

printf '%s\n' "$BIN_VERSION" > "$STAGE/VERSION"
priv cp -f "$STAGE/VERSION" "$LIBDIR/VERSION"
record "$LIBDIR/VERSION"

if [ "$LINK_CLI" = "1" ] && [ -n "$SHELL_DIR" ]; then
    priv ln -sf "$LIBDIR/gocryptfs-cli" "$BINDIR/gocryptfs-cli"
    record "$BINDIR/gocryptfs-cli"
elif [ "$LINK_CLI" = "1" ]; then
    echo "[!] 没有独立后端可链接（--from-release 未取到 shell/）"
fi

# ---- 写安装清单（供 --uninstall 精确删除）----
{
    printf '{\n'
    printf '  "app": "gocryptfs-tui",\n'
    printf '  "version": "%s",\n' "$(json_escape "$BIN_VERSION")"
    printf '  "prefix": "%s",\n' "$(json_escape "$PREFIX")"
    printf '  "source": "%s",\n' "$(json_escape "$SOURCE_DESC")"
    printf '  "installed_at": "%s",\n' "$(date -Iseconds)"
    printf '  "files": [\n'
    n=0
    total=$(wc -l < "$STAGE/files" | tr -d ' ')
    while IFS= read -r f; do
        n=$((n + 1))
        sep=","
        if [ "$n" -eq "$total" ]; then sep=""; fi
        printf '    "%s"%s\n' "$(json_escape "$f")" "$sep"
    done < "$STAGE/files"
    printf '  ]\n'
    printf '}\n'
} > "$STAGE/INSTALLED.json"
priv cp -f "$STAGE/INSTALLED.json" "$MANIFEST"
priv cp -f "$STAGE/files" "$MANIFEST_FILES"

# ---- 自检 ----
echo ""
echo "==> 自检"
if [ -n "$SHELL_DIR" ]; then
    if "$LIBDIR/gocryptfs-cli" --help 2>&1 | grep -q -- '--lang'; then
        echo "[✔] Shell 后端支持 --lang（i18n 已启用）"
    else
        echo "[!] Shell 后端未提供 --lang，请确认复制的是最新 shell/lib/"
    fi
fi
if [ "$LINK_CLI" = "1" ] && [ -n "$SHELL_DIR" ]; then
    resolved="$(command -v gocryptfs-cli 2>/dev/null || true)"
    if [ -z "$resolved" ]; then
        echo "[!] PATH 中没有 gocryptfs-cli；请把 $BINDIR 加入 PATH"
    elif [ "$resolved" != "$BINDIR/gocryptfs-cli" ]; then
        echo "[!] PATH 优先解析到: $resolved"
        echo "    不是本次安装的 $BINDIR/gocryptfs-cli，可能调用到旧后端（语言/文案不一致）"
    else
        echo "[✔] PATH 解析到本次安装的 gocryptfs-cli"
    fi
elif [ -n "$SHELL_DIR" ]; then
    echo "[·] 未链接 gocryptfs-cli 到 PATH（默认行为）。规范位置:"
    echo "      $LIBDIR/gocryptfs-cli"
    echo "    想直接用命令行版可执行："
    echo "      ln -sf $LIBDIR/gocryptfs-cli $BINDIR/gocryptfs-cli"
    echo "      （TUI 用的是内嵌后端，不需要这一步）"
fi

if [ "$DEPS_CHECK" -eq 1 ]; then
    echo ""
    echo "==> 运行时依赖检查"
    if "$BINDIR/gocryptfs-tui" --check-deps; then
        :
    else
        rc=$?
        if [ "$rc" -eq 1 ]; then
            echo ""
            echo "[!] 缺少运行时依赖，请按上面的安装命令补齐后再运行 gocryptfs-tui"
        elif [ -n "$SHELL_DIR" ]; then
            # 旧版二进制不认识 --check-deps：退回 Shell 后端的检查
            echo "[·] 该 TUI 二进制不支持 --check-deps，改用 Shell 后端检查"
            "$LIBDIR/gocryptfs-cli" --check-deps \
                || echo "[!] 缺少运行时依赖，请按上面的提示安装"
        fi
    fi
fi

echo ""
echo "[✔] 安装完成（来源: $SOURCE_DESC，版本: $BIN_VERSION）"
echo "  TUI:  $BINDIR/gocryptfs-tui"
if [ "$LINK_CLI" = "1" ] && [ -n "$SHELL_DIR" ]; then
    echo "  CLI:  $BINDIR/gocryptfs-cli -> $LIBDIR/gocryptfs-cli"
elif [ -n "$SHELL_DIR" ]; then
    echo "  CLI:  $LIBDIR/gocryptfs-cli（未链接到 PATH；加 --link-cli 可链接）"
else
    echo "  CLI:  （仅内嵌后端，首次运行 TUI 时释放到数据目录）"
fi
echo "  清单: $MANIFEST"
echo ""
echo "==> 文件清单"
find "$LIBDIR" -type f | sort
echo ""
echo "直接运行:  gocryptfs-tui"
if [ -f "$0" ] && [ -r "$0" ]; then
    echo "卸载:      $0 --uninstall --prefix $PREFIX"
else
    echo "卸载:      curl -LsSf https://github.com/$REPO/releases/latest/download/gocryptfs-tui-install.sh \\"
    echo "             | sh -s -- --uninstall --prefix $PREFIX"
fi
