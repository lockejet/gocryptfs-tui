#!/bin/bash
# create-test-env.sh: 生成/清理 gocryptfs-tui 的测试环境
#
# 用法：
#   bash test/create-test-env.sh              # 默认：生成环境（含初始化，先清理）
#   bash test/create-test-env.sh --clean-first # 显式先清理后生成
#   bash test/create-test-env.sh --clean      # 只清理
#   bash test/create-test-env.sh --pass xxx   # 自定义密码
#   bash test/create-test-env.sh --no-cipher  # 只生成配置，不初始化加密卷
#   bash test/create-test-env.sh --keep       # 保留旧环境

set -euo pipefail

TEST_ROOT="${TEST_ROOT:-/tmp/gocryptfs-tui-test}"
CONFIG_FILE="$TEST_ROOT/test-config.yaml"
#DEFAULT_PASSWORD="test-password-12345"
DEFAULT_PASSWORD="test321"

# 获取当前实际运行用户（兼容 sudo）
REAL_USER="${SUDO_USER:-$USER}"
REAL_GROUP=$(id -gn "$REAL_USER" 2>/dev/null || echo "$REAL_USER")

# ---------------- 权限修复函数 ----------------
fix_permissions() {
    local path="$1"
    [ -e "$path" ] || return 0
    local owner
    owner=$(stat -c '%U' "$path" 2>/dev/null)
    if [ "$owner" != "$REAL_USER" ]; then
        if [ "$(id -u)" -eq 0 ]; then
            chown -R "$REAL_USER:$REAL_GROUP" "$path" 2>/dev/null || true
        else
            sudo chown -R "$REAL_USER:$REAL_GROUP" "$path" 2>/dev/null || true
        fi
    fi
}

# ---------------- 清理函数 ----------------
clean_env() {
    if [ ! -d "$TEST_ROOT" ] && ! mount 2>/dev/null | grep -q " $TEST_ROOT"; then
        echo "==> 测试环境不存在，无需清理: $TEST_ROOT"
        return 0
    fi

    echo "==> 清理测试环境: $TEST_ROOT"

    # 1. 全局扫描所有挂载点
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
    echo "    ✅ 已清理"
}

# ---------------- 参数 ----------------
PASSWORD="$DEFAULT_PASSWORD"
WITH_CIPHER=true
KEEP_EXISTING=false
CLEAN_ONLY=false
CLEAN_FIRST=false

while [ $# -gt 0 ]; do
    case "$1" in
        --clean)        CLEAN_ONLY=true; shift ;;
        --clean-first)  CLEAN_FIRST=true; shift ;;
        --pass)         PASSWORD="$2"; shift 2 ;;
        --no-cipher)    WITH_CIPHER=false; shift ;;
        --keep)         KEEP_EXISTING=true; shift ;;
        -h|--help)
            cat <<EOF
用法: $0 [OPTIONS]

选项:
  --clean             卸载所有挂载点并删除测试环境（不生成）
  --clean-first       生成前先清理（与默认行为相同）
  --pass <password>   指定密码（默认: $DEFAULT_PASSWORD）
  --no-cipher         只生成配置，不初始化加密卷
  --keep              保留已存在的测试环境（默认先清理）

默认密码: $DEFAULT_PASSWORD
EOF
            exit 0 ;;
        *) echo "未知选项: $1" >&2; exit 1 ;;
    esac
done

if [ "$CLEAN_ONLY" = true ]; then
    clean_env
    exit 0
fi

# ---------------- 依赖检查 ----------------
if [ "$WITH_CIPHER" = true ] && ! command -v gocryptfs >/dev/null 2>&1; then
    echo "[!] gocryptfs 未安装" >&2
    exit 1
fi

# ---------------- 清理旧环境 ----------------
if [ "$CLEAN_FIRST" = true ] || [ "$KEEP_EXISTING" = false ]; then
    if [ -d "$TEST_ROOT" ]; then
        clean_env
    fi
fi

# ---------------- 目录结构 ----------------
echo "==> 创建目录结构: $TEST_ROOT"
mkdir -p "$TEST_ROOT"/{cipher,plain,plain-src/{photos,documents}}

# 确保所有目录对当前用户可写
fix_permissions "$TEST_ROOT"
chmod -R u+rwX "$TEST_ROOT" 2>/dev/null || true

# ---------------- 生成测试数据 ----------------
echo "==> 生成测试数据"

