#!/bin/bash
# gocryptfs-lib.sh: gocryptfs-cli 的全部库函数（轮询挂载点版）
#
# 与 -notifypid 版的区别：
#   §8.5 gocryptfs_with_password 使用"后台运行 + 轮询挂载点"方案
#   不依赖 -notifypid，兼容所有 gocryptfs 版本
#
# 原理：
#   1. gocryptfs 后台运行，输出重定向到临时文件（避免管道阻塞）
#   2. 轮询挂载点是否出现（最多 30 秒）
#   3. 挂载成功 → 进程后台继续运行，脚本返回 0
#   4. 进程提前退出 → 取退出码判断（可能是密码错误）
#   5. 超时 → kill 进程，返回 1

# § 0. 函数索引
if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
    echo "gocryptfs-lib.sh — 函数索引（轮询版）"
    exit 0
fi

# § 1. 常量
LOG_FILE="${LOG_FILE:-$HOME/.local/share/gocryptfs-tui/app.log.jsonl}"
LOCK_FILE="${LOCK_FILE:-/tmp/gocryptfs-tui.lock}"
EXIT_OK=0; EXIT_ERROR=1; EXIT_PASSWORD=2; EXIT_STATE=3
EXIT_MOUNTPOINT=4; EXIT_NOSPACE=5; EXIT_UMOUNT=6
EXIT_UMOUNT_FORCE=7; EXIT_CONFIG=8

# § 2. 帮助
show_help() {
    cat <<'EOF'
gocryptfs-cli - gocryptfs-tui 的 Shell 后端
用法: gocryptfs-cli [-c <config>] <command> [options]
命令: list / info / ls / tree / mount / umount / create / remove
      config / edit / check-deps / log / help

log 子命令选项:
  --limit N        显示最近 N 条（默认 20）
  --src SRC        过滤来源: cli / tui
  --action ACTION  过滤操作: mount / umount / create / remove / tui.start / ...
  --result RESULT  过滤结果: success / failed / started / cancelled
  --since DATE     起始时间（ISO 8601）
  --follow         实时跟踪（类似 tail -f）
  --json           JSON 输出
EOF
}

