#!/bin/bash
# gocryptfs-lib.sh: gocryptfs-cli 的全部库函数
#
# 设计说明：
#   - 本文件不包含 shebang 依赖的 set -euo pipefail（由调用者设置）
#   - 直接执行本文件可查看函数索引，便于调试
#   - 调用者：source "$LIB_DIR/gocryptfs-lib.sh"
#
# 目录：
#   § 1. 常量与退出码
#   § 2. 帮助与依赖
#   § 3. 日志
#   § 4. 协议输出
#   § 5. 配置读写
#   § 6. 挂载状态
#   § 7. 权限锁定
#   § 8. 密码
#   § 9. 容量检查
#   § 10. 文件锁
#   § 11. 历史
#   § 12. 用户确认
#   § 13. list / info
#   § 14. ls / tree

# ============================================================
# § 0. 调试守卫：直接执行时打印函数索引
# ============================================================
if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
    echo "gocryptfs-lib.sh — 函数索引"
    echo ""
    echo "§ 2. help"
    echo "    show_help / check_deps"
    echo "§ 3-4. log / protocol"
    echo "    log_line / log_error / emit_progress / emit_check"
    echo "    emit_done / emit_error / emit_json_ok"
    echo "§ 5. config"
    echo "    require_config / get_vault_field / vault_exists"
    echo "    get_setting / get_effective_setting / save_config_content"
    echo "§ 6-7. mount / lock"
    echo "    is_mounted / dir_is_empty / lock_dir / unlock_dir"
    echo "§ 8. password"
    echo "    read_password_from_stdin / read_password_interactive"
    echo "§ 9-11. space / lock / history"
    echo "    check_space / acquire_lock / log_history"
    echo "§ 12. confirm"
    echo "    confirm"
    echo "§ 13. list / info"
    echo "    cmd_list / cmd_info / _list_json / _list_table"
    echo "§ 14. dir"
    echo "    cmd_ls / cmd_tree"
    exit 0
fi

# ============================================================
# § 1. 常量与退出码
# ============================================================
LOG_FILE="${LOG_FILE:-/tmp/gocryptfs-tui.log}"
HISTORY_FILE="${HISTORY_FILE:-$HOME/.local/share/gocryptfs-tui/history.jsonl}"
LOCK_FILE="${LOCK_FILE:-/tmp/gocryptfs-tui.lock}"

EXIT_OK=0
EXIT_ERROR=1
EXIT_PASSWORD=2
EXIT_STATE=3
EXIT_MOUNTPOINT=4
EXIT_NOSPACE=5
EXIT_UMOUNT=6
EXIT_UMOUNT_FORCE=7
EXIT_CONFIG=8

# ============================================================
# § 2. 帮助与依赖
# ============================================================
show_help() {
    cat <<'EOF'
gocryptfs-cli - gocryptfs-tui 的 Shell 后端

用法: gocryptfs-cli [-c <config>] <command> [options]

全局选项:
  -c, --config <path>    指定配置文件（默认 ~/.config/gocryptfs-tui/config.yaml）
  --json                 以 JSON 格式输出（TUI 使用）
  -v, --verbose          详细输出
  --dry-run              预览模式，不实际修改
  --check-deps           检查依赖是否齐全

命令:
  list                                列出所有卷
  info <name>                         显示卷详情
  mount <name>                        挂载（解密）
  umount <name> [--force]             卸载（--force 使用 lazy unmount）
  ls <name>                           列出明文目录
  tree <name>                         树状显示明文目录
  create <src> --name <n> [options]   创建加密
  remove <name> [options]             删除加密
  config                              显示配置路径
  edit                                用 $EDITOR 打开配置
  check-deps                          检查依赖

create 选项:
  --name <n>              卷名（必需）
  --cipher <path>         密文路径（默认 sibling 目录）
  --keep-source           保留源文件
  --no-keep-source        删除源文件
  --yes                   跳过二次确认

remove 选项:
  --restore               还原明文（默认）
  --no-restore            不还原
  --delete-cipher         删除加密后端
  --keep-cipher           保留加密后端
  --target <path>         还原目标路径
  --yes                   跳过二次确认

退出码:
  0  成功         1  通用错误     2  密码错误     3  状态错误
  4  挂载点问题   5  空间不足     6  卸载失败     7  强制卸载失败
  8  配置错误

示例:
  gocryptfs-cli list
  gocryptfs-cli mount Work < /tmp/pass
  gocryptfs-cli create /srv/photos --name photos --dry-run
  gocryptfs-cli -c /tmp/test.yaml list --json
EOF
}

