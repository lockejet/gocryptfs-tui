#!/bin/bash
# test-all.sh: 运行所有测试批次
set -uo pipefail
SCRIPT_DIR="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

TOTAL_FAIL=0
for t in "$SCRIPT_DIR"/test-batch*.sh; do
    echo ""
    echo "════════════════════════════════════════"
    echo " 运行: $(basename "$t")"
    echo "════════════════════════════════════════"
    if ! bash "$t"; then
        TOTAL_FAIL=$((TOTAL_FAIL+1))
    fi
done

echo ""
if [ $TOTAL_FAIL -eq 0 ]; then
    echo "🎉 全部测试批次通过"
    exit 0
else
    echo "⚠️  $TOTAL_FAIL 个批次有失败"
    exit 1
fi
