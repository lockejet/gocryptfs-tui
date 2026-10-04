#!/bin/bash
# test-i18n.sh: i18n 验收测试（TUI + Shell 后端）
#
# 位置: <项目根>/test/test-i18n.sh
# 用法: bash test/test-i18n.sh
#
# 覆盖：语言解析矩阵（--lang / 环境变量 / 配置 / 系统 locale）、报错与帮助文案。
# 只依赖编译好的二进制与 shell/gocryptfs-cli，不需要 FUSE / gocryptfs / root。

set -uo pipefail

SCRIPT_DIR="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd -P "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

BIN="$PROJECT_ROOT/target/debug/gocryptfs-tui"
[ -x "$BIN" ] || BIN="$PROJECT_ROOT/target/release/gocryptfs-tui"
if [ ! -x "$BIN" ]; then
    echo "错误: 未找到二进制，请先执行 make build-debug" >&2
    exit 1
fi

TMP="$(mktemp -d /tmp/gocryptfs-tui-i18n.XXXXXX)"
trap 'rm -rf "$TMP"' EXIT
CONFIG_ZH="$TMP/config-zh.yaml"
CONFIG_EN="$TMP/config-en.yaml"
printf 'language: zh-CN\n' > "$CONFIG_ZH"
printf 'language: en-US\n' > "$CONFIG_EN"

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

# 取 --help 中「-c, --config」这一行的描述部分，用于判断当前语言
help_lang() {
    "$@" --help 2>/dev/null | grep -F -- '-c, --config' | head -1
}

check_exit() {
    local desc="$1" expected="$2" actual="$3" output="$4"
    if [ "$expected" = "$actual" ]; then
        echo "  ✅ $desc"
        PASS=$((PASS+1))
    else
        echo "  ❌ $desc"
        echo "     期望退出码: $expected 实际: $actual"
        echo "     输出: $output"
        FAIL=$((FAIL+1))
    fi
}

echo "===== i18n CLI 验收测试 ====="
echo "  二进制: $BIN"
echo ""

echo "==> 1. 语言解析优先级"

# 1.1 默认（LANG=C，无其他设置）→ zh-CN
out="$(help_lang env -u GOCRYPTFS_TUI_LANG LANG=C LC_ALL= "$BIN" -c "$TMP/none.yaml")"
check "LANG=C → 中文" "    -c, --config <PATH>    配置文件路径" "$out"

# 1.2 系统 locale → en-US
out="$(help_lang env LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 "$BIN" -c "$TMP/none.yaml")"
check "LANG=en_US.UTF-8 → English" "    -c, --config <PATH>    config file path" "$out"

# 1.3 系统 locale → zh-CN
out="$(help_lang env LANG=zh_CN.UTF-8 LC_ALL=zh_CN.UTF-8 "$BIN" -c "$TMP/none.yaml")"
check "LANG=zh_CN.UTF-8 → 中文" "    -c, --config <PATH>    配置文件路径" "$out"

# 1.4 LANGUAGE 偏好列表优先于 LANG
out="$(help_lang env LANGUAGE=en_US:zh_CN LANG=zh_CN.UTF-8 LC_ALL= "$BIN" -c "$TMP/none.yaml")"
check "LANGUAGE=en_US:zh_CN 优先" "    -c, --config <PATH>    config file path" "$out"

# 1.5 LC_ALL=C 表示不本地化，压过 LANG（POSIX）
out="$(help_lang env LC_ALL=C LANG=en_US.UTF-8 "$BIN" -c "$TMP/none.yaml")"
check "LC_ALL=C 压过 LANG → 默认中文" "    -c, --config <PATH>    配置文件路径" "$out"

# 1.6 LC_ALL=C 时忽略 LANGUAGE（GNU）
out="$(help_lang env -u LANG LC_ALL=C LANGUAGE=en_US "$BIN" -c "$TMP/none.yaml")"
check "LC_ALL=C 时忽略 LANGUAGE" "    -c, --config <PATH>    配置文件路径" "$out"

