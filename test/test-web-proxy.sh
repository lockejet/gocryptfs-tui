#!/bin/bash
# test-web-proxy.sh: 反向代理场景测试（--public-url / --trust-proxy）
#
# 位置: <项目根>/test/test-web-proxy.sh
# 用法: bash test/test-web-proxy.sh            # 默认用 target/release 或 target/debug 的 gocryptfs-web
#       WEB_BIN=/path/to/gocryptfs-web bash test/test-web-proxy.sh
#
# 覆盖：
#   1) --public-url + --trust-proxy：二维码/一次性链接用 public-url、cookie 带 Secure、
#      meta 返回 public_url、Origin 校验接受 forward 后的域名
#   2) 登录限流：信任代理时按 X-Forwarded-For 的真实 IP 计数
#   3) 未开 --trust-proxy：忽略伪造的 X-Forwarded-*（cookie 无 Secure、链接用监听地址、伪造 XFF 不生效）

set -uo pipefail
LC_ALL=C; export LC_ALL

SCRIPT_DIR="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd -P "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

WEB_BIN="${WEB_BIN:-$PROJECT_ROOT/target/release/gocryptfs-web}"
[ -x "$WEB_BIN" ] || WEB_BIN="$PROJECT_ROOT/target/debug/gocryptfs-web"
if [ ! -x "$WEB_BIN" ]; then
    echo "找不到 gocryptfs-web（先 make build / cargo build）" >&2
    exit 1
fi
command -v curl >/dev/null || { echo "需要 curl" >&2; exit 1; }

WORK="$(mktemp -d /tmp/gfs-web-proxy.XXXXXX)" || exit 1
PIDS=()
cleanup() {
    for pid in "${PIDS[@]:-}"; do
        [ -n "${pid:-}" ] && kill "$pid" 2>/dev/null
    done
    rm -rf "$WORK"
}
trap cleanup EXIT

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

pick_port() {
    local port=$1
    while ss -ltn 2>/dev/null | grep -q ":$port "; do port=$((port+1)); done
    echo "$port"
}
PORT_A="$(pick_port 39132)"
PORT_B="$(pick_port $((PORT_A+1)))"

wait_ready() {
    local base=$1 i
    for i in $(seq 1 50); do
        if curl -fsS -o /dev/null --max-time 1 "$base/healthz" 2>/dev/null; then return 0; fi
        sleep 0.1
    done
    return 1
}

# 结果放入全局 SERVER_PID（不要把函数放进 $(...) 子 shell，否则 PIDS 不会回传）
SERVER_PID=""
start_server() { # $1=dir $2=port $3..=extra args
    local dir=$1 port=$2; shift 2
    mkdir -p "$dir"
    "$WEB_BIN" --listen "127.0.0.1:$port" --token-file "$dir/web.token" "$@" \
        > "$dir/server.log" 2>&1 &
    SERVER_PID=$!
    PIDS+=("$SERVER_PID")
    if ! wait_ready "http://127.0.0.1:$port"; then
        echo "  服务启动失败（$port）：" >&2
        sed 's/^/    /' "$dir/server.log" >&2
        return 1
    fi
}

token_of() {
    grep -oE 'token:      [A-Za-z0-9_-]+' "$1" | head -1 | sed 's/token:      //'
}

login() { # $1=base $2=token $3=cookiejar $4=headers  [$5]=extra curl args...
    local base=$1 token=$2 jar=$3 hdr=$4; shift 4
    curl -s -D "$hdr" -c "$jar" -o /dev/null -w '%{http_code}' \
        -X POST -H 'content-type: application/json' -H 'x-gfs-csrf: 1' \
        --data "{\"token\":\"$token\"}" "$@" "$base/api/session"
}

echo "===== 反向代理场景测试 ====="
echo "  二进制: $WEB_BIN"
echo "  端口:   A=$PORT_A B=$PORT_B"
echo ""

# ---------- A：--public-url https://example.com + --trust-proxy ----------
DIR_A="$WORK/a"
start_server "$DIR_A" "$PORT_A" --public-url https://example.com --trust-proxy || exit 1
PID_A="$SERVER_PID"
TOKEN_A="$(token_of "$DIR_A/server.log")"
BASE_A="http://127.0.0.1:$PORT_A"

HDR_A="$WORK/a.hdr"
JAR_A="$WORK/a.jar"
code="$(login "$BASE_A" "$TOKEN_A" "$JAR_A" "$HDR_A")"
check "A: token 登录 204" "204" "$code"
check "A: cookie 带 Secure（public-url 为 https）" "yes" \
    "$(grep -iq 'set-cookie:.*secure' "$HDR_A" && echo yes || echo no)"