check_deps() {
    local missing=()
    local required=(gocryptfs fusermount rsync yq jq mountpoint)
    for cmd in "${required[@]}"; do
        if ! command -v "$cmd" >/dev/null 2>&1; then
            missing+=("$cmd")
        fi
    done
    if [ ${#missing[@]} -gt 0 ]; then
        echo "[!] 缺少依赖: ${missing[*]}" >&2
        echo "    Debian/Ubuntu: apt install gocryptfs rsync jq" >&2
        echo "    yq:  https://github.com/mikefarah/yq" >&2
        return 1
    fi
    echo "[✔] 依赖齐全" >&2
    return 0
}

# ============================================================
# § 3. 日志
# ============================================================
log_line() {
    local msg="$1"
    printf '[%s] %s\n' "$(date '+%H:%M:%S')" "$msg" >&2
    {
        printf '[%s] %s\n' "$(date -Iseconds)" "$msg"
    } >> "$LOG_FILE" 2>/dev/null || true
}

log_error() {
    local msg="$1"
    printf '[!] %s\n' "$msg" >&2
    {
        printf '[%s] ERROR: %s\n' "$(date -Iseconds)" "$msg"
    } >> "$LOG_FILE" 2>/dev/null || true
}

# ============================================================
# § 4. 协议输出
# 约定：stdout = 用户可读输出；stderr = 日志 + 协议行(@@xxx@@)
# ============================================================
emit_progress() {
    printf '@@PROGRESS@@ %s %s %s\n' "$1" "$2" "$3" >&2
}

emit_check() {
    printf '@@CHECK@@ %s %s\n' "$1" "$2" >&2
}

emit_done() {
    local msg="$1"
    if [ "$JSON_MODE" != true ]; then
        echo "[✔] $msg" >&2
    fi
    printf '@@DONE@@ %s\n' "$msg" >&2
}

emit_error() {
    local code="$1"
    local msg="$2"
    if [ "$JSON_MODE" = true ]; then
        jq -cn --argjson code "$code" --arg msg "$msg" \
            '{status:"error", code:$code, message:$msg}' >&2
    else
        echo "[✗] $msg" >&2
    fi
    printf '@@ERROR@@ %s %s\n' "$code" "$msg" >&2
    exit "$code"
}

emit_json_ok() {
    if [ "$JSON_MODE" = true ]; then
        echo "$1"
    fi
    printf '@@DONE@@ ok\n' >&2
}

# ============================================================
# § 5. 配置读写
# ============================================================
require_config() {
    if [ ! -f "$CONFIG_FILE" ]; then
        emit_error $EXIT_CONFIG "配置文件不存在: $CONFIG_FILE"
    fi
}

get_vault_field() {
    local name="$1" field="$2"
    yq -r ".vaults[] | select(.name == \"$name\") | .$field // \"\"" "$CONFIG_FILE"
}

vault_exists() {
    local name="$1" n
    n=$(yq -r ".vaults[] | select(.name == \"$name\") | .name" "$CONFIG_FILE" | head -1)
    [ "$n" = "$name" ]
}

get_setting() {
    local path="$1" default="${2:-}" val
    val=$(yq -r ".settings.$path // \"\"" "$CONFIG_FILE")
    if [ -z "$val" ] || [ "$val" = "null" ]; then
        echo "$default"
    else
        echo "$val"
    fi
}

get_effective_setting() {
    local name="$1" path="$2" default="${3:-}" override
    override=$(yq -r ".vaults[] | select(.name == \"$name\") | .overrides.$path // \"\"" "$CONFIG_FILE")
    if [ -n "$override" ] && [ "$override" != "null" ]; then
        echo "$override"
    else
        get_setting "$path" "$default"
    fi
}

save_config_content() {
    local content="$1"
    local tmp="${CONFIG_FILE}.tmp.$$"
    printf '%s' "$content" > "$tmp"
    sync "$tmp" 2>/dev/null || true
    mv "$tmp" "$CONFIG_FILE"
}

# ============================================================
# § 6. 挂载状态
# ============================================================
is_mounted() {
    mountpoint -q "$1" 2>/dev/null
}

dir_is_empty() {
    [ -z "$(ls -A "$1" 2>/dev/null)" ]
}

# ============================================================
# § 7. 权限锁定
# ============================================================
lock_dir() {
    local dir="$1" mode
    mode=$(get_setting "lock_mode" "555")
    if [ "$DRY_RUN" = true ]; then
        log_line "[DRY-RUN] chmod $mode $dir"
        return 0
    fi
    if [ -d "$dir" ]; then
        chmod "$mode" "$dir"
        log_line "已锁定 ($mode): $dir"
    fi
}

unlock_dir() {
    local dir="$1" mode
    mode=$(get_setting "unlock_mode" "755")
    if [ "$DRY_RUN" = true ]; then
        log_line "[DRY-RUN] chmod $mode $dir"
        return 0
    fi
    if [ -d "$dir" ]; then
        chmod "$mode" "$dir"
        log_line "已解锁 ($mode): $dir"
    fi
}

# ============================================================
# § 8. 密码
# ============================================================
read_password_from_stdin() {
    local password
    IFS= read -r password || true
    if [ -z "$password" ]; then
        emit_error $EXIT_PASSWORD "密码为空"
    fi
    echo "$password"
}

read_password_interactive() {
    local p1 p2
    while true; do
        printf '请输入密码: ' >&2
        IFS= read -rs p1
        printf '\n' >&2
        printf '再次输入: ' >&2
        IFS= read -rs p2
        printf '\n' >&2
        if [ -z "$p1" ]; then
            echo "密码不能为空" >&2
            continue
        fi
        if [ "$p1" != "$p2" ]; then
            echo "两次密码不一致" >&2
            continue
        fi
        break
    done
    echo "$p1"
}

# ============================================================
# § 9. 容量检查
# 返回：0=充足  5=不足  1=无法检查
# ============================================================
check_space() {
    local src="$1" target_dir="$2"

    if [ ! -e "$src" ]; then
        log_error "源路径不存在: $src"
        return 1
    fi
    if [ ! -e "$target_dir" ]; then
        local p="$target_dir"
        while [ ! -e "$p" ] && [ "$p" != "/" ]; do
            p="$(dirname "$p")"
        done
        target_dir="$p"
    fi

    local src_size target_free
    src_size=$(du -sb "$src" 2>/dev/null | awk '{print $1}')
    target_free=$(df -B1 --output=avail "$target_dir" 2>/dev/null | tail -1 | tr -d ' ')

    if [ -z "$src_size" ] || [ -z "$target_free" ]; then
        log_error "无法计算容量"
        return 1
    fi

    local required=$(( src_size * 111 / 100 ))

    emit_check "src_size" "$src_size"
    emit_check "target_free" "$target_free"
    emit_check "required" "$required"

    if [ "$target_free" -lt "$required" ]; then
        return 5
    fi
    return 0
}

# ============================================================
# § 10. 文件锁
# ============================================================
acquire_lock() {
    exec 200>"$LOCK_FILE"
    if ! flock -n 200; then
        emit_error $EXIT_CONFIG "另一个 gocryptfs-tui 进程正在运行（锁: $LOCK_FILE）"
    fi
}

# ============================================================
# § 11. 历史
# ============================================================
log_history() {
    mkdir -p "$(dirname "$HISTORY_FILE")"
    jq -cn \
        --arg ts "$(date -Iseconds)" \
        --arg action "$1" \
        --arg name "$2" \
        --arg status "$3" \
        --arg detail "${4:-}" \
        '{ts:$ts, action:$action, name:$name, status:$status, detail:$detail}' \
        >> "$HISTORY_FILE" 2>/dev/null || true
}

# ============================================================
# § 12. 用户确认
# ============================================================
confirm() {
    if [ "$JSON_MODE" = true ]; then
        return 0
    fi
    local prompt="$1" default="${2:-n}" hint="[y/N]"
    [ "$default" = "y" ] && hint="[Y/n]"
    printf '%s %s ' "$prompt" "$hint" >&2
    local reply
    read -r reply
    reply="${reply:-$default}"
    [[ "$reply" =~ ^[Yy]$ ]]
}

# ============================================================
# § 13. list / info
# ============================================================
cmd_list() {
    require_config
    if [ "$JSON_MODE" = true ]; then
        _list_json
    else
        _list_table
    fi
}

_list_json() {
    local vaults_json="[]" count
    count=$(yq -r '.vaults // [] | length' "$CONFIG_FILE" 2>/dev/null || echo 0)

    for ((i=0; i<count; i++)); do
        local id name path mp mounted locked
        id=$(yq -r ".vaults[$i].id // 0" "$CONFIG_FILE")
        name=$(yq -r ".vaults[$i].name // \"\"" "$CONFIG_FILE")
        path=$(yq -r ".vaults[$i].path // \"\"" "$CONFIG_FILE")
        mp=$(yq -r ".vaults[$i].mount_point // \"\"" "$CONFIG_FILE")

        if is_mounted "$mp"; then mounted=true; else mounted=false; fi

        locked=false
        if [ "$mounted" = false ] && [ -d "$mp" ]; then
            local mode
            mode=$(stat -c '%a' "$mp" 2>/dev/null)
            mode="${mode: -3}"
            [ "$mode" = "555" ] && locked=true
        fi

        vaults_json=$(jq -cn \
            --argjson prev "$vaults_json" \
            --argjson id "$id" \
            --arg name "$name" \
            --arg path "$path" \
            --arg mount_point "$mp" \
            --argjson mounted "$mounted" \
            --argjson locked "$locked" \
            '$prev + [{id:$id, name:$name, path:$path, mount_point:$mount_point, mounted:$mounted, locked:$locked}]')
    done

    jq -cn --argjson vaults "$vaults_json" '{status:"ok", data:{vaults:$vaults}}'
    printf '@@DONE@@ list\n' >&2
}

_list_table() {
    local count
    count=$(yq -r '.vaults // [] | length' "$CONFIG_FILE" 2>/dev/null || echo 0)

    if [ "$count" -eq 0 ]; then
        echo "(配置中没有任何卷)"
        echo "编辑 $CONFIG_FILE，在 vaults 段添加卷定义。"
        echo "参考 examples/config.yaml.example 的格式。"
        return 0
    fi

    printf '%-20s %-12s %-10s %s\n' "NAME" "STATUS" "LOCKED" "MOUNT"
    printf '%-20s %-12s %-10s %s\n' "----" "------" "------" "-----"

    for ((i=0; i<count; i++)); do
        local name mp status locked_str
        name=$(yq -r ".vaults[$i].name // \"\"" "$CONFIG_FILE")
        mp=$(yq -r ".vaults[$i].mount_point // \"\"" "$CONFIG_FILE")

        if is_mounted "$mp"; then
            status="mounted"; locked_str="-"
        else
            status="unmounted"
            if [ -d "$mp" ]; then
                local mode
                mode=$(stat -c '%a' "$mp" 2>/dev/null)
                mode="${mode: -3}"
                [ "$mode" = "555" ] && locked_str="yes" || locked_str="no"
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

    local id path mp mounted locked mode
    id=$(get_vault_field "$name" "id")
    path=$(get_vault_field "$name" "path")
    mp=$(get_vault_field "$name" "mount_point")

    if is_mounted "$mp"; then mounted=true; else mounted=false; fi

    mode="-"; locked=false
    if [ -d "$mp" ]; then
        mode=$(stat -c '%a' "$mp" 2>/dev/null)
        local last3="${mode: -3}"
        [ "$last3" = "555" ] && locked=true
    fi

    if [ "$JSON_MODE" = true ]; then
        jq -cn \
            --argjson id "$id" \
            --arg name "$name" \
            --arg path "$path" \
            --arg mount_point "$mp" \
            --argjson mounted "$mounted" \
            --argjson locked "$locked" \
            --arg mode "$mode" \
            '{status:"ok", data:{id:$id, name:$name, path:$path, mount_point:$mount_point, mounted:$mounted, locked:$locked, mode:$mode}}'
        printf '@@DONE@@ info\n' >&2
    else
        echo "ID:       $id"
        echo "名称:     $name"
        echo "加密路径: $path"
        echo "挂载点:   $mp"
        echo "已挂载:   $mounted"
        echo "已锁定:   $locked"
        echo "权限:     $mode"
    fi
}

# ============================================================
# § 14. ls / tree
# ============================================================
cmd_ls() {
    require_config
    local name="$1"
    [ -z "$name" ] && emit_error $EXIT_ERROR "用法: ls <name>"
    vault_exists "$name" || emit_error $EXIT_ERROR "卷不存在: $name"

    local mp
    mp=$(get_vault_field "$name" "mount_point")
    is_mounted "$mp" || emit_error $EXIT_STATE "卷未挂载: $name"

    if [ "$JSON_MODE" = true ]; then
        local lines_json="[]"
        while IFS= read -r line; do
            lines_json=$(jq -cn --argjson prev "$lines_json" --arg line "$line" \
                '$prev + [$line]')
        done < <(ls -la "$mp" 2>/dev/null)
        jq -cn --argjson lines "$lines_json" --arg path "$mp" \
            '{status:"ok", data:{path:$path, lines:$lines}}'
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

    local mp
    mp=$(get_vault_field "$name" "mount_point")
    is_mounted "$mp" || emit_error $EXIT_STATE "卷未挂载: $name"

    if ! command -v tree >/dev/null 2>&1; then
        emit_error $EXIT_ERROR "tree 命令未安装"
    fi

    if [ "$JSON_MODE" = true ]; then
        local lines_json="[]"
        while IFS= read -r line; do
            lines_json=$(jq -cn --argjson prev "$lines_json" --arg line "$line" \
                '$prev + [$line]')
        done < <(tree -L 2 "$mp" 2>/dev/null)
        jq -cn --argjson lines "$lines_json" --arg path "$mp" \
            '{status:"ok", data:{path:$path, lines:$lines}}'
        printf '@@DONE@@ tree\n' >&2
    else
        tree -L 2 "$mp"
    fi
}

# § 14.5 密码辅助（临时文件，权限 600，用完即删）
gocryptfs_with_password() {
    local passfile
    passfile=$(mktemp -t gocryptfs-pass.XXXXXX) || return 1
    chmod 600 "$passfile"
    printf '%s' "$1" > "$passfile"
    shift
    local rc=0
    gocryptfs -passfile "$passfile" "$@" || rc=$?
    rm -f "$passfile"
    return $rc
}

# § 15 mount
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
    [ ! -d "$mp" ] && [ "$DRY_RUN" = false ] && mkdir -p "$mp"

    local password
    if [ -t 0 ]; then password=$(read_password_interactive); else password=$(read_password_from_stdin); fi

    local opts=()
    [ "$(get_effective_setting "$name" "gocryptfs.allow_other" "false")" = "true" ] && opts+=("-allow_other")
    [ "$(get_effective_setting "$name" "gocryptfs.read_only" "false")"   = "true" ] && opts+=("-ro")
    [ "$(get_effective_setting "$name" "gocryptfs.nonempty" "false")"    = "true" ] && opts+=("-nonempty")

    if [ "$DRY_RUN" = true ]; then
        log_line "[DRY-RUN] gocryptfs ${opts[*]} $path $mp"
        printf '@@DONE@@ mount (dry-run)\n' >&2
        return 0
    fi

    unlock_dir "$mp"
    log_line "执行: gocryptfs ${opts[*]} $path $mp"
    if gocryptfs_with_password "$password" "${opts[@]}" "$path" "$mp" && is_mounted "$mp"; then
        log_history "mount" "$name" "success"
        emit_done "已挂载: $name"
    else
        lock_dir "$mp"
        log_history "mount" "$name" "failed"
        emit_error $EXIT_ERROR "挂载失败: $name"
    fi
}

# § 16 umount
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
                log_history "umount" "$name" "failed-force"
                emit_error $EXIT_UMOUNT_FORCE "强制卸载失败: $name"
            }
        else
            log_history "umount" "$name" "failed"
            emit_error $EXIT_UMOUNT "卸载失败: $name（可尝试 --force）"
        fi
    fi
    lock_dir "$mp"
    log_history "umount" "$name" "success"
    emit_done "已卸载: $name"
}

