#!/bin/bash
# test-batch2.sh: 第二批功能测试（mount/umount/create/remove）
# 位置: <项目根>/test/test-batch2.sh
# 用法: bash test/test-batch2.sh
#
# 特性：
#   - 开头和结尾都有清理逻辑（防止挂载残留）
#   - 用 mount | grep 全局扫描所有挂载点
#   - 失败时 fallback 到 lazy unmount 和 sudo

set -uo pipefail

# 固定界面语言为默认（zh-CN），避免受运行环境 locale 影响
LC_ALL=C; export LC_ALL

SCRIPT_DIR="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd -P "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

CLI="$PROJECT_ROOT/shell/gocryptfs-cli"
TEST_ROOT="/tmp/gocryptfs-tui-test-b2"
CONFIG="$TEST_ROOT/config.yaml"
PASS_TEST="test-password-12345"

PASS=0
FAIL=0

check() {
    local desc="$1" expected="$2" actual="$3"
    if [ "$expected" = "$actual" ]; then
        echo "  ✅ $desc"; PASS=$((PASS+1))
    else
        echo "  ❌ $desc"
        echo "     期望: $expected"
        echo "     实际: $actual"
        FAIL=$((FAIL+1))
    fi
}

# ---------------- 清理函数 ----------------
cleanup_test_root() {
    # 1. 全局扫描所有相关挂载（不只是 plain/）
    local mounts
    mounts=$(mount 2>/dev/null | grep " $TEST_ROOT" | awk '{print $3}' | sort -r)

    if [ -n "$mounts" ]; then
        echo "    卸载挂载点："
        echo "$mounts" | while read -r mp; do
            [ -z "$mp" ] && continue
            echo "      $mp"
            if ! fusermount -u "$mp" 2>/dev/null; then
                fusermount -u -z "$mp" 2>/dev/null || \
                    sudo fusermount -u -z "$mp" 2>/dev/null || \
                    echo "      ⚠️  无法卸载: $mp"
            fi
        done
        sleep 0.5
    fi

    # 2. 二次检查残留
    if mount 2>/dev/null | grep -q " $TEST_ROOT"; then
        echo "    ⚠️  仍有挂载残留，尝试 fuser"
        sudo fuser -km "$TEST_ROOT" 2>/dev/null || true
        sleep 1
    fi

    # 3. 删除目录
    if [ -d "$TEST_ROOT" ]; then
        if ! rm -rf "$TEST_ROOT" 2>/dev/null; then
            sudo fuser -km "$TEST_ROOT" 2>/dev/null || true
            sleep 0.5
            sudo rm -rf "$TEST_ROOT" 2>/dev/null || true
        fi
    fi
}

# ---------------- 前置清理 ----------------
echo "===== 第二批功能测试 ====="
echo "  项目根: $PROJECT_ROOT"
echo "  测试根: $TEST_ROOT"
echo ""
echo "==> 清理旧测试环境"
cleanup_test_root

# ---------------- 初始化测试环境 ----------------
echo "==> 准备测试环境"
mkdir -p "$TEST_ROOT"/{cipher,plain,src}

# 准备明文章
mkdir -p "$TEST_ROOT/src/photos/2024"
echo "photo 1" > "$TEST_ROOT/src/photos/img1.jpg"
echo "photo 2" > "$TEST_ROOT/src/photos/img2.jpg"
head -c 100000 /dev/urandom > "$TEST_ROOT/src/photos/2024/big.bin"
mkdir -p "$TEST_ROOT/src/photos/.git"
echo "git" > "$TEST_ROOT/src/photos/.git/config"

# 初始配置
cat > "$CONFIG" << EOF
settings:
  unlock_mode: "755"
  lock_mode: "555"
  gocryptfs:
    allow_other: false
  rsync:
    compress: false
    partial: true
  create:
    keep_source: false
    tmp_mount_suffix: ".mount_tmp"
  remove:
    restore: true
    direct_delete_cipher: false
    restore_target_mode: in_place
vaults: []
EOF

echo ""

# ---------------- 测试 1-6: create ----------------
echo "==> 测试 create"

# 1. dry-run
out=$(printf '%s\n' "$PASS_TEST" | bash "$CLI" -c "$CONFIG" create "$TEST_ROOT/src/photos" --name photos --cipher "$TEST_ROOT/cipher/photos" --dry-run 2>&1 >/dev/null | grep -c 'DRY-RUN')
check "create dry-run 输出 DRY-RUN 行" "1" "$([ "$out" -ge 1 ] && echo 1 || echo 0)"

