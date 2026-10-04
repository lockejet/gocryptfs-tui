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
PREFIX="${PREFIX:-$HOME/.local}"
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
install.sh — install / uninstall gocryptfs-tui (TUI binary + shell backend)

Usage (one script, three modes):
  lazy    curl -LsSf <release>/gocryptfs-tui-install.sh | sh -s -- [--user|--system]
  clone   git clone … && ./install.sh --from-release [--user|--system]
  source  git clone … && make build && ./install.sh --local [--user|--system]

Prefix (default: --user):
  --user              install to ~/.local (default; no sudo)
  --system            install to /usr/local and link the standalone CLI (sudo as needed)
  --prefix DIR        custom prefix

Mode and uninstall:
  ./install.sh --local                 force local-source mode (needs cargo)
  ./install.sh --from-release          no build: fetch the platform binary from GitHub Releases
  ./install.sh --from-release v0.4.2   pin a version (same as --version v0.4.2)
  ./install.sh --uninstall             uninstall using the install manifest
  ./install.sh --no-build              skip compilation, use existing artifacts
  ./install.sh --link-cli              symlink gocryptfs-cli into <prefix>/bin
  ./install.sh --no-link-cli           never link, even with --system
  ./install.sh --no-deps-check         skip the post-install dependency check

  # no source/toolchain needed (GitHub Release asset):
  curl -LsSf https://github.com/lockejet/gocryptfs-tui/releases/latest/download/gocryptfs-tui-install.sh \
    | sh -s -- --system

The mode is auto-detected: `curl … | sh` (no source tree) installs from the Release;
running `./install.sh` inside a checkout builds locally. Force with --local / --from-release.
POSIX sh (dash) compatible, so `curl ... | sh` works as-is.
USAGE_EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --uninstall|-u)  UNINSTALL=1 ;;
        --prefix)        PREFIX="${2:?--prefix needs a directory}"; shift ;;
        --prefix=*)      PREFIX="${1#--prefix=}" ;;
        --user)          PREFIX="$HOME/.local" ;;
        --system)        PREFIX="/usr/local"
                         # 系统级默认链接独立 CLI（--no-link-cli 可关掉，与顺序无关）
                         if [ "$LINK_CLI_SET" -eq 0 ]; then LINK_CLI=1; fi ;;
        --from-release)  FROM_RELEASE=1; MODE="release"
                         case "${2:-}" in v[0-9]*) RELEASE_VER="$2"; shift ;; esac ;;
        --from-release=*) FROM_RELEASE=1; MODE="release"; RELEASE_VER="${1#--from-release=}" ;;
        --version|-V)    RELEASE_VER="${2:?--version needs a version, e.g. v0.4.2}"; FROM_RELEASE=1; MODE="release"; shift ;;
        --release-version) RELEASE_VER="${2:?--release-version needs a version}"; MODE="release"; shift ;;
        --local)         MODE="local" ;;
        --no-build)      DO_BUILD=0
                         if [ -z "$MODE" ]; then MODE="local"; fi ;;
        --link-cli)      LINK_CLI=1; LINK_CLI_SET=1 ;;
        --no-link-cli)   LINK_CLI=0; LINK_CLI_SET=1 ;;
        --no-deps-check) DEPS_CHECK=0 ;;
        -h|--help)       usage; exit 0 ;;
        *) echo "[!] unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
    shift
done

case "$PREFIX" in
    ""|"/") echo "[!] invalid PREFIX: '${PREFIX}'" >&2; exit 1 ;;
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
    *) echo "[!] refusing to operate on unexpected path: $LIBDIR" >&2; exit 1 ;;
esac

# ---- 仅在目标不可写时提权 ----
probe="$PREFIX"
while [ ! -e "$probe" ] && [ "$probe" != "/" ]; do
    probe="$(dirname "$probe")"