check_deps() {
    local missing=()
    for cmd in gocryptfs fusermount rsync yq jq mountpoint; do
        command -v "$cmd" >/dev/null 2>&1 || missing+=("$cmd")
    done
    if [ ${#missing[@]} -gt 0 ]; then
        echo "[!] 缺少依赖: ${missing[*]}" >&2
        return 1
    fi
    echo "[✔] 依赖齐全" >&2
}

# § 3. 日志
log_line() {
    printf '[%s] %s\n' "$(date '+%H:%M:%S')" "$1" >&2
}

log_error() {
    printf '[!] %s\n' "$1" >&2
}

# 结构化事件日志（JSONL）
# 参数: $1=action $2=target $3=result $4=detail
log_event() {
    local action="$1" target="$2" result="$3" detail="${4:-}"

    mkdir -p "$(dirname "$LOG_FILE")"

    jq -cn \
        --arg ts "$(date -Iseconds)" \
        --arg src "cli" \
        --arg action "$action" \
        --arg target "$target" \
        --arg result "$result" \
        --arg detail "$detail" \
        '{ts:$ts,src:$src,action:$action,target:$target,result:$result,detail:$detail,pid:null,duration_ms:null}' \
        >> "$LOG_FILE" 2>/dev/null || true
}

# § 4. 协议
emit_progress() { printf '@@PROGRESS@@ %s %s %s\n' "$1" "$2" "$3" >&2; }
emit_check()    { printf '@@CHECK@@ %s %s\n' "$1" "$2" >&2; }
emit_done() {
    [ "$JSON_MODE" != true ] && echo "[✔] $1" >&2
    printf '@@DONE@@ %s\n' "$1" >&2
}
emit_error() {
    local code="$1" msg="$2"
    if [ "$JSON_MODE" = true ]; then
        jq -cn --argjson code "$code" --arg msg "$msg" \
            '{status:"error",code:$code,message:$msg}' >&2
    else
        echo "[✗] $msg" >&2
    fi
    printf '@@ERROR@@ %s %s\n' "$code" "$msg" >&2
    exit "$code"
}

# § 5. 配置
require_config() {
    [ -f "$CONFIG_FILE" ] || emit_error $EXIT_CONFIG "配置文件不存在: $CONFIG_FILE"
}
get_vault_field() {
    yq -r ".vaults[] | select(.name == \"$1\") | .$2 // \"\"" "$CONFIG_FILE"
}
vault_exists() {
    local n
    n=$(yq -r ".vaults[] | select(.name == \"$1\") | .name" "$CONFIG_FILE" | head -1)
    [ "$n" = "$1" ]
}
get_setting() {
    local val
    val=$(yq -r ".settings.$1 // \"\"" "$CONFIG_FILE" 2>/dev/null)
    if [ -z "$val" ] || [ "$val" = "null" ]; then echo "${2:-}"; else echo "$val"; fi
}
get_effective_setting() {
    local override
    override=$(yq -r ".vaults[] | select(.name == \"$1\") | .overrides.$2 // \"\"" "$CONFIG_FILE")
    if [ -n "$override" ] && [ "$override" != "null" ]; then
        echo "$override"
    else
        get_setting "$2" "${3:-}"
    fi
}
save_config_content() {
    local tmp="${CONFIG_FILE}.tmp.$$"
    printf '%s' "$1" > "$tmp"
    sync "$tmp" 2>/dev/null || true
    mv "$tmp" "$CONFIG_FILE"
}

# § 6. 状态
is_mounted() { mountpoint -q "$1" 2>/dev/null; }
dir_is_empty() { [ -z "$(ls -A "$1" 2>/dev/null)" ]; }

# § 7. 权限
lock_dir() {
    local mode; mode=$(get_setting "lock_mode" "555")
    [ "$DRY_RUN" = true ] && { log_line "[DRY-RUN] chmod $mode $1"; return 0; }
    [ -d "$1" ] && chmod "$mode" "$1" && log_line "已锁定 ($mode): $1"
}
unlock_dir() {
    local mode; mode=$(get_setting "unlock_mode" "755")
    [ "$DRY_RUN" = true ] && { log_line "[DRY-RUN] chmod $mode $1"; return 0; }
    [ -d "$1" ] && chmod "$mode" "$1" && log_line "已解锁 ($mode): $1"
}

# § 8. 密码
read_password_from_stdin() {
    local p; IFS= read -r p || true
    [ -z "$p" ] && emit_error $EXIT_PASSWORD "密码为空"
    echo "$p"
}
read_password_interactive() {
    local p1 p2
    while true; do
        printf '请输入密码: ' >&2; IFS= read -rs p1; printf '\n' >&2
        printf '再次输入: ' >&2; IFS= read -rs p2; printf '\n' >&2
        [ -z "$p1" ] && { echo "密码不能为空" >&2; continue; }
        [ "$p1" != "$p2" ] && { echo "两次密码不一致" >&2; continue; }
        break
    done
    echo "$p1"
}

# § 8.5. 用临时文件调用 gocryptfs（轮询挂载点）
GOCRYPTFS_LAST_OUTPUT=""

# 参数: $1=密码  $2..=gocryptfs 参数（挂载时最后一个参数必须是挂载点）
gocryptfs_with_password() {
    local password="$1"; shift

    local passfile output_file
    passfile=$(mktemp -t gocryptfs-pass.XXXXXX) || {
        log_error "无法创建密码临时文件"
        return 1
    }
    chmod 600 "$passfile"
    printf '%s' "$password" > "$passfile"

    output_file=$(mktemp -t gocryptfs-out.XXXXXX) || {
        rm -f "$passfile"
        log_error "无法创建输出临时文件"
        return 1
    }

    local is_init=false
    for arg in "$@"; do
        [ "$arg" = "-init" ] && is_init=true && break
    done

    local rc=0

    if [ "$is_init" = true ]; then
        gocryptfs -passfile "$passfile" "$@" > "$output_file" 2>&1
        rc=$?
    else
        local mount_point="${!#}"

        gocryptfs -passfile "$passfile" "$@" < /dev/null > "$output_file" 2>&1 &
        local gpid=$!

        local mounted=false
        local waited=0
        while [ "$waited" -lt 300 ]; do
            if is_mounted "$mount_point"; then
                mounted=true
                break
            fi
            if ! kill -0 "$gpid" 2>/dev/null; then
                wait "$gpid" 2>/dev/null
                rc=$?
                break
            fi
            sleep 0.1
            waited=$((waited + 1))
        done

        if [ "$mounted" = true ]; then
            rc=0
        elif [ "$rc" = "0" ] && [ "$mounted" = false ]; then
            kill "$gpid" 2>/dev/null || true
            wait "$gpid" 2>/dev/null || true
            rc=1
            GOCRYPTFS_LAST_OUTPUT="挂载超时（30 秒内挂载点未出现）"
        fi
    fi

    if [ -z "$GOCRYPTFS_LAST_OUTPUT" ]; then
        GOCRYPTFS_LAST_OUTPUT=$(cat "$output_file" 2>/dev/null)
    fi
    if [ -n "$GOCRYPTFS_LAST_OUTPUT" ]; then
        printf '%s\n' "$GOCRYPTFS_LAST_OUTPUT" >&2
    fi

    rm -f "$output_file" "$passfile"
    return $rc
}

gocryptfs_output_is_password_error() {
    printf '%s' "$GOCRYPTFS_LAST_OUTPUT" | grep -qiE 'password.*incorrect|incorrect.*password|fatal: password|Wrong password'
}

# § 9. 容量
check_space() {
    local src="$1" tdir="$2"
    [ -e "$src" ] || { log_error "源不存在: $src"; return 1; }
    if [ ! -e "$tdir" ]; then
        local p="$tdir"
        while [ ! -e "$p" ] && [ "$p" != "/" ]; do p="$(dirname "$p")"; done
        tdir="$p"
    fi
    local ssz tfree req
    ssz=$(du -sb "$src" 2>/dev/null | awk '{print $1}')
    tfree=$(df -B1 --output=avail "$tdir" 2>/dev/null | tail -1 | tr -d ' ')
    if [ -z "$ssz" ] || [ -z "$tfree" ]; then
        log_error "无法计算容量"
        return 1
    fi
    req=$(( ssz * 111 / 100 ))
    emit_check "src_size" "$ssz"
    emit_check "target_free" "$tfree"
    emit_check "required" "$req"
    [ "$tfree" -lt "$req" ] && return 5
    return 0
}

# § 10. 锁
acquire_lock() {
    exec 200>"$LOCK_FILE"
    flock -n 200 || emit_error $EXIT_CONFIG "另一个进程正在运行"
}

# § 11. 确认
confirm() {
    [ "$JSON_MODE" = true ] && return 0
    local prompt="$1" default="${2:-n}" hint="[y/N]"
    [ "$default" = "y" ] && hint="[Y/n]"
    printf '%s %s ' "$prompt" "$hint" >&2
    local r; read -r r; r="${r:-$default}"
    [[ "$r" =~ ^[Yy]$ ]]
}

# § 12. list / info
cmd_list() {
    require_config
    if [ "$JSON_MODE" = true ]; then _list_json; else _list_table; fi
}
_list_json() {
    local vaults_json="[]" count
    count=$(yq -r '.vaults // [] | length' "$CONFIG_FILE" 2>/dev/null || echo 0)
    for ((i=0; i<count; i++)); do
        local id name path mp mounted=false locked=false valid=false
        id=$(yq -r ".vaults[$i].id // 0" "$CONFIG_FILE" 2>/dev/null)
        name=$(yq -r ".vaults[$i].name // \"\"" "$CONFIG_FILE" 2>/dev/null)
        path=$(yq -r ".vaults[$i].path // \"\"" "$CONFIG_FILE" 2>/dev/null)
        mp=$(yq -r ".vaults[$i].mount_point // \"\"" "$CONFIG_FILE" 2>/dev/null)
        [ -n "$path" ] && [ -f "$path/gocryptfs.conf" ] && valid=true
        is_mounted "$mp" && mounted=true
        if [ "$mounted" = false ] && [ -d "$mp" ]; then
            local m; m=$(stat -c '%a' "$mp" 2>/dev/null); m="${m: -3}"
            [ "$m" = "555" ] && locked=true
        fi
        vaults_json=$(jq -cn \
            --argjson prev "$vaults_json" \
            --argjson id "${id:-0}" \
            --arg name "$name" --arg path "$path" --arg mp "$mp" \
            --argjson mounted "$mounted" --argjson locked "$locked" \
            --argjson valid "$valid" \
            '$prev + [{id:$id,name:$name,path:$path,mount_point:$mp,mounted:$mounted,locked:$locked,valid:$valid}]')
    done
    jq -cn --argjson vaults "$vaults_json" '{status:"ok",data:{vaults:$vaults}}'
    printf '@@DONE@@ list\n' >&2
}
_list_table() {
    local count
    count=$(yq -r '.vaults // [] | length' "$CONFIG_FILE" 2>/dev/null || echo 0)
    if [ "$count" -eq 0 ]; then
        echo "(配置中没有任何卷)"
        return 0
    fi
    printf '%-20s %-12s %-10s %s\n' "NAME" "STATUS" "LOCKED" "MOUNT"
    printf '%-20s %-12s %-10s %s\n' "----" "------" "------" "-----"
    for ((i=0; i<count; i++)); do
        local name path mp status locked_str
        name=$(yq -r ".vaults[$i].name // \"\"" "$CONFIG_FILE")
        path=$(yq -r ".vaults[$i].path // \"\"" "$CONFIG_FILE")
        mp=$(yq -r ".vaults[$i].mount_point // \"\"" "$CONFIG_FILE")
        if [ ! -f "$path/gocryptfs.conf" ]; then
            status="invalid"; locked_str="-"
        elif is_mounted "$mp"; then
            status="mounted"; locked_str="-"
        else
            status="unmounted"
            if [ -d "$mp" ]; then
                local m; m=$(stat -c '%a' "$mp" 2>/dev/null); m="${m: -3}"
                [ "$m" = "555" ] && locked_str="yes" || locked_str="no"
            else
                locked_str="no-dir"
            fi
        fi
        printf '%-20s %-12s %-10s %s\n' "$name" "$status" "$locked_str" "$mp"
    done
}
cmd_info() {
    require_config
    local name="$1"
    [ -z "$name" ] && emit_error $EXIT_ERROR "用法: info <name>"
    vault_exists "$name" || emit_error $EXIT_ERROR "卷不存在: $name"
    local id path mp mounted=false locked=false valid=false mode="-"
    id=$(get_vault_field "$name" "id")
    path=$(get_vault_field "$name" "path")
    mp=$(get_vault_field "$name" "mount_point")
    [ -f "$path/gocryptfs.conf" ] && valid=true
    is_mounted "$mp" && mounted=true
    if [ -d "$mp" ]; then
        mode=$(stat -c '%a' "$mp" 2>/dev/null)
        [ "${mode: -3}" = "555" ] && locked=true
    fi
    if [ "$JSON_MODE" = true ]; then
        jq -cn --argjson id "${id:-0}" --arg name "$name" --arg path "$path" \
            --arg mp "$mp" --argjson mounted "$mounted" \
            --argjson locked "$locked" --arg mode "$mode" \
            --argjson valid "$valid" \
            '{status:"ok",data:{id:$id,name:$name,path:$path,mount_point:$mp,mounted:$mounted,locked:$locked,mode:$mode,valid:$valid}}'
        printf '@@DONE@@ info\n' >&2
    else
        echo "ID: $id"; echo "名称: $name"
        echo "加密路径: $path"; echo "挂载点: $mp"
        echo "已挂载: $mounted"; echo "已锁定: $locked"
        echo "权限: $mode"; echo "有效: $valid"
    fi
}

# § 13. ls / tree
cmd_ls() {
    require_config
    local name="$1"
    [ -z "$name" ] && emit_error $EXIT_ERROR "用法: ls <name>"
    vault_exists "$name" || emit_error $EXIT_ERROR "卷不存在: $name"
    local mp; mp=$(get_vault_field "$name" "mount_point")
    is_mounted "$mp" || emit_error $EXIT_STATE "卷未挂载: $name"
    if [ "$JSON_MODE" = true ]; then
        local lines="[]"
        while IFS= read -r l; do
            lines=$(jq -cn --argjson prev "$lines" --arg l "$l" '$prev + [$l]')
        done < <(ls -la "$mp" 2>/dev/null)
        jq -cn --argjson lines "$lines" --arg path "$mp" \
            '{status:"ok",data:{path:$path,lines:$lines}}'
        printf '@@DONE@@ ls\n' >&2
    else
        ls -la "$mp"
    fi
}
cmd_tree() {
    require_config
    local name="$1"
    [ -z "$name" ] && emit_error $EXIT_ERROR "用法: tree <name>"
    vault_exists "$name" || emit_error $EXIT_ERROR "卷不存在: $name"
    local mp; mp=$(get_vault_field "$name" "mount_point")
    is_mounted "$mp" || emit_error $EXIT_STATE "卷未挂载: $name"
    command -v tree >/dev/null 2>&1 || emit_error $EXIT_ERROR "tree 未安装"
    if [ "$JSON_MODE" = true ]; then
        local lines="[]"
        while IFS= read -r l; do
            lines=$(jq -cn --argjson prev "$lines" --arg l "$l" '$prev + [$l]')
        done < <(tree -L 2 "$mp" 2>/dev/null)
        jq -cn --argjson lines "$lines" --arg path "$mp" \
            '{status:"ok",data:{path:$path,lines:$lines}}'
        printf '@@DONE@@ tree\n' >&2
    else
        tree -L 2 "$mp"
    fi
}

# § 14. mount
cmd_mount() {
    require_config
    local name="$1"
    [ -z "$name" ] && emit_error $EXIT_ERROR "用法: mount <name>"
    vault_exists "$name" || emit_error $EXIT_ERROR "卷不存在: $name"

    local path mp
    path=$(get_vault_field "$name" "path")
    mp=$(get_vault_field "$name" "mount_point")

    is_mounted "$mp" && emit_error $EXIT_STATE "卷已挂载: $name"
    [ -d "$path" ] || emit_error $EXIT_ERROR "加密目录不存在: $path"
    [ -f "$path/gocryptfs.conf" ] || \
        emit_error $EXIT_ERROR "不是有效的 gocryptfs 加密卷（缺少 gocryptfs.conf）: $name"

    [ ! -d "$mp" ] && { log_line "创建挂载点: $mp"; [ "$DRY_RUN" = false ] && mkdir -p "$mp"; }

    local allow_nonempty
    allow_nonempty=$(get_effective_setting "$name" "gocryptfs.nonempty" "false")
    if [ -n "$(ls -A "$mp" 2>/dev/null)" ] && [ "$allow_nonempty" != "true" ]; then
        emit_error $EXIT_MOUNTPOINT "挂载点非空: $mp（需设置 nonempty: true）"
    fi

    local password
    if [ -t 0 ]; then password=$(read_password_interactive); else password=$(read_password_from_stdin); fi

    local opts=()
    [ "$(get_effective_setting "$name" "gocryptfs.allow_other" "false")"     = "true" ] && opts+=("-allow_other")
    [ "$(get_effective_setting "$name" "gocryptfs.allow_root" "false")"      = "true" ] && opts+=("-allow_root")
    [ "$(get_effective_setting "$name" "gocryptfs.read_only" "false")"       = "true" ] && opts+=("-ro")
    [ "$(get_effective_setting "$name" "gocryptfs.nosuid" "false")"          = "true" ] && opts+=("-nosuid")
    [ "$(get_effective_setting "$name" "gocryptfs.nodev" "false")"           = "true" ] && opts+=("-nodev")
    [ "$(get_effective_setting "$name" "gocryptfs.noexec" "false")"          = "true" ] && opts+=("-noexec")
    [ "$(get_effective_setting "$name" "gocryptfs.nonempty" "false")"        = "true" ] && opts+=("-nonempty")
    [ "$(get_effective_setting "$name" "gocryptfs.kernel_cache" "false")"    = "true" ] && opts+=("-kernel_cache")
    [ "$(get_effective_setting "$name" "gocryptfs.one_file_system" "false")" = "true" ] && opts+=("-one_file_system")

    if [ "$DRY_RUN" = true ]; then
        log_line "[DRY-RUN] gocryptfs ${opts[*]} $path $mp"
        printf '@@DONE@@ mount (dry-run)\n' >&2
        return 0
    fi

    unlock_dir "$mp"
    log_line "执行: gocryptfs ${opts[*]} $path $mp"

    if ! gocryptfs_with_password "$password" "${opts[@]}" "$path" "$mp"; then
        lock_dir "$mp"
        log_event "mount" "$name" "failed"
        if gocryptfs_output_is_password_error; then
            emit_error $EXIT_PASSWORD "密码错误"
        else
            emit_error $EXIT_ERROR "挂载失败: $name"
        fi
    fi

    if ! is_mounted "$mp"; then
        lock_dir "$mp"
        log_event "mount" "$name" "failed"
        emit_error $EXIT_ERROR "挂载失败: $name（gocryptfs 未成功挂载）"
    fi

    log_event "mount" "$name" "success"
    emit_done "已挂载: $name"
}

# § 15. umount
cmd_umount() {
    require_config
    local name="" force=false
    while [ $# -gt 0 ]; do
        case "$1" in
            --force) force=true; shift ;;
            -*) emit_error $EXIT_ERROR "未知选项: $1" ;;
            *) name="$1"; shift ;;
        esac
    done
    [ -z "$name" ] && emit_error $EXIT_ERROR "用法: umount <name> [--force]"
    vault_exists "$name" || emit_error $EXIT_ERROR "卷不存在: $name"

    local mp; mp=$(get_vault_field "$name" "mount_point")
    is_mounted "$mp" || emit_error $EXIT_STATE "卷未挂载: $name"

    if [ "$DRY_RUN" = true ]; then
        log_line "[DRY-RUN] fusermount -u $mp"
        printf '@@DONE@@ umount (dry-run)\n' >&2
        return 0
    fi

    log_line "执行: fusermount -u $mp"
    local rc=0
    fusermount -u "$mp" 2>&1 || rc=$?

    if [ "$rc" -ne 0 ]; then
        if [ "$force" = true ]; then
            fusermount -u -z "$mp" 2>&1 || {
                log_event "umount" "$name" "failed" "强制卸载也失败"
                emit_error $EXIT_UMOUNT_FORCE "强制卸载失败: $name"
            }
        else
            log_event "umount" "$name" "failed"
            emit_error $EXIT_UMOUNT "卸载失败: $name（可尝试 --force）"
        fi
    fi

    lock_dir "$mp"
    log_event "umount" "$name" "success"
    emit_done "已卸载: $name"
}