# § 17 create
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
        emit_error $EXIT_ERROR "临时挂载失败"
    fi

    log_line "开始迁移: $src → $tmp_mount"
    local rsync_opts=("-a" "-h")
    [ "$keep_source" = "true" ] || rsync_opts+=("--remove-source-files")

    rsync "${rsync_opts[@]}" "$src/" "$tmp_mount/" 2>&1 || {
        fusermount -u "$tmp_mount" 2>/dev/null || true
        emit_error $EXIT_ERROR "rsync 迁移失败"
    }

    [ "$keep_source" = "false" ] && find "$src" -mindepth 1 -type d -empty -delete 2>/dev/null || true

    if [ "$yes" = false ] && [ "$JSON_MODE" != true ]; then
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

    log_history "create" "$name" "success"
    emit_done "创建完成: $name"
}

# § 18 remove
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
            emit_error $EXIT_PASSWORD "自动挂载失败"
        fi
    fi

    if [ "$restore" = true ]; then
        local migrate_to
        [ "$target_mode" = "in_place" ] && migrate_to="${mp}.restore_tmp" || migrate_to="$target_path"
        mkdir -p "$migrate_to"
        log_line "迁移: $mp/ → $migrate_to/"
        rsync -a -h "$mp/" "$migrate_to/" 2>&1 || {
            fusermount -u "$mp" 2>/dev/null || true
            lock_dir "$mp"
            emit_error $EXIT_ERROR "rsync 迁移失败"
        }
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
    log_history "remove" "$name" "success"
    emit_done "删除完成: $name"
}