check "A: 一次性链接使用 public-url" "https://example.com" \
    "$(curl -s -b "$JAR_A" -X POST -H 'content-type: application/json' -H 'x-gfs-csrf: 1' -d '{}' \
        "$BASE_A/api/login-links" | grep -oE '"url":"https://example.com' | sed 's/"url":"//')"
check "A: meta 返回 public_url" "https://example.com" \
    "$(curl -s -b "$JAR_A" "$BASE_A/api/meta" | python3 -c 'import json,sys;print(json.load(sys.stdin).get("public_url"))')"
check "A: meta 返回 trust_proxy=true" "True" \
    "$(curl -s -b "$JAR_A" "$BASE_A/api/meta" | python3 -c 'import json,sys;print(json.load(sys.stdin).get("trust_proxy"))')"
code="$(curl -s -o /dev/null -w '%{http_code}' -b "$JAR_A" -X POST \
    -H 'content-type: application/json' -H 'x-gfs-csrf: 1' \
    -H 'origin: https://example.com' -H 'x-forwarded-host: example.com' -H 'x-forwarded-proto: https' \
    -d '{}' "$BASE_A/api/login-links")"
check "A: Origin=https://example.com 通过" "200" "$code"
code="$(curl -s -o /dev/null -w '%{http_code}' -b "$JAR_A" -X POST \
    -H 'content-type: application/json' -H 'x-gfs-csrf: 1' \
    -H 'origin: https://evil.example' -d '{}' "$BASE_A/api/login-links")"
check "A: Origin=evil 被拒绝" "403" "$code"

# 密码用户 + 按 X-Forwarded-For 限流
printf 'pw12345\n' | "$WEB_BIN" user add --token-file "$DIR_A/web.token" alice >/dev/null 2>&1
fail_login() { # $1=xff
    curl -s -o /dev/null -w '%{http_code}' -X POST -H 'content-type: application/json' -H 'x-gfs-csrf: 1' \
        -H "x-forwarded-for: $1" --data '{"username":"alice","password":"wrong"}' "$BASE_A/api/session"
}
for _ in 1 2 3 4 5; do fail_login "203.0.113.9" >/dev/null; done
check "A: 信任代理时 XFF 相同 → 429" "429" "$(fail_login "203.0.113.9")"
check "A: 信任代理时 XFF 不同 → 401（未误锁）" "401" "$(fail_login "203.0.113.10")"

# ---------- B：默认（不信任代理） ----------
DIR_B="$WORK/b"
start_server "$DIR_B" "$PORT_B" || exit 1
PID_B="$SERVER_PID"
TOKEN_B="$(token_of "$DIR_B/server.log")"
BASE_B="http://127.0.0.1:$PORT_B"
HDR_B="$WORK/b.hdr"
JAR_B="$WORK/b.jar"
code="$(login "$BASE_B" "$TOKEN_B" "$JAR_B" "$HDR_B")"
check "B: token 登录 204" "204" "$code"
check "B: cookie 无 Secure" "no" \
    "$(grep -iq 'set-cookie:.*secure' "$HDR_B" && echo yes || echo no)"
check "B: 一次性链接用监听地址" "http://127.0.0.1:$PORT_B" \
    "$(curl -s -b "$JAR_B" -X POST -H 'content-type: application/json' -H 'x-gfs-csrf: 1' -d '{}' \
        "$BASE_B/api/login-links" | grep -oE '"url":"http://127.0.0.1:[0-9]+' | sed 's/"url":"//')"
# 伪造 XFF 不应改变限流 key（真实 IP 相同 → 仍然 429）
printf 'pw12345\n' | "$WEB_BIN" user add --token-file "$DIR_B/web.token" bob >/dev/null 2>&1
fail_login_b() {
    curl -s -o /dev/null -w '%{http_code}' -X POST -H 'content-type: application/json' -H 'x-gfs-csrf: 1' \
        -H "x-forwarded-for: $1" --data '{"username":"bob","password":"wrong"}' "$BASE_B/api/session"
}
for _ in 1 2 3 4 5; do fail_login_b "198.51.100.1" >/dev/null; done
check "B: 未信任代理 → 伪造 XFF 不生效（仍 429）" "429" "$(fail_login_b "198.51.100.2")"

echo ""
echo "===== 结果: $PASS 通过, $FAIL 失败 ====="
[ "$FAIL" -eq 0 ] || exit 1