# § 16. create
cmd_create() {
    require_config

    local src="" name="" cipher="" keep_source="" yes=false
    while [ $# -gt 0 ]; do
        case "$1" in
            --name)           name="$2"; shift 2 ;;
            --cipher)         cipher="$2"; shift 2 ;;
            --keep-source)    keep_source=true; shift ;;
            --no-keep-source) keep_source=false; shift ;;
            --yes|-y)         yes=true; shift ;;
            -*) emit_error $EXIT_ERROR "未知选项: $1" ;;
            *) [ -z "$src" ] && src="$1" || emit_error $EXIT_ERROR "多余参数: $1"; shift ;;
        esac
    done

    [ -z "$src" ]  && emit_error $EXIT_ERROR "缺少源目录"
    [ -z "$name" ] && emit_error $EXIT_ERROR "缺少 --name"
    [ -d "$src" ]  || emit_error $EXIT_ERROR "源目录不存在: $src"
    src="${src%/}"

    local existing
    existing=$(yq -r ".vaults[] | select(.mount_point == \"$src\") | .name" "$CONFIG_FILE" 2>/dev/null | head -1)
    if [ -n "$existing" ]; then
        emit_error $EXIT_STATE "源目录 $src 已绑定卷「$existing」，请先删除或使用其他源目录"
    fi

    [ -z "$cipher" ] && cipher="$(dirname "$src")/.cipher.d/$name"

    local tmp_mount="${src}$(get_setting "create.tmp_mount_suffix" ".mount_tmp")"

    local cfg_keep; cfg_keep=$(get_setting "create.keep_source" "false")
    if [ -z "$keep_source" ]; then
        keep_source="$cfg_keep"
    elif [ "$cfg_keep" = "true" ] && [ "$keep_source" = "false" ]; then
        keep_source=true
    fi

    local is_resume=false
    [ -d "$cipher" ] && is_resume=true

    if [ "$is_resume" = false ]; then
        check_space "$src" "$(dirname "$cipher")"
        [ $? -eq 5 ] && emit_error $EXIT_NOSPACE "磁盘空间不足"
    fi

    local password
    if [ -t 0 ]; then password=$(read_password_interactive); else password=$(read_password_from_stdin); fi

    if [ "$DRY_RUN" = true ]; then
        log_line "[DRY-RUN] mkdir $cipher"
        [ "$is_resume" = false ] && log_line "[DRY-RUN] gocryptfs -init $cipher"
        log_line "[DRY-RUN] gocryptfs $cipher $tmp_mount"
        log_line "[DRY-RUN] rsync $src/ $tmp_mount/"
        log_line "[DRY-RUN] fusermount -u $tmp_mount"
        log_line "[DRY-RUN] gocryptfs $cipher $src"
        printf '@@DONE@@ create (dry-run)\n' >&2
        return 0
    fi

    if [ "$is_resume" = false ]; then
        log_line "创建加密后端: $cipher"
        mkdir -p "$cipher" || emit_error $EXIT_ERROR "无法创建 $cipher"
        if ! gocryptfs_with_password "$password" -init "$cipher"; then
            rmdir "$cipher" 2>/dev/null || true
            emit_error $EXIT_ERROR "gocryptfs -init 失败"
        fi
    fi

    mkdir -p "$tmp_mount"
    is_mounted "$tmp_mount" && fusermount -u "$tmp_mount" 2>/dev/null || true

    log_line "挂载到临时点: $tmp_mount"
    if ! gocryptfs_with_password "$password" "$cipher" "$tmp_mount"; then
        if gocryptfs_output_is_password_error; then
            emit_error $EXIT_PASSWORD "密码错误"
        else
            emit_error $EXIT_ERROR "临时挂载失败"
        fi
    fi

    log_line "开始迁移: $src → $tmp_mount"
    local rsync_opts=("-a" "-h")
    [ "$keep_source" = "true" ] || rsync_opts+=("--remove-source-files")

    if ! rsync "${rsync_opts[@]}" "$src/" "$tmp_mount/" 2>&1; then
        fusermount -u "$tmp_mount" 2>/dev/null || true
        log_event "create" "$name" "failed" "rsync 迁移失败"
        emit_error $EXIT_ERROR "rsync 迁移失败"
    fi

    [ "$keep_source" = "false" ] && find "$src" -mindepth 1 -type d -empty -delete 2>/dev/null || true

    if [ "$yes" = false ] && [ "$JSON_MODE" != true ] && [ -t 0 ]; then
        confirm "确认替换挂载点 $src ？" "n" || {
            fusermount -u "$tmp_mount" 2>/dev/null || true
            emit_error $EXIT_STATE "用户取消"
        }
    fi

    fusermount -u "$tmp_mount" || emit_error $EXIT_UMOUNT "卸载临时点失败"
    rmdir "$tmp_mount" 2>/dev/null || true

    if [ -d "$src" ]; then
        if [ -n "$(ls -A "$src" 2>/dev/null)" ]; then
            mv "$src" "${src}.plain" || emit_error $EXIT_ERROR "移动源目录失败"
        else
            rmdir "$src" 2>/dev/null || true
        fi
    fi
    mkdir -p "$src"

    log_line "正式挂载: $cipher → $src"
    local aoo=""
    [ "$(get_setting "gocryptfs.allow_other" "false")" = "true" ] && aoo="-allow_other"
    if ! gocryptfs_with_password "$password" $aoo "$cipher" "$src"; then
        emit_error $EXIT_ERROR "最终挂载失败"
    fi

    local new_id
    new_id=$(yq -r '[.vaults // [] | .[].id] | max // 0' "$CONFIG_FILE" 2>/dev/null || echo 0)
    new_id=$((new_id + 1))
    yq -i ".vaults += [{\"id\": $new_id, \"name\": \"$name\", \"path\": \"$cipher\", \"mount_point\": \"$src\"}]" "$CONFIG_FILE"
    yq -i ".pending = [(.pending // [])[] | select(.source_dir != \"$src\")]" "$CONFIG_FILE" 2>/dev/null || true

    log_event "create" "$name" "success"
    emit_done "创建完成: $name"
}