# 2. 真实创建
if printf '%s\n' "$PASS_TEST" | bash "$CLI" -c "$CONFIG" create "$TEST_ROOT/src/photos" --name photos --cipher "$TEST_ROOT/cipher/photos" --yes >/dev/null 2>&1; then
    check "create 成功" "0" "0"
else
    check "create 成功" "0" "1"
fi

# 3. 加密后端存在
check "加密后端目录存在" "1" "$([ -d "$TEST_ROOT/cipher/photos" ] && echo 1 || echo 0)"

# 4. 创建后自动挂载
out=$(bash "$CLI" -c "$CONFIG" list --json 2>/dev/null | jq -r '.data.vaults[0].mounted')
check "创建后自动挂载" "true" "$out"

# 5. 配置中已追加
out=$(bash "$CLI" -c "$CONFIG" list --json 2>/dev/null | jq -r '.data.vaults | length')
check "配置中卷数" "1" "$out"

# 6. 明文可读
out=$(ls "$TEST_ROOT/src/photos/" 2>/dev/null | wc -l | tr -d ' ')
check "明文可读" "3" "$out"

# ---------------- 测试 7-12: mount / umount ----------------
echo "==> 测试 mount / umount"

# 7. 卸载
if bash "$CLI" -c "$CONFIG" umount photos >/dev/null 2>&1; then
    check "umount 成功" "0" "0"
else
    check "umount 成功" "0" "1"
fi

# 8. 卸载后锁定
out=$(stat -c '%a' "$TEST_ROOT/src/photos" 2>/dev/null | tail -c4)
check "卸载后权限 555" "555" "$out"

# 9. 卸载后状态
out=$(bash "$CLI" -c "$CONFIG" list --json 2>/dev/null | jq -r '.data.vaults[0].mounted')
check "卸载后状态" "false" "$out"

# 10. 重新挂载
if printf '%s\n' "$PASS_TEST" | bash "$CLI" -c "$CONFIG" mount photos >/dev/null 2>&1; then
    check "mount 成功" "0" "0"
else
    check "mount 成功" "0" "1"
fi

# 11. 挂载后明文可见
out=$(ls "$TEST_ROOT/src/photos/" 2>/dev/null | wc -l | tr -d ' ')
check "挂载后明文可读" "3" "$out"

# 12. 重复挂载报错
bash "$CLI" -c "$CONFIG" mount photos </dev/null >/dev/null 2>&1
rc=$?
check "重复挂载退出码非 0" "1" "$([ $rc -ne 0 ] && echo 1 || echo 0)"

# ---------------- 测试 13-17: remove ----------------
echo "==> 测试 remove"

# 13. dry-run
out=$(printf '%s\n' "$PASS_TEST" | bash "$CLI" -c "$CONFIG" remove photos --dry-run 2>&1 >/dev/null | grep -c 'DRY-RUN')
check "remove dry-run" "1" "$([ "$out" -ge 1 ] && echo 1 || echo 0)"

# 14. 真实删除
if printf '%s\n' "$PASS_TEST" | bash "$CLI" -c "$CONFIG" remove photos --yes >/dev/null 2>&1; then
    check "remove 成功" "0" "0"
else
    check "remove 成功" "0" "1"
fi

# 15. 配置中移除
out=$(bash "$CLI" -c "$CONFIG" list --json 2>/dev/null | jq -r '.data.vaults | length')
check "配置中卷数为 0" "0" "$out"

# 16. 明文已还原
check "明文已还原" "1" "$([ -d "$TEST_ROOT/src/photos" ] && echo 1 || echo 0)"

# 17. 加密后端保留
check "加密后端保留" "1" "$([ -d "$TEST_ROOT/cipher/photos" ] && echo 1 || echo 0)"

# ---------------- 结果 ----------------
echo ""
echo "===== 结果 ====="
echo "PASS: $PASS"
echo "FAIL: $FAIL"

# ---------------- 后置清理 ----------------
echo ""
echo "==> 清理测试环境"
cleanup_test_root

# 最终检查
if mount 2>/dev/null | grep -q " $TEST_ROOT"; then
    echo "    ⚠️  仍有挂载残留"
    mount | grep " $TEST_ROOT"
fi

echo ""

if [ $FAIL -eq 0 ]; then
    echo "🎉 全部通过"
    exit 0
else
    echo "⚠️  有失败项"
    exit 1
fi