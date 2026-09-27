cat > test/create-test-env.sh << 'TESTENV_EOF'
#!/bin/bash
# create-test-env.sh: 生成/清理 gocryptfs-tui 的测试环境
#
# 用法：
#   bash test/create-test-env.sh              # 默认：生成环境（含加密卷初始化）
#   bash test/create-test-env.sh --clean      # 卸载所有挂载点 + 删除测试环境
#   bash test/create-test-env.sh --pass xxx   # 自定义密码
#   bash test/create-test-env.sh --no-cipher  # 只生成配置，不初始化加密卷
#   bash test/create-test-env.sh --keep       # 保留旧环境（不清理）
#
# 环境变量：
#   TEST_ROOT   测试根目录（默认 /tmp/gocryptfs-tui-test）

set -euo pipefail

TEST_ROOT="${TEST_ROOT:-/tmp/gocryptfs-tui-test}"
CONFIG_FILE="$TEST_ROOT/test-config.yaml"
DEFAULT_PASSWORD="test-password-12345"

# ---------------- 清理函数 ----------------
clean_env() {
    if [ ! -d "$TEST_ROOT" ]; then
        echo "==> 测试环境不存在，无需清理: $TEST_ROOT"
        return 0
    fi

    echo "==> 清理测试环境: $TEST_ROOT"

    # 1. 卸载 plain/ 下所有挂载点
    if [ -d "$TEST_ROOT/plain" ]; then
        for mp in "$TEST_ROOT"/plain/*; do
            [ -d "$mp" ] || continue
            if mountpoint -q "$mp" 2>/dev/null; then
                echo "    卸载: $mp"
                if ! fusermount -u "$mp" 2>/dev/null; then
                    echo "    常规卸载失败，尝试 lazy unmount"
                    fusermount -u -z "$mp" 2>/dev/null || \
                        echo "    ⚠️  无法卸载: $mp"
                fi
            fi
        done
    fi

    # 2. 卸载任何 .mount_tmp / .restore_tmp 残留
    for mp in "$TEST_ROOT"/plain-src/*.mount_tmp "$TEST_ROOT"/plain-src/*.restore_tmp; do
        [ -d "$mp" ] || continue
        if mountpoint -q "$mp" 2>/dev/null; then
            echo "    卸载临时点: $mp"
            fusermount -u -z "$mp" 2>/dev/null || true
        fi
    done

    # 3. 删除目录
    rm -rf "$TEST_ROOT"
    echo "    ✅ 已删除: $TEST_ROOT"
    echo "==> 清理完成"
}

# ---------------- 参数 ----------------
PASSWORD="$DEFAULT_PASSWORD"
WITH_CIPHER=true
KEEP_EXISTING=false
CLEAN_ONLY=false

while [ $# -gt 0 ]; do
    case "$1" in
        --clean)
            CLEAN_ONLY=true; shift ;;
        --pass)
            PASSWORD="$2"; shift 2 ;;
        --no-cipher)
            WITH_CIPHER=false; shift ;;
        --keep)
            KEEP_EXISTING=true; shift ;;
        -h|--help)
            cat <<EOF
用法: $0 [OPTIONS]

选项:
  --clean             卸载所有挂载点并删除测试环境（不生成）
  --pass <password>   指定密码（默认: $DEFAULT_PASSWORD）
  --no-cipher         只生成配置，不初始化加密卷
  --keep              保留已存在的测试环境（默认先清理）

环境变量:
  TEST_ROOT           测试根目录（默认: /tmp/gocryptfs-tui-test）

默认密码: $DEFAULT_PASSWORD

示例:
  $0                  # 生成测试环境
  $0 --clean          # 清理测试环境
  $0 --clean && $0    # 先清理再重新生成
EOF
            exit 0 ;;
        *)
            echo "未知选项: $1" >&2; exit 1 ;;
    esac
done

# ---------------- --clean：只清理后退出 ----------------
if [ "$CLEAN_ONLY" = true ]; then
    clean_env
    exit 0
fi

# ---------------- 依赖检查 ----------------
if [ "$WITH_CIPHER" = true ] && ! command -v gocryptfs >/dev/null 2>&1; then
    echo "[!] gocryptfs 未安装，无法初始化加密卷" >&2
    echo "    提示: 用 --no-cipher 只生成配置" >&2
    exit 1
fi

# ---------------- 清理旧环境 ----------------
if [ "$KEEP_EXISTING" = false ] && [ -d "$TEST_ROOT" ]; then
    clean_env
fi

# ---------------- 目录结构 ----------------
echo "==> 创建目录结构: $TEST_ROOT"
mkdir -p "$TEST_ROOT"/{cipher,plain,plain-src/{photos,documents}}

# ---------------- 生成测试数据 ----------------
echo "==> 生成测试数据"

# photos 目录
mkdir -p "$TEST_ROOT/plain-src/photos/2024"
echo "Summer vacation photo #1" > "$TEST_ROOT/plain-src/photos/README.md"
echo "binary-data-$(date +%s)" > "$TEST_ROOT/plain-src/photos/img001.raw"
echo "binary-data-$(date +%s)" > "$TEST_ROOT/plain-src/photos/img002.raw"
for i in $(seq 1 10); do
    head -c 1024 /dev/urandom > "$TEST_ROOT/plain-src/photos/2024/file$i.bin"
done

# documents 目录（含过滤器能命中的文件）
echo "Important content" > "$TEST_ROOT/plain-src/documents/report.md"
echo "temp file" > "$TEST_ROOT/plain-src/documents/draft.tmp"
mkdir -p "$TEST_ROOT/plain-src/documents/.git"
echo "git metadata" > "$TEST_ROOT/plain-src/documents/.git/config"

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

        if [ -f "$cipher_path/gocryptfs.conf" ]; then
            echo "    已存在: $name（跳过）"
            continue
        fi

        if gocryptfs -init -passfile "$passfile" "$cipher_path" >/dev/null 2>&1; then
            echo "    ✅ $name"
        else
            echo "    ❌ $name 初始化失败"
        fi
    done

    rm -f "$passfile"
fi

# ---------------- 生成测试配置 ----------------
echo "==> 生成测试配置: $CONFIG_FILE"
cat > "$CONFIG_FILE" <<EOF
# 测试环境配置（自动生成，请勿提交到 git）
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

pending: []
EOF

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
    echo "  （挂载时输入此密码）"
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
echo "  ./target/release/gocryptfs-tui"
echo ""
echo "  # 清理测试环境"
echo "  bash test/create-test-env.sh --clean"
echo "=========================================="