# § 17. remove
cmd_remove() {
    require_config

    local name="" restore="" keep_cipher="" target="" yes=false
    while [ $# -gt 0 ]; do
        case "$1" in
            --restore)       restore=true; shift ;;
            --no-restore)    restore=false; shift ;;
            --delete-cipher) keep_cipher=false; shift ;;
            --keep-cipher)   keep_cipher=true; shift ;;
            --target)        target="$2"; shift 2 ;;
            --yes|-y)        yes=true; shift ;;
            -*) emit_error $EXIT_ERROR "未知选项: $1" ;;
            *) name="$1"; shift ;;
        esac
    done
    [ -z "$name" ] && emit_error $EXIT_ERROR "用法: remove <name> [options]"
    vault_exists "$name" || emit_error $EXIT_ERROR "卷不存在: $name"

    local path mp
    path=$(get_vault_field "$name" "path")
    mp=$(get_vault_field "$name" "mount_point")

    local cfg_restore cfg_keep
    cfg_restore=$(get_setting "remove.restore" "true")
    cfg_keep=$(get_setting "remove.direct_delete_cipher" "false")

    [ -z "$restore" ] && restore="$cfg_restore"
    [ "$cfg_restore" = "true" ] && [ "$restore" = "false" ] && restore=true

    local delete_cipher
    if [ -z "$keep_cipher" ]; then
        [ "$cfg_keep" = "true" ] && delete_cipher=true || delete_cipher=false
    elif [ "$keep_cipher" = "false" ]; then
        [ "$cfg_keep" = "true" ] && delete_cipher=true || delete_cipher=false
    else
        delete_cipher=false
    fi

    local target_mode target_path
    target_mode=$(get_setting "remove.restore_target_mode" "in_place")
    if [ -n "$target" ]; then
        target_path="$target"
    elif [ "$target_mode" = "in_place" ]; then
        target_path="$mp"
    elif [ "$target_mode" = "sibling" ]; then
        target_path="${mp}.plain"
    else
        target_path=$(get_setting "remove.restore_target_custom" "$mp")
    fi

    if [ "$yes" = false ] && [ -t 0 ]; then
        printf '警告：将删除加密卷并还原明文。\n' >&2
        printf '请输入 DELETE 确认: ' >&2
        local confirm_text
        read -r confirm_text
        if [ "$confirm_text" != "DELETE" ]; then
            emit_error $EXIT_STATE "用户取消"
        fi
    fi

    local password=""
    if ! is_mounted "$mp"; then
        if [ -t 0 ]; then password=$(read_password_interactive); else password=$(read_password_from_stdin); fi
    fi

    if [ "$DRY_RUN" = true ]; then
        log_line "[DRY-RUN] rsync $mp/ → $target_path/"
        log_line "[DRY-RUN] fusermount -u $mp"
        [ "$delete_cipher" = true ] && log_line "[DRY-RUN] rm -rf $path" || log_line "[DRY-RUN] 保留 $path"
        printf '@@DONE@@ remove (dry-run)\n' >&2
        return 0
    fi

    if ! is_mounted "$mp"; then
        log_line "自动挂载: $path → $mp"
        [ ! -d "$mp" ] && mkdir -p "$mp"
        unlock_dir "$mp"
        local aoo=""
        [ "$(get_setting "gocryptfs.allow_other" "false")" = "true" ] && aoo="-allow_other"
        if ! gocryptfs_with_password "$password" $aoo "$path" "$mp"; then
            if gocryptfs_output_is_password_error; then
                emit_error $EXIT_PASSWORD "密码错误"
            else
                emit_error $EXIT_PASSWORD "自动挂载失败"
            fi
        fi
    fi

    if [ "$restore" = true ]; then
        local migrate_to
        [ "$target_mode" = "in_place" ] && migrate_to="${mp}.restore_tmp" || migrate_to="$target_path"
        mkdir -p "$migrate_to"
        log_line "迁移: $mp/ → $migrate_to/"
        if ! rsync -a -h "$mp/" "$migrate_to/" 2>&1; then
            fusermount -u "$mp" 2>/dev/null || true
            lock_dir "$mp"
            log_event "remove" "$name" "failed" "rsync 迁移失败"
            emit_error $EXIT_ERROR "rsync 迁移失败"
        fi
    fi

    log_line "卸载: $mp"
    fusermount -u "$mp" 2>&1 || fusermount -u -z "$mp" 2>&1 || emit_error $EXIT_UMOUNT_FORCE "卸载失败"

    if [ "$restore" = true ] && [ "$target_mode" = "in_place" ]; then
        [ -d "$mp" ] && find "$mp" -mindepth 1 -delete 2>/dev/null || true
        if [ -d "${mp}.restore_tmp" ]; then
            cp -a "${mp}.restore_tmp/." "$mp/" 2>/dev/null || true
            rm -rf "${mp}.restore_tmp"
        fi
    fi

    if [ "$delete_cipher" = true ]; then
        log_line "删除加密后端: $path"
        rm -rf "$path" || emit_error $EXIT_ERROR "删除 $path 失败"
    else
        log_line "保留加密后端: $path"
    fi

    yq -i "del(.vaults[] | select(.name == \"$name\"))" "$CONFIG_FILE"

    log_event "remove" "$name" "success"
    emit_done "删除完成: $name"
}

