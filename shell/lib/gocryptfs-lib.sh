#!/bin/bash
# gocryptfs-lib.sh: gocryptfs-cli 的全部库函数（轮询挂载点版）
# 面向用户的文案走 lib/i18n.sh 的消息表（t/te），支持 zh-CN / en-US
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

# § 0. i18n（消息表与语言解析在 lib/i18n.sh；缺失时降级为直接输出键名）
_I18N_DIR="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ -f "$_I18N_DIR/i18n.sh" ]; then
    # shellcheck source=/dev/null
    source "$_I18N_DIR/i18n.sh"
fi
if ! declare -F t >/dev/null 2>&1; then
    I18N_LANG="zh-CN"
    t()  { printf '%s' "${1:-}"; }
    te() { printf '%s\n' "${1:-}"; }
    i18n_init() { :; }
    i18n_set_lang() { return 1; }
fi

# § 0.1 函数索引
if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
    i18n_init
    te lib.self_index
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
    i18n_init
    t cli.help
    printf '\n'
}

check_deps() {
    local missing=()
    for cmd in gocryptfs fusermount rsync yq jq mountpoint; do
        command -v "$cmd" >/dev/null 2>&1 || missing+=("$cmd")
    done
    if [ ${#missing[@]} -gt 0 ]; then
        te deps.missing "${missing[*]}" >&2
        return 1
    fi
    te deps.ok >&2
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
    [ -f "$CONFIG_FILE" ] || emit_error $EXIT_CONFIG "$(t error.config_missing "$CONFIG_FILE")"
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
    [ -d "$1" ] && chmod "$mode" "$1" && log_line "$(t log.dir_locked "$mode" "$1")"
}
unlock_dir() {
    local mode; mode=$(get_setting "unlock_mode" "755")
    [ "$DRY_RUN" = true ] && { log_line "[DRY-RUN] chmod $mode $1"; return 0; }
    [ -d "$1" ] && chmod "$mode" "$1" && log_line "$(t log.dir_unlocked "$mode" "$1")"
}

# § 8. 密码
read_password_from_stdin() {
    local p; IFS= read -r p || true
    [ -z "$p" ] && emit_error $EXIT_PASSWORD "$(t error.password_empty)"
    echo "$p"
}
read_password_interactive() {
    local p1 p2
    while true; do
        t prompt.password >&2; IFS= read -rs p1; printf '\n' >&2
        t prompt.password_again >&2; IFS= read -rs p2; printf '\n' >&2
        [ -z "$p1" ] && { te error.password_empty_retry >&2; continue; }
        [ "$p1" != "$p2" ] && { te error.password_mismatch >&2; continue; }
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
        log_error "$(t error.tmp_password_file)"
        return 1
    }
    chmod 600 "$passfile"
    printf '%s' "$password" > "$passfile"

    output_file=$(mktemp -t gocryptfs-out.XXXXXX) || {
        rm -f "$passfile"
        log_error "$(t error.tmp_output_file)"
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
            GOCRYPTFS_LAST_OUTPUT="$(t error.mount_timeout)"
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
    [ -e "$src" ] || { log_error "$(t error.source_missing "$src")"; return 1; }
    if [ ! -e "$tdir" ]; then
        local p="$tdir"
        while [ ! -e "$p" ] && [ "$p" != "/" ]; do p="$(dirname "$p")"; done
        tdir="$p"
    fi
    local ssz tfree req
    ssz=$(du -sb "$src" 2>/dev/null | awk '{print $1}')
    tfree=$(df -B1 --output=avail "$tdir" 2>/dev/null | tail -1 | tr -d ' ')
    if [ -z "$ssz" ] || [ -z "$tfree" ]; then
        log_error "$(t error.capacity_failed)"
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
    flock -n 200 || emit_error $EXIT_CONFIG "$(t error.already_running)"
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
        te error.list_empty
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
    [ -z "$name" ] && emit_error $EXIT_ERROR "$(t usage.info)"
    vault_exists "$name" || emit_error $EXIT_ERROR "$(t error.vault_missing "$name")"
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
        te info.id "$id"; te info.name "$name"
        te info.cipher_path "$path"; te info.mount_point "$mp"
        te info.mounted "$mounted"; te info.locked "$locked"
        te info.mode "$mode"; te info.valid "$valid"
    fi
}

# § 13. ls / tree
cmd_ls() {
    require_config
    local name="$1"
    [ -z "$name" ] && emit_error $EXIT_ERROR "$(t usage.ls)"
    vault_exists "$name" || emit_error $EXIT_ERROR "$(t error.vault_missing "$name")"
    local mp; mp=$(get_vault_field "$name" "mount_point")
    is_mounted "$mp" || emit_error $EXIT_STATE "$(t error.not_mounted "$name")"
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
    [ -z "$name" ] && emit_error $EXIT_ERROR "$(t usage.tree)"
    vault_exists "$name" || emit_error $EXIT_ERROR "$(t error.vault_missing "$name")"
    local mp; mp=$(get_vault_field "$name" "mount_point")
    is_mounted "$mp" || emit_error $EXIT_STATE "$(t error.not_mounted "$name")"
    command -v tree >/dev/null 2>&1 || emit_error $EXIT_ERROR "$(t error.tree_missing)"
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
    [ -z "$name" ] && emit_error $EXIT_ERROR "$(t usage.mount)"
    vault_exists "$name" || emit_error $EXIT_ERROR "$(t error.vault_missing "$name")"

    local path mp
    path=$(get_vault_field "$name" "path")
    mp=$(get_vault_field "$name" "mount_point")

    is_mounted "$mp" && emit_error $EXIT_STATE "$(t error.already_mounted "$name")"
    [ -d "$path" ] || emit_error $EXIT_ERROR "$(t error.cipher_dir_missing "$path")"
    [ -f "$path/gocryptfs.conf" ] || \
        emit_error $EXIT_ERROR "$(t error.invalid_vault "$name")"

    [ ! -d "$mp" ] && { log_line "$(t log.mountpoint_created "$mp")"; [ "$DRY_RUN" = false ] && mkdir -p "$mp"; }

    local allow_nonempty
    allow_nonempty=$(get_effective_setting "$name" "gocryptfs.nonempty" "false")
    if [ -n "$(ls -A "$mp" 2>/dev/null)" ] && [ "$allow_nonempty" != "true" ]; then
        emit_error $EXIT_MOUNTPOINT "$(t error.mountpoint_not_empty "$mp")"
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
    log_line "$(t log.exec "gocryptfs ${opts[*]} $path $mp")"

    if ! gocryptfs_with_password "$password" "${opts[@]}" "$path" "$mp"; then
        lock_dir "$mp"
        log_event "mount" "$name" "failed"
        if gocryptfs_output_is_password_error; then
            emit_error $EXIT_PASSWORD "$(t error.wrong_password)"
        else
            emit_error $EXIT_ERROR "$(t error.mount_failed "$name")"
        fi
    fi

    if ! is_mounted "$mp"; then
        lock_dir "$mp"
        log_event "mount" "$name" "failed"
        emit_error $EXIT_ERROR "$(t error.mount_failed_notmounted "$name")"
    fi

    log_event "mount" "$name" "success"
    emit_done "$(t done.mounted "$name")"
}

# § 15. umount
cmd_umount() {
    require_config
    local name="" force=false
    while [ $# -gt 0 ]; do
        case "$1" in
            --force) force=true; shift ;;
            -*) emit_error $EXIT_ERROR "$(t error.unknown_option "$1")" ;;
            *) name="$1"; shift ;;
        esac
    done
    [ -z "$name" ] && emit_error $EXIT_ERROR "$(t usage.umount)"
    vault_exists "$name" || emit_error $EXIT_ERROR "$(t error.vault_missing "$name")"

    local mp; mp=$(get_vault_field "$name" "mount_point")
    is_mounted "$mp" || emit_error $EXIT_STATE "$(t error.not_mounted "$name")"

    if [ "$DRY_RUN" = true ]; then
        log_line "[DRY-RUN] fusermount -u $mp"
        printf '@@DONE@@ umount (dry-run)\n' >&2
        return 0
    fi

    log_line "$(t log.exec "fusermount -u $mp")"
    local rc=0
    fusermount -u "$mp" 2>&1 || rc=$?

    if [ "$rc" -ne 0 ]; then
        if [ "$force" = true ]; then
            fusermount -u -z "$mp" 2>&1 || {
                log_event "umount" "$name" "failed" "$(t log.force_umount_failed)"
                emit_error $EXIT_UMOUNT_FORCE "$(t error.force_umount_failed "$name")"
            }
        else
            log_event "umount" "$name" "failed"
            emit_error $EXIT_UMOUNT "$(t error.umount_failed "$name")"
        fi
    fi

    lock_dir "$mp"
    log_event "umount" "$name" "success"
    emit_done "$(t done.umounted "$name")"
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
            -*) emit_error $EXIT_ERROR "$(t error.unknown_option "$1")" ;;
            *) [ -z "$src" ] && src="$1" || emit_error $EXIT_ERROR "$(t error.extra_arg "$1")"; shift ;;
        esac
    done

    [ -z "$src" ]  && emit_error $EXIT_ERROR "$(t error.source_dir_missing)"
    [ -z "$name" ] && emit_error $EXIT_ERROR "$(t error.name_missing)"
    [ -d "$src" ]  || emit_error $EXIT_ERROR "$(t error.source_dir_not_exist "$src")"
    src="${src%/}"

    local existing
    existing=$(yq -r ".vaults[] | select(.mount_point == \"$src\") | .name" "$CONFIG_FILE" 2>/dev/null | head -1)
    if [ -n "$existing" ]; then
        emit_error $EXIT_STATE "$(t error.source_in_use "$src" "$existing")"
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
        [ $? -eq 5 ] && emit_error $EXIT_NOSPACE "$(t error.no_space)"
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
        log_line "$(t log.create_cipher "$cipher")"
        mkdir -p "$cipher" || emit_error $EXIT_ERROR "$(t error.cannot_create_dir "$cipher")"
        if ! gocryptfs_with_password "$password" -init "$cipher"; then
            rmdir "$cipher" 2>/dev/null || true
            emit_error $EXIT_ERROR "$(t error.init_failed)"
        fi
    fi

    mkdir -p "$tmp_mount"
    is_mounted "$tmp_mount" && fusermount -u "$tmp_mount" 2>/dev/null || true

    log_line "$(t log.mount_tmp "$tmp_mount")"
    if ! gocryptfs_with_password "$password" "$cipher" "$tmp_mount"; then
        if gocryptfs_output_is_password_error; then
            emit_error $EXIT_PASSWORD "$(t error.wrong_password)"
        else
            emit_error $EXIT_ERROR "$(t error.tmp_mount_failed)"
        fi
    fi

    log_line "$(t log.migrate_start "$src" "$tmp_mount")"
    local rsync_opts=("-a" "-h")
    [ "$keep_source" = "true" ] || rsync_opts+=("--remove-source-files")

    if ! rsync "${rsync_opts[@]}" "$src/" "$tmp_mount/" 2>&1; then
        fusermount -u "$tmp_mount" 2>/dev/null || true
        log_event "create" "$name" "failed" "$(t error.rsync_failed)"
        emit_error $EXIT_ERROR "$(t error.rsync_failed)"
    fi

    [ "$keep_source" = "false" ] && find "$src" -mindepth 1 -type d -empty -delete 2>/dev/null || true

    if [ "$yes" = false ] && [ "$JSON_MODE" != true ] && [ -t 0 ]; then
        confirm "$(t confirm.replace_mountpoint "$src")" "n" || {
            fusermount -u "$tmp_mount" 2>/dev/null || true
            emit_error $EXIT_STATE "$(t error.user_abort)"
        }
    fi

    fusermount -u "$tmp_mount" || emit_error $EXIT_UMOUNT "$(t error.umount_tmp_failed)"
    rmdir "$tmp_mount" 2>/dev/null || true

    if [ -d "$src" ]; then
        if [ -n "$(ls -A "$src" 2>/dev/null)" ]; then
            mv "$src" "${src}.plain" || emit_error $EXIT_ERROR "$(t error.move_source_failed)"
        else
            rmdir "$src" 2>/dev/null || true
        fi
    fi
    mkdir -p "$src"

    log_line "$(t log.mount_final "$cipher" "$src")"
    local aoo=""
    [ "$(get_setting "gocryptfs.allow_other" "false")" = "true" ] && aoo="-allow_other"
    if ! gocryptfs_with_password "$password" $aoo "$cipher" "$src"; then
        emit_error $EXIT_ERROR "$(t error.final_mount_failed)"
    fi

    local new_id
    new_id=$(yq -r '[.vaults // [] | .[].id] | max // 0' "$CONFIG_FILE" 2>/dev/null || echo 0)
    new_id=$((new_id + 1))
    yq -i ".vaults += [{\"id\": $new_id, \"name\": \"$name\", \"path\": \"$cipher\", \"mount_point\": \"$src\"}]" "$CONFIG_FILE"
    yq -i ".pending = [(.pending // [])[] | select(.source_dir != \"$src\")]" "$CONFIG_FILE" 2>/dev/null || true

    log_event "create" "$name" "success"
    emit_done "$(t done.created "$name")"
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
            -*) emit_error $EXIT_ERROR "$(t error.unknown_option "$1")" ;;
            *) name="$1"; shift ;;
        esac
    done
    [ -z "$name" ] && emit_error $EXIT_ERROR "$(t usage.remove)"
    vault_exists "$name" || emit_error $EXIT_ERROR "$(t error.vault_missing "$name")"

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
        te prompt.delete_warning >&2
        t prompt.delete_confirm >&2
        local confirm_text
        read -r confirm_text
        if [ "$confirm_text" != "DELETE" ]; then
            emit_error $EXIT_STATE "$(t error.user_abort)"
        fi
    fi

    local password=""
    if ! is_mounted "$mp"; then
        if [ -t 0 ]; then password=$(read_password_interactive); else password=$(read_password_from_stdin); fi
    fi

    if [ "$DRY_RUN" = true ]; then
        log_line "[DRY-RUN] rsync $mp/ → $target_path/"
        log_line "[DRY-RUN] fusermount -u $mp"
        [ "$delete_cipher" = true ] && log_line "[DRY-RUN] rm -rf $path" || log_line "$(t log.dry_run_keep "$path")"
        printf '@@DONE@@ remove (dry-run)\n' >&2
        return 0
    fi

    if ! is_mounted "$mp"; then
        log_line "$(t log.auto_mount "$path" "$mp")"
        [ ! -d "$mp" ] && mkdir -p "$mp"
        unlock_dir "$mp"
        local aoo=""
        [ "$(get_setting "gocryptfs.allow_other" "false")" = "true" ] && aoo="-allow_other"
        if ! gocryptfs_with_password "$password" $aoo "$path" "$mp"; then
            if gocryptfs_output_is_password_error; then
                emit_error $EXIT_PASSWORD "$(t error.wrong_password)"
            else
                emit_error $EXIT_PASSWORD "$(t error.auto_mount_failed)"
            fi
        fi
    fi

    if [ "$restore" = true ]; then
        local migrate_to
        [ "$target_mode" = "in_place" ] && migrate_to="${mp}.restore_tmp" || migrate_to="$target_path"
        mkdir -p "$migrate_to"
        log_line "$(t log.migrate "$mp/" "$migrate_to/")"
        if ! rsync -a -h "$mp/" "$migrate_to/" 2>&1; then
            fusermount -u "$mp" 2>/dev/null || true
            lock_dir "$mp"
            log_event "remove" "$name" "failed" "$(t error.rsync_failed)"
            emit_error $EXIT_ERROR "$(t error.rsync_failed)"
        fi
    fi

    log_line "$(t log.umount "$mp")"
    fusermount -u "$mp" 2>&1 || fusermount -u -z "$mp" 2>&1 || emit_error $EXIT_UMOUNT_FORCE "$(t error.umount_failed_plain)"

    if [ "$restore" = true ] && [ "$target_mode" = "in_place" ]; then
        [ -d "$mp" ] && find "$mp" -mindepth 1 -delete 2>/dev/null || true
        if [ -d "${mp}.restore_tmp" ]; then
            cp -a "${mp}.restore_tmp/." "$mp/" 2>/dev/null || true
            rm -rf "${mp}.restore_tmp"
        fi
    fi

    if [ "$delete_cipher" = true ]; then
        log_line "$(t log.delete_cipher "$path")"
        rm -rf "$path" || emit_error $EXIT_ERROR "$(t error.delete_failed "$path")"
    else
        log_line "$(t log.keep_cipher "$path")"
    fi

    yq -i "del(.vaults[] | select(.name == \"$name\"))" "$CONFIG_FILE"

    log_event "remove" "$name" "success"
    emit_done "$(t done.removed "$name")"
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
            -*) emit_error $EXIT_ERROR "$(t error.unknown_option "$1")" ;;
            *) shift ;;
        esac
    done

    if [ ! -f "$LOG_FILE" ]; then
        te log.none "$LOG_FILE"
        return 0
    fi

    # 构造 jq 过滤器
    local filter="."
    [ -n "$src" ]    && filter="$filter | select(.src == \"$src\")"
    [ -n "$action" ] && filter="$filter | select(.action == \"$action\")"
    [ -n "$result" ] && filter="$filter | select(.result == \"$result\")"
    [ -n "$since" ]  && filter="$filter | select(.ts >= \"$since\")"

    # 只喂 JSON 行给 jq：历史日志里可能混有旧版本写入的纯文本行
    if [ "$follow" = true ]; then
        tail -f "$LOG_FILE" | grep --line-buffered '^{' | jq -c --unbuffered "$filter"
        return 0
    fi

    if [ "$JSON_MODE" = true ]; then
        grep '^{' "$LOG_FILE" | tail -n "$limit" | jq -c "$filter"
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

    grep '^{' "$LOG_FILE" | tail -n "$limit" \
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