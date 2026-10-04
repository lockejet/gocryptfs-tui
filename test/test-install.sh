#!/bin/bash
# test-install.sh — install.sh 的安装/卸载回归测试（本地源码模式，不联网）
#
# 位置: <项目根>/test/test-install.sh
# 用法: bash test/test-install.sh
#
# 覆盖：
#   * 用户级安装（默认不链接 CLI）与系统级语义（--system 时链接 CLI）
#   * 安装清单 INSTALLED.json / INSTALLED.files 的内容
#   * --uninstall 按清单精确删除，且**不动**前缀里的无关文件
#   * --from-release 的参数解析（不实际下载：用 --no-deps-check + 非法版本快速失败）
set -uo pipefail

SCRIPT_DIR="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd -P "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

INSTALL="$PROJECT_ROOT/install.sh"
BIN="$PROJECT_ROOT/target/release/gocryptfs-tui"

PASS=0
FAIL=0
check() {
    local desc="$1" expected="$2" actual="$3"
    if [ "$expected" = "$actual" ]; then
        echo "  ✅ $desc"
        PASS=$((PASS+1))
    else
        echo "  ❌ $desc"
        echo "     期望: $expected"
        echo "     实际: $actual"
        FAIL=$((FAIL+1))
    fi
}
exists() { [ -e "$1" ] && echo yes || echo no; }

if [ ! -x "$BIN" ]; then
    echo "错误: 未找到 $BIN，请先 make build" >&2
    exit 1
fi

echo "===== install.sh 回归测试 ====="
TMP="$(mktemp -d /tmp/gocryptfs-tui-install-test.XXXXXX)"
trap 'rm -rf "$TMP"' EXIT

# ---- 1. 用户级安装（默认不链接 CLI）----
P1="$TMP/user"
sh "$INSTALL" --prefix "$P1" --no-build --no-deps-check >/dev/null 2>&1
check "用户级安装: TUI 可执行" "yes" "$(exists "$P1/bin/gocryptfs-tui")"
check "用户级安装: 独立后端存在" "yes" "$(exists "$P1/lib/gocryptfs-tui/gocryptfs-cli")"
check "用户级安装: VERSION 存在" "yes" "$(exists "$P1/lib/gocryptfs-tui/VERSION")"
check "用户级安装: 默认不链接 CLI" "no" "$(exists "$P1/bin/gocryptfs-cli")"
check "安装清单存在" "yes" "$(exists "$P1/lib/gocryptfs-tui/INSTALLED.json")"
check "文件清单存在" "yes" "$(exists "$P1/lib/gocryptfs-tui/INSTALLED.files")"
check "清单含 TUI 路径" "yes" \
    "$(grep -qx "$P1/bin/gocryptfs-tui" "$P1/lib/gocryptfs-tui/INSTALLED.files" && echo yes || echo no)"
check "清单 source 为 local" "yes" \
    "$(grep -q '"source": "local:' "$P1/lib/gocryptfs-tui/INSTALLED.json" && echo yes || echo no)"

# ---- 2. 系统级语义：--system 会链接 CLI（用 --prefix 覆盖落点以便在沙箱测试）----
P2="$TMP/system"
sh "$INSTALL" --system --prefix "$P2" --no-build --no-deps-check >/dev/null 2>&1
check "--system: 链接 gocryptfs-cli" "yes" "$(exists "$P2/bin/gocryptfs-cli")"
check "--system: 链接指向本次安装" "yes" \
    "$([ "$(readlink "$P2/bin/gocryptfs-cli")" = "$P2/lib/gocryptfs-tui/gocryptfs-cli" ] && echo yes || echo no)"
check "--system: 清单收录链接" "yes" \
    "$(grep -qx "$P2/bin/gocryptfs-cli" "$P2/lib/gocryptfs-tui/INSTALLED.files" && echo yes || echo no)"

# --no-link-cli 能覆盖 --system
P3="$TMP/nolink"
sh "$INSTALL" --system --prefix "$P3" --no-link-cli --no-build --no-deps-check >/dev/null 2>&1
check "--no-link-cli 覆盖 --system" "no" "$(exists "$P3/bin/gocryptfs-cli")"

# ---- 3. 卸载：按清单删除，不动无关文件 ----
echo keep > "$P1/bin/unrelated-tool"
echo keep > "$P1/lib/unrelated-file"
sh "$INSTALL" --uninstall --prefix "$P1" >/dev/null 2>&1
check "卸载: TUI 已删除" "no" "$(exists "$P1/bin/gocryptfs-tui")"
check "卸载: 后端目录已删除" "no" "$(exists "$P1/lib/gocryptfs-tui")"
check "卸载: 无关文件保留 (bin)" "yes" "$(exists "$P1/bin/unrelated-tool")"
check "卸载: 无关文件保留 (lib)" "yes" "$(exists "$P1/lib/unrelated-file")"

# 有清单时也不应误删前缀之外的路径
OUTSIDE="$TMP/outside.txt"
echo keep > "$OUTSIDE"
printf '%s\n' "$OUTSIDE" >> "$P2/lib/gocryptfs-tui/INSTALLED.files"
sh "$INSTALL" --uninstall --prefix "$P2" >/dev/null 2>&1
check "卸载: 拒绝删除前缀外路径" "yes" "$(exists "$OUTSIDE")"

# ---- 4. POSIX sh 兼容性（dash 是 Debian/Ubuntu 的 /bin/sh，用户习惯 curl | sh）----
if command -v dash >/dev/null 2>&1; then
    dash -n "$INSTALL" >/dev/null 2>&1 && v=0 || v=1
    check "install.sh 通过 dash 语法检查" "0" "$v"
fi
sh -n "$INSTALL" >/dev/null 2>&1 && v=0 || v=1
check "install.sh 通过 sh 语法检查" "0" "$v"

# 管道执行（模拟 curl ... | sh -s -- ...）
P4="$TMP/piped"
cat "$INSTALL" | sh -s -- --prefix "$P4" --no-build --no-deps-check >/dev/null 2>&1
check "管道执行: TUI 已安装" "yes" "$(exists "$P4/bin/gocryptfs-tui")"
check "管道执行: 清单已写" "yes" "$(exists "$P4/lib/gocryptfs-tui/INSTALLED.json")"
# 管道执行时不应把 $0（=sh）当成脚本路径来提示
out="$(cat "$INSTALL" | sh -s -- --prefix "$P4" --no-build --no-deps-check 2>&1 | tail -3)"
case "$out" in *"gocryptfs-tui-install.sh"*) v=1 ;; *) v=0 ;; esac
check "管道执行: 卸载提示给出 URL 形式" "1" "$v"

# ---- 5. 参数解析：--version 需要版本号；未知参数报错 ----
out="$(sh "$INSTALL" --version 2>&1 >/dev/null || true)"
case "$out" in *"需要版本号"*) v=1 ;; *) v=0 ;; esac
check "--version 缺版本号报错" "1" "$v"
out="$(sh "$INSTALL" --bogus 2>&1 || true)"
case "$out" in *"未知参数"*) v=1 ;; *) v=0 ;; esac
check "未知参数报错" "1" "$v"

echo ""
echo "===== 结果 ====="
echo "PASS: $PASS"
echo "FAIL: $FAIL"
[ "$FAIL" -eq 0 ] || exit 1
echo "🎉 全部通过"