# § 18. log
cmd_log() {
    local limit=20 src="" action="" result="" since="" follow=false

    while [ $# -gt 0 ]; do
        case "$1" in
            --limit)  limit="$2"; shift 2 ;;
            --src)    src="$2"; shift 2 ;;
            --action) action="$2"; shift 2 ;;
            --result) result="$2"; shift 2 ;;
            --since)  since="$2"; shift 2 ;;
            --follow) follow=true; shift ;;
            -*) emit_error $EXIT_ERROR "未知选项: $1" ;;
            *) shift ;;
        esac
    done

    if [ ! -f "$LOG_FILE" ]; then
        echo "（暂无日志: $LOG_FILE）"
        return 0
    fi

    # 构造 jq 过滤器
    local filter="."
    [ -n "$src" ]    && filter="$filter | select(.src == \"$src\")"
    [ -n "$action" ] && filter="$filter | select(.action == \"$action\")"
    [ -n "$result" ] && filter="$filter | select(.result == \"$result\")"
    [ -n "$since" ]  && filter="$filter | select(.ts >= \"$since\")"

    if [ "$follow" = true ]; then
        tail -f "$LOG_FILE" | jq -c --unbuffered "$filter"
        return 0
    fi

    if [ "$JSON_MODE" = true ]; then
        tail -n "$limit" "$LOG_FILE" | jq -c "$filter"
        return 0
    fi

    # 人类可读（带颜色，仅 TTY 时启用）
    local green="" red="" yellow="" cyan="" reset=""
    if [ -t 1 ]; then
        green=$(tput setaf 2 2>/dev/null || echo "")
        red=$(tput setaf 1 2>/dev/null || echo "")
        yellow=$(tput setaf 3 2>/dev/null || echo "")
        cyan=$(tput setaf 6 2>/dev/null || echo "")
        reset=$(tput sgr0 2>/dev/null || echo "")
    fi

    tail -n "$limit" "$LOG_FILE" \
        | jq -r "$filter | [.ts // \"\", .src // \"\", .action // \"\", .target // \"\", .result // \"\", .detail // \"\"] | @tsv" \
        | while IFS=$'\t' read -r ts src action target result detail; do
            local t
            if [ ${#ts} -ge 19 ]; then
                t="${ts:11:8}"
            else
                t="$ts"
            fi
            local c=""
            case "$result" in
                success)   c="$green" ;;
                failed)    c="$red" ;;
                started)   c="$cyan" ;;
                cancelled) c="$yellow" ;;
            esac
            printf '%s%s  %-4s  %-12s  %-22s  %-10s  %s%s\n' \
                "$c" "$t" "$src" "$action" "$target" "$result" "$detail" "$reset"
        done
}