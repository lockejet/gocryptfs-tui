#!/bin/bash
# test-batch1.sh: 第一批功能自动测试（自包含环境准备）
#
# 位置: <项目根>/test/test-batch1.sh
# 用法: bash test/test-batch1.sh

set -uo pipefail

# 固定界面语言为默认（zh-CN），避免受运行环境 locale 影响
LC_ALL=C; export LC_ALL

SCRIPT_DIR="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd -P "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

CLI="$PROJECT_ROOT/shell/gocryptfs-cli"
LIB="$PROJECT_ROOT/shell/lib/gocryptfs-lib.sh"
CONFIG="/tmp/gocryptfs-tui-test/test-config.yaml"
EMPTY_CONFIG="/tmp/gocryptfs-tui-test-empty/config.yaml"

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

echo "===== 第一批功能测试 ====="
echo "  项目根: $PROJECT_ROOT"
echo "  配置:   $CONFIG"
echo ""

# ---------------- 环境准备（自包含，无需手工步骤）----------------
echo "==> 准备测试环境"

# 主测试环境：若无则生成
if [ ! -f "$CONFIG" ]; then
    echo "  生成主测试环境: /tmp/gocryptfs-tui-test/"
    bash "$SCRIPT_DIR/create-test-env.sh" > /dev/null 2>&1
    if [ ! -f "$CONFIG" ]; then
        echo "  ❌ 无法生成测试环境，中止"
        exit 1
    fi
fi

# 空配置环境：总是重新生成（干净的空配置）
mkdir -p "$(dirname "$EMPTY_CONFIG")"
cat > "$EMPTY_CONFIG" << 'EOF'
settings:
  lock_mode: "555"
  unlock_mode: "755"
vaults: []
EOF

echo "  主配置:   $CONFIG"
echo "  空配置:   $EMPTY_CONFIG"
echo ""

# ---------------- 测试开始 ----------------

# 1. lib 单文件运行
out=$(bash "$LIB" 2>&1 | head -1)
check "lib 单文件运行" "gocryptfs-lib.sh — 函数索引（轮询版）" "$out"

# 2. 依赖检查
out=$(bash "$CLI" check-deps 2>&1 | tail -1)
check "依赖检查" "[✔] 依赖齐全" "$out"

# 3. 表格 list（含数据）
out=$(bash "$CLI" -c "$CONFIG" list 2>/dev/null | head -1)
check "list 表格模式首行" "NAME                 STATUS       LOCKED     MOUNT" "$out"

# 4. JSON 位置无关性
out=$(bash "$CLI" -c "$CONFIG" list --json 2>/dev/null | jq -r .status)
check "JSON (--json 尾部)" "ok" "$out"

out=$(bash "$CLI" --json -c "$CONFIG" list 2>/dev/null | jq -r .status)
check "JSON (--json 头部)" "ok" "$out"

out=$(bash "$CLI" -c "$CONFIG" --json list 2>/dev/null | jq -r .status)
check "JSON (--json 中间)" "ok" "$out"

# 5. stdout 纯净性（首字节为 {）
out=$(bash "$CLI" -c "$CONFIG" list --json 2>/dev/null | head -c1)
check "stdout 首字节为 {" "{" "$out"

# 6. 协议行走 stderr
out=$(bash "$CLI" -c "$CONFIG" list --json 2>&1 >/dev/null | grep -c '@@DONE@@')
check "协议行走 stderr" "1" "$out"

# 7. info JSON
out=$(bash "$CLI" -c "$CONFIG" info test_plain --json 2>/dev/null | jq -r .data.name)
check "info test_plain" "test_plain" "$out"

# 8. 不存在的卷 → 退出码非 0
bash "$CLI" -c "$CONFIG" info nonexistent >/dev/null 2>&1
rc=$?
check "info 不存在卷 退出码非 0" "1" "$([ $rc -ne 0 ] && echo 1 || echo 0)"

# 9. 未挂载时 ls → 退出码 3
bash "$CLI" -c "$CONFIG" ls test_plain >/dev/null 2>&1
rc=$?
check "ls 未挂载 退出码=3" "3" "$rc"

# 10. 配置文件不存在 → 退出码 8
bash "$CLI" -c /tmp/nonexistent.yaml list >/dev/null 2>&1
rc=$?
check "配置不存在 退出码=8" "8" "$rc"