# 1.7 LC_MESSAGES 优先于 LANG
out="$(help_lang env -u LC_ALL LC_MESSAGES=en_US.UTF-8 LANG=zh_CN.UTF-8 "$BIN" -c "$TMP/none.yaml")"
check "LC_MESSAGES 优先于 LANG" "    -c, --config <PATH>    config file path" "$out"

# 1.8 配置文件 language: 覆盖系统 locale
out="$(help_lang env LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 "$BIN" -c "$CONFIG_ZH")"
check "配置 language: zh-CN 覆盖 locale" "    -c, --config <PATH>    配置文件路径" "$out"

# 1.9 GOCRYPTFS_TUI_LANG 覆盖配置文件
out="$(help_lang env GOCRYPTFS_TUI_LANG=en-US LANG=zh_CN.UTF-8 "$BIN" -c "$CONFIG_ZH")"
check "GOCRYPTFS_TUI_LANG 覆盖配置" "    -c, --config <PATH>    config file path" "$out"

# 1.10 --lang 优先级最高
out="$(help_lang env GOCRYPTFS_TUI_LANG=zh-CN LANG=en_US.UTF-8 "$BIN" -c "$CONFIG_EN" --lang zh-CN)"
check "--lang 覆盖一切" "    -c, --config <PATH>    配置文件路径" "$out"

echo ""
echo "==> 2. 错误信息本地化"

out="$(env LANG=zh_CN.UTF-8 "$BIN" -c "$TMP/none.yaml" --bogus 2>&1)"; code=$?
check_exit "未知参数退出码 2" "2" "$code" "$out"
check "未知参数中文报错" "错误: 未知参数: --bogus" "$(printf '%s' "$out" | head -1)"

out="$(env LANG=en_US.UTF-8 "$BIN" -c "$TMP/none.yaml" --bogus 2>&1)"
check "未知参数英文报错" "Error: Unknown argument: --bogus" "$(printf '%s' "$out" | head -1)"

out="$("$BIN" -c "$TMP/none.yaml" --lang fr-FR 2>&1)"; code=$?
check_exit "不支持语言退出码 2" "2" "$code" "$out"
if printf '%s' "$out" | grep -q -- '--lang\|Unsupported language.*fr-FR'; then
    echo "  ✅ 不支持语言给出明确提示"
    PASS=$((PASS+1))
else
    echo "  ❌ 不支持语言缺少提示: $out"
    FAIL=$((FAIL+1))
fi

echo ""
echo "==> 3. 其他"

out="$("$BIN" --lang en-US --version)"
if printf '%s' "$out" | grep -q '^gocryptfs-tui '; then
    echo "  ✅ 版本输出可用"
    PASS=$((PASS+1))
else
    echo "  ❌ 版本输出异常: $out"
    FAIL=$((FAIL+1))
fi

echo ""
echo "==> 4. Shell 后端（gocryptfs-cli）"

SH="$PROJECT_ROOT/shell/gocryptfs-cli"
SH_LIB="$PROJECT_ROOT/shell/lib/gocryptfs-lib.sh"
CONFIG_SH_EN="$TMP/shell-en.yaml"
printf 'language: en-US\n' > "$CONFIG_SH_EN"

# 4.1 帮助文案
out="$(env LC_ALL=C bash "$SH" help | head -1)"
check "shell 默认中文帮助" "gocryptfs-cli - gocryptfs-tui 的 Shell 后端" "$out"

out="$(env LC_ALL=C bash "$SH" --lang en-US help | head -1)"
check "shell --lang en-US 帮助" "gocryptfs-cli - shell backend of gocryptfs-tui" "$out"

# 4.2 系统 locale / 环境变量 / 配置
out="$(env LANG=en_US.UTF-8 LC_ALL= bash "$SH" -c "$TMP/none.yaml" list 2>&1 | head -1)"
check "shell LANG=en_US.UTF-8 → English" "[✗] Config file not found: $TMP/none.yaml" "$out"

