#!/bin/bash
# cleanup-test-env.sh: 强制清理所有 gocryptfs-tui 测试环境
#
# 用法: bash test/cleanup-test-env.sh

set -uo pipefail

echo "==> 扫描所有相关挂载"
MOUNTS=$(mount | grep "gocryptfs-tui-test" || true)

if [ -z "$MOUNTS" ]; then
    echo "    无相关挂载"
else
    echo "$MOUNTS"
    echo ""
    echo "==> 卸载"
    echo "$MOUNTS" | awk '{print $3}' | sort -r | while read -r mp; do
        [ -z "$mp" ] && continue
        echo "    卸载: $mp"
        fusermount -u "$mp" 2>/dev/null || \
            fusermount -u -z "$mp" 2>/dev/null || \
            echo "      ⚠️  失败: $mp"
    done
fi

echo ""
echo "==> 等待内核清理"
sleep 0.5

echo "==> 检查残留"
REMAINING=$(mount | grep "gocryptfs-tui-test" || true)
if [ -n "$REMAINING" ]; then
    echo "    ⚠️  仍有残留："
    echo "$REMAINING"
    echo ""
    echo "==> 尝试 fuser -km 强制清理"
    sudo fuser -km /tmp/gocryptfs-tui-test /tmp/gocryptfs-tui-test-b2 /tmp/gocryptfs-tui-test-empty 2>/dev/null || true
    sleep 1
fi

echo ""
echo "==> 删除目录"
for d in /tmp/gocryptfs-tui-test /tmp/gocryptfs-tui-test-b2 /tmp/gocryptfs-tui-test-empty; do
    if [ -d "$d" ]; then
        echo "    删除: $d"
        rm -rf "$d" 2>/dev/null || sudo rm -rf "$d" 2>/dev/null || echo "      ⚠️  失败: $d"
    fi
done

echo ""
echo "==> 最终检查"
mount | grep "gocryptfs-tui-test" && echo "⚠️  仍有挂载残留" || echo "✅ 无挂载残留"
ls /tmp/ | grep "gocryptfs-tui-test" && echo "⚠️  仍有目录残留" || echo "✅ 无目录残留"
echo ""
echo "清理完成"