# 11. 空列表表格模式
out=$(bash "$CLI" -c "$EMPTY_CONFIG" list 2>/dev/null | head -1)
check "空列表提示" "(配置中没有任何卷)" "$out"

# 12. 空列表 JSON 模式
out=$(bash "$CLI" -c "$EMPTY_CONFIG" list --json 2>/dev/null | jq -r '.data.vaults | length')
check "空列表 JSON vaults 长度" "0" "$out"

# 13. 空列表 JSON status
out=$(bash "$CLI" -c "$EMPTY_CONFIG" list --json 2>/dev/null | jq -r .status)
check "空列表 JSON status" "ok" "$out"

# 14. list 表格模式含 vault 数量校验
out=$(bash "$CLI" -c "$CONFIG" list 2>/dev/null | tail -n +3 | wc -l | tr -d ' ')
check "list 表格模式卷数" "2" "$out"

echo ""
# 15. 缺 yq 时 list 必须报错（不能静默返回 0 个卷，否则 TUI 只看到空列表）
tmpbin="$(mktemp -d)"
for c in jq mountpoint dirname readlink bash; do
    p="$(command -v "$c" 2>/dev/null)" && ln -sf "$p" "$tmpbin/$c"
done
err="$(env -i PATH="$tmpbin" HOME="$HOME" LC_ALL=C bash "$CLI" -c "$CONFIG" list --json 2>&1 >/dev/null)"
rc=$?
loud=0
printf '%s' "$err" | grep -q '缺少依赖' && [ "$rc" -ne 0 ] && loud=1
check "缺 yq 时 list 报错且退出码非 0" "1" "$loud"
rm -rf "$tmpbin"

# 16. 缺卷名时 info/ls/tree/mount 应给用法，而不是 set -u 崩溃
bad=0
for c in info ls tree mount; do
    err="$(bash "$CLI" -c "$CONFIG" "$c" 2>&1 >/dev/null)"
    printf '%s' "$err" | grep -q "unbound variable" && bad=1
    printf '%s' "$err" | grep -q "用法:" || bad=1
done
check "缺卷名的 info/ls/tree/mount 给用法而非崩溃" "0" "$bad"

# 17. 选项缺值时报错明确
out="$(bash "$CLI" -c "$CONFIG" log --limit 2>&1 >/dev/null)"
case "$out" in *"需要一个值"*) v=1 ;; *) v=0 ;; esac
check "选项缺值报错明确（--limit）" "1" "$v"

# 18. CLI 与 TUI 的全局选项对齐：-V/--version
out="$(env LC_ALL=C bash "$CLI" -V 2>&1)"
case "$out" in gocryptfs-cli\ *) v=1 ;; *) v=0 ;; esac
check "-V 打印 gocryptfs-cli 版本行" "1" "$v"

# 19. -D/--data-dir 决定日志位置（与 TUI 的 -D 同义）
out="$(env LC_ALL=C bash "$CLI" -D /tmp/gocryptfs-tui-dtest log 2>&1 | head -1)"
case "$out" in *"/tmp/gocryptfs-tui-dtest/app.log.jsonl"*) v=1 ;; *) v=0 ;; esac
check "-D 决定日志路径" "1" "$v"

# 20. XDG_CONFIG_HOME 生效（无 -c 时）
out="$(env LC_ALL=C XDG_CONFIG_HOME=/tmp/gocryptfs-tui-xdg bash "$CLI" config 2>&1)"
check "XDG_CONFIG_HOME 生效" "/tmp/gocryptfs-tui-xdg/gocryptfs-tui/config.yaml" "$out"

# 21. 兼容旧变量 CONFIG_FILE
out="$(env LC_ALL=C CONFIG_FILE=/tmp/legacy-config.yaml bash "$CLI" config 2>&1)"
check "CONFIG_FILE 兼容" "/tmp/legacy-config.yaml" "$out"

# 22. -D 缺值报错明确
out="$(env LC_ALL=C bash "$CLI" -D 2>&1 >/dev/null)"
case "$out" in *"需要目录"*) v=1 ;; *) v=0 ;; esac
check "-D 缺值报错明确" "1" "$v"

echo "===== 结果 ====="
echo "PASS: $PASS"
echo "FAIL: $FAIL"
if [ $FAIL -eq 0 ]; then
    echo "🎉 全部通过"
    exit 0
else
    echo "⚠️  有失败项"
    exit 1
fi