mkdir -p "$TEST_ROOT/plain-src/photos/2024"
echo "Summer vacation photo #1" > "$TEST_ROOT/plain-src/photos/README.md"
echo "binary-data-$(date +%s)" > "$TEST_ROOT/plain-src/photos/img001.raw"
echo "binary-data-$(date +%s)" > "$TEST_ROOT/plain-src/photos/img002.raw"
for i in $(seq 1 10); do
    head -c 1024 /dev/urandom > "$TEST_ROOT/plain-src/photos/2024/file$i.bin"
done

echo "Important content" > "$TEST_ROOT/plain-src/documents/report.md"
echo "temp file" > "$TEST_ROOT/plain-src/documents/draft.tmp"
mkdir -p "$TEST_ROOT/plain-src/documents/.git"
echo "git metadata" > "$TEST_ROOT/plain-src/documents/.git/config"

fix_permissions "$TEST_ROOT"

# ---------------- 初始化加密卷 ----------------
VAULTS=(test_plain test_secret)

if [ "$WITH_CIPHER" = true ]; then
    echo "==> 初始化加密卷（密码: $PASSWORD）"
    passfile=$(mktemp -t gocryptfs-init.XXXXXX)
    chmod 600 "$passfile"
    printf '%s' "$PASSWORD" > "$passfile"

    for name in "${VAULTS[@]}"; do
        cipher_path="$TEST_ROOT/cipher/$name"
        mount_path="$TEST_ROOT/plain/$name"

        mkdir -p "$cipher_path" "$mount_path"

        # 确保挂载点属主和权限对当前用户可写
        fix_permissions "$mount_path"
        chmod 755 "$mount_path"

        # 加密后端也需要可写
        fix_permissions "$cipher_path"
        chmod 700 "$cipher_path"

        if [ -f "$cipher_path/gocryptfs.conf" ]; then
            echo "    已存在: $name（跳过）"
            continue
        fi

        if gocryptfs -init -passfile "$passfile" "$cipher_path" >/dev/null 2>&1; then
            echo "    ✅ $name"
            # 初始化后再次修正权限
            fix_permissions "$cipher_path"
            chmod 700 "$cipher_path"
        else
            echo "    ❌ $name 初始化失败"
        fi
    done

    rm -f "$passfile"
fi

# ---------------- 生成测试配置 ----------------
echo "==> 生成测试配置: $CONFIG_FILE"
cat > "$CONFIG_FILE" <<EOF
# 测试环境配置（自动生成）
# 生成时间: $(date -Iseconds)

settings:
  unlock_mode: "755"
  lock_mode: "555"
  gocryptfs:
    allow_other: true
  rsync:
    archive: true
    compress: true
    partial: true
  create:
    keep_source: false
    tmp_mount_suffix: ".mount_tmp"
  remove:
    restore: true
    direct_delete_cipher: false
    restore_target_mode: in_place

vaults:
  - id: 1
    name: test_plain
    path: $TEST_ROOT/cipher/test_plain
    mount_point: $TEST_ROOT/plain/test_plain
  - id: 2
    name: test_secret
    path: $TEST_ROOT/cipher/test_secret
    mount_point: $TEST_ROOT/plain/test_secret

pending:
  - source_dir: $TEST_ROOT/plain-src/photos
  - source_dir: $TEST_ROOT/plain-src/documents
EOF

fix_permissions "$TEST_ROOT"

# ---------------- 输出结果 ----------------
echo ""
echo "=========================================="
echo "[✔] 测试环境已生成"
echo "=========================================="
echo "  根目录:     $TEST_ROOT"
echo "  配置文件:   $CONFIG_FILE"
echo "  明文源:     $TEST_ROOT/plain-src/"
echo "  密文存储:   $TEST_ROOT/cipher/"
echo "  挂载点:     $TEST_ROOT/plain/"
echo ""

if [ "$WITH_CIPHER" = true ]; then
    echo "  加密卷密码: $PASSWORD"
    echo ""
fi

echo "使用方式："
echo ""
echo "  # CLI 测试"
echo "  bash shell/gocryptfs-cli -c $CONFIG_FILE list"
echo "  echo '$PASSWORD' | bash shell/gocryptfs-cli -c $CONFIG_FILE mount test_plain"
echo ""
echo "  # TUI 测试"
echo "  cp $CONFIG_FILE ~/.config/gocryptfs-tui/config.yaml"
echo "  gocryptfs-tui"
echo ""
echo "  # 清理测试环境"
echo "  bash test/create-test-env.sh --clean"
echo "=========================================="