out="$(env GOCRYPTFS_LANG=zh-CN LANG=en_US.UTF-8 LC_ALL= bash "$SH" -c "$TMP/none.yaml" list 2>&1 | head -1)"
check "shell GOCRYPTFS_LANG 覆盖 locale" "[✗] 配置文件不存在: $TMP/none.yaml" "$out"

out="$(env LANG=zh_CN.UTF-8 LC_ALL= bash "$SH" -c "$CONFIG_SH_EN" list 2>&1 | head -1)"
check "shell 配置 language: en-US 生效" "(no vaults in config)" "$out"

out="$(env LC_ALL=C bash "$SH" --check-deps 2>&1 | head -1)"
check "shell check-deps 中文" "[✔] 依赖齐全" "$out"

out="$(env LANG=en_US.UTF-8 LC_ALL= bash "$SH" --check-deps 2>&1 | head -1)"
check "shell check-deps English" "[✔] All dependencies present" "$out"

# 4.3 报错文案
out="$(env LC_ALL=C bash "$SH" --lang fr-FR help 2>&1 | head -1)"
check "shell 不支持语言" "不支持的语言: fr-FR（可用: zh-CN, en-US）" "$out"

out="$(env LC_ALL=C bash "$SH" --lang= help 2>&1 | head -1)"
check "shell --lang= 空值" "错误: -l/--lang 需要语言代码" "$out"

out="$(env LC_ALL=C bash "$SH" -c 2>&1 | head -1)"
check "shell -c 缺值" "错误: -c/--config 需要参数" "$out"

out="$(env LC_ALL=C bash "$SH" bogus 2>&1 | head -1)"
check "shell 未知命令" "未知命令: bogus" "$out"

out="$(env LANG=en_US.UTF-8 LC_ALL= bash "$SH" bogus 2>&1 | head -1)"
check "shell 未知命令 English" "Unknown command: bogus" "$out"

# 4.4 lib 自检信息
out="$(env LC_ALL=C bash "$SH_LIB" | head -1)"
check "shell lib 自检中文" "gocryptfs-lib.sh — 函数索引（轮询版）" "$out"

out="$(env LANG=en_US.UTF-8 LC_ALL= bash "$SH_LIB" | head -1)"
check "shell lib 自检 English" "gocryptfs-lib.sh — function index (polling build)" "$out"

# 4.5 log 子命令：历史日志里可能混有旧版本写入的纯文本行，必须跳过而不是报错
LOG_MIXED="$TMP/mixed.log.jsonl"
{
    printf '[2026-01-01T00:00:00+08:00] 已解锁 (755): /x\n'
    printf '{"ts":"2026-01-01T00:00:01+08:00","src":"cli","action":"mount","target":"v","result":"success","detail":""}\n'
} > "$LOG_MIXED"

out="$(env LC_ALL=C LOG_FILE="$LOG_MIXED" bash "$SH" log --json 2>&1)"; code=$?
check_exit "shell log 容忍非 JSON 行" "0" "$code" "$out"
check "shell log 只输出 JSON 行" "1" "$(printf '%s\n' "$out" | grep -c .)"
check "shell log 内容正确" "mount" "$(printf '%s' "$out" | jq -r '.action')"

echo ""
# 15. --check-deps：依赖齐全退出 0；缺依赖退出 1 并给出安装命令
out="$("$BIN" --check-deps 2>&1)"; rc=$?
if printf '%s' "$out" | grep -qE '依赖齐全|dependencies present'; then
    check "--check-deps 依赖齐全时退出 0" "0" "$rc"
else
    check "--check-deps 缺依赖时退出 1" "1" "$rc"
    printf '%s' "$out" | grep -q 'apt install' && has_hint=1 || has_hint=0
    check "--check-deps 给出安装命令" "1" "$has_hint"
fi

echo "===== 结果: $PASS 通过, $FAIL 失败 ====="
[ "$FAIL" -eq 0 ] || exit 1