done
SUDO=""
if [ ! -w "$probe" ]; then
    if [ "$(id -u)" -eq 0 ]; then
        echo "[!] running as root; this script must run as a normal user." >&2
        echo "    correct usage: ./install.sh (it uses sudo internally when needed)" >&2
        exit 1
    fi
    command -v sudo >/dev/null 2>&1 || {
        echo "[!] $PREFIX is not writable and sudo is not installed" >&2
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
        echo "[!] --from-release requires curl or wget" >&2
        return 1
    fi
}

json_escape() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

# ------------------------------------------------------------
# 卸载：优先按安装清单精确删除
# ------------------------------------------------------------
if [ "$UNINSTALL" -eq 1 ]; then
    echo "==> uninstalling (PREFIX=$PREFIX)"
    removed=0
    if [ -f "$MANIFEST_FILES" ]; then
        echo "    using manifest: $MANIFEST_FILES"
        while IFS= read -r f; do
            [ -n "$f" ] || continue
            case "$f" in
                "$PREFIX"/*) ;;   # 只允许删除前缀内的路径
                *) echo "[!] skipping path outside prefix: $f" >&2; continue ;;
            esac
            if [ -e "$f" ] || [ -L "$f" ]; then
                priv rm -f "$f"
                echo "  removed $f"
                removed=1
            fi
        done < "$MANIFEST_FILES"
        priv rm -f "$MANIFEST" "$MANIFEST_FILES"
    else
        echo "    (no install manifest found; using default paths)"
    fi
    # 无论有无清单都清理已知路径，并删除空的 lib 目录
    priv rm -f "$BINDIR/gocryptfs-cli" "$BINDIR/gocryptfs-tui"
    priv rm -rf "$LIBDIR"
    # 只在确认为空时才删父目录（非空会失败，不影响其它文件）
    priv rmdir "$PREFIX/lib" >/dev/null 2>&1 || true
    priv rmdir "$BINDIR" >/dev/null 2>&1 || true
    echo "[OK] uninstalled:"
    echo "  $BINDIR/gocryptfs-tui"
    echo "  $BINDIR/gocryptfs-cli"
    echo "  $LIBDIR"
    echo ""
    echo "[i] config and data directories were kept (remove them manually if needed):"
    echo "  ${XDG_CONFIG_HOME:-$HOME/.config}/gocryptfs-tui/"
    echo "  ${XDG_DATA_HOME:-$HOME/.local/share}/gocryptfs-tui/"
    remaining="$(command -v gocryptfs-cli 2>/dev/null || true)"
    if [ -n "$remaining" ]; then
        echo ""
        echo "[!] PATH still resolves to: $remaining"
        echo "    if that is another/older install, uninstall it too to avoid a stale backend."
    fi
    exit 0
fi

# ------------------------------------------------------------
# 安装
# ------------------------------------------------------------
# ---- 运行时依赖（仅警告；真正的判定交给 TUI 的 --check-deps）----
echo "==> checking dependencies"
missing_deps=""
for cmd in gocryptfs fusermount rsync yq jq mountpoint; do
    have "$cmd" || missing_deps="$missing_deps $cmd"
done
if [ -n "$missing_deps" ]; then
    echo "[!] missing runtime dependencies:${missing_deps}"
    echo "    continuing; install the missing ones as suggested by 'gocryptfs-tui --check-deps'."
else
    echo "[OK] dependencies present"
fi

# ---- 模式自动判断 ----
# curl|sh 时没有源码树（$0 也不是可读文件），必须走 Release；否则在源码目录里就本地编译。
IN_REPO=0
if [ -f Cargo.toml ] && [ -f src/main.rs ] && [ -f "$0" ] && [ -r "$0" ]; then
    IN_REPO=1
fi
if [ -z "$MODE" ]; then
    if [ "$IN_REPO" -eq 1 ] && { [ -f target/release/gocryptfs-tui ] || have cargo; }; then
        MODE="local"
    else
        MODE="release"
        if [ "$IN_REPO" -eq 1 ]; then
            echo "[i] no cargo and no target/release found; using the Release binary (use --local to force a local build)"
        fi
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
        *) echo "[!] unsupported architecture: $(uname -m)" >&2; exit 1 ;;
    esac
    if have ldd && ldd --version 2>&1 | grep -qi musl; then libc="musl"; else libc="gnu"; fi
    target="${arch}-unknown-linux-${libc}"
    asset="gocryptfs-tui-${target}.tar.xz"

    # 未指定版本：在 git checkout 里优先用当前 tag（保证与该份源码对应），否则用 latest
    if [ -z "$RELEASE_VER" ] && have git && git rev-parse --git-dir >/dev/null 2>&1; then
        RELEASE_VER="$(git describe --tags --abbrev=0 2>/dev/null || true)"
    fi
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

    echo "==> source: Release ($ver_label, $target)"
    if ! download "$base/$asset" "$DL/$asset"; then
        echo "[!] download failed: $base/$asset" >&2
        echo "    download manually: https://github.com/$REPO/releases" >&2
        exit 1
    fi
    if download "$base/$asset.sha256" "$DL/$asset.sha256" 2>/dev/null; then
        ( cd "$DL" && sha256sum -c "$asset.sha256" >/dev/null ) \
            || { echo "[!] sha256 verification failed: $asset" >&2; exit 1; }
        echo "[OK] sha256 verified"
    else
        echo "[!] no .sha256 available; skipping verification" >&2
    fi
    tar -xJf "$DL/$asset" -C "$DL"
    BIN_SRC="$DL/${asset%.tar.xz}/gocryptfs-tui"
    [ -f "$BIN_SRC" ] || { echo "[!] archive does not contain gocryptfs-tui" >&2; exit 1; }

    # 独立后端：从同一 Release 的源码包取 shell/
    echo "==> fetching the shell backend (source.tar.gz)"
    if download "$base/source.tar.gz" "$DL/source.tar.gz"; then
        if download "$base/source.tar.gz.sha256" "$DL/source.tar.gz.sha256" 2>/dev/null; then
            ( cd "$DL" && sha256sum -c "source.tar.gz.sha256" >/dev/null ) \
                || { echo "[!] source.tar.gz verification failed" >&2; exit 1; }
        fi
        tar -xzf "$DL/source.tar.gz" -C "$DL"
        SHELL_DIR="$(find "$DL" -maxdepth 2 -type d -name shell | head -1)"
    fi
    if [ -z "$SHELL_DIR" ] || [ ! -f "$SHELL_DIR/gocryptfs-cli" ]; then
        echo "[!] could not extract shell/ from the source archive; installing the TUI only (no standalone CLI)" >&2
        SHELL_DIR=""
    fi
    SOURCE_DESC="release:$ver_label"
else
    if [ "$DO_BUILD" -eq 1 ]; then
        if ! have cargo; then
            echo "[!] local mode needs the Rust toolchain, but cargo was not found." >&2
            echo "    without source/toolchain, install from the Release:" >&2
            echo "      curl -LsSf https://github.com/$REPO/releases/latest/download/gocryptfs-tui-install.sh \\" >&2
            echo "        | sh -s -- --from-release --system" >&2
            exit 1
        fi
        echo "==> building gocryptfs-tui (local source mode)"
        cargo build --release
    else
        echo "==> skipping build (--no-build)"
    fi
    BIN_SRC="target/release/gocryptfs-tui"
    SHELL_DIR="shell"
    SOURCE_DESC="local:$BIN_SRC"
    echo "==> source: local build ($BIN_SRC)"
    [ -f "$BIN_SRC" ] || { echo "[!] $BIN_SRC not found (run make build first, or drop --no-build)" >&2; exit 1; }
    if [ ! -f "$SHELL_DIR/gocryptfs-cli" ] || [ ! -f "$SHELL_DIR/lib/gocryptfs-lib.sh" ]; then
        echo "[!] shell/ or shell/lib/ is missing (gocryptfs-lib.sh and i18n.sh are required)" >&2
        exit 1
    fi
fi

BIN_VERSION="$("$BIN_SRC" --version 2>/dev/null | sed -n 's/^gocryptfs-tui //p' || true)"
[ -n "$BIN_VERSION" ] || BIN_VERSION="$SOURCE_DESC"

# ---- 清理旧安装并安装 ----
echo "==> installing to $PREFIX"
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
    echo "[!] nothing to link: no standalone backend (shell/ was not fetched)"
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
echo "==> self-check"
if [ -n "$SHELL_DIR" ]; then
    if "$LIBDIR/gocryptfs-cli" --help 2>&1 | grep -q -- '--lang'; then
        echo "[OK] shell backend supports --lang (i18n enabled)"
    else
        echo "[!] shell backend does not provide --lang; make sure shell/lib/ is up to date"
    fi
fi
if [ "$LINK_CLI" = "1" ] && [ -n "$SHELL_DIR" ]; then
    resolved="$(command -v gocryptfs-cli 2>/dev/null || true)"
    if [ -z "$resolved" ]; then
        echo "[!] gocryptfs-cli is not on PATH; add $BINDIR to PATH"
    elif [ "$resolved" != "$BINDIR/gocryptfs-cli" ]; then
        echo "[!] PATH resolves to: $resolved"
        echo "    which is not this install's $BINDIR/gocryptfs-cli; a stale backend may be used"
    else
        echo "[OK] PATH resolves to this install's gocryptfs-cli"
    fi
elif [ -n "$SHELL_DIR" ]; then
    echo "[i] gocryptfs-cli is not linked into PATH (default). Canonical location:"
    echo "      $LIBDIR/gocryptfs-cli"
    echo "    to use the CLI directly, run:"
    echo "      ln -sf $LIBDIR/gocryptfs-cli $BINDIR/gocryptfs-cli"
    echo "      (the TUI uses its embedded backend; this step is optional)"
fi

if [ "$DEPS_CHECK" -eq 1 ]; then
    echo ""
    echo "==> runtime dependency check"
    if "$BINDIR/gocryptfs-tui" --check-deps; then
        :
    else
        rc=$?
        if [ "$rc" -eq 1 ]; then
            echo ""
            echo "[!] missing runtime dependencies; install them as shown above before running gocryptfs-tui"
        elif [ -n "$SHELL_DIR" ]; then
            # 旧版二进制不认识 --check-deps：退回 Shell 后端的检查
            echo "[i] this TUI binary does not support --check-deps; falling back to the shell backend"
            "$LIBDIR/gocryptfs-cli" --check-deps \
                || echo "[!] missing runtime dependencies; install them as shown above"
        fi
    fi
fi

echo ""
echo "[OK] installed (source: $SOURCE_DESC, version: $BIN_VERSION)"
echo "  TUI:  $BINDIR/gocryptfs-tui"
if [ "$LINK_CLI" = "1" ] && [ -n "$SHELL_DIR" ]; then
    echo "  CLI:  $BINDIR/gocryptfs-cli -> $LIBDIR/gocryptfs-cli"
elif [ -n "$SHELL_DIR" ]; then
    echo "  CLI:  $LIBDIR/gocryptfs-cli (not linked into PATH; add --link-cli to link)"
else
    echo "  CLI:  (embedded backend only; extracted to the data dir on first run)"
fi
echo "  manifest: $MANIFEST"
echo ""
echo "==> installed files"
find "$LIBDIR" -type f | sort
echo ""
echo "Run:  gocryptfs-tui"
if [ -f "$0" ] && [ -r "$0" ]; then
    echo "Uninstall: $0 --uninstall --prefix $PREFIX"
else
    echo "Uninstall: curl -LsSf https://github.com/$REPO/releases/latest/download/gocryptfs-tui-install.sh \\"
    echo "             | sh -s -- --uninstall --prefix $PREFIX"
fi
