#!/bin/bash
# i18n.sh — gocryptfs-cli 的国际化支持（简体中文 / English）
#
# 语言解析优先级（与 TUI 保持一致，见 src/i18n.rs）：
#   --lang  >  GOCRYPTFS_LANG  >  GOCRYPTFS_TUI_LANG  >  配置 language:
#           >  系统 locale(LC_ALL/LC_MESSAGES/LANG，LANGUAGE 次之)  >  默认 zh-CN
#   系统 locale 为 C / POSIX 时视为「不本地化」，回落到默认 zh-CN。
#
# 用法：
#   t  <key> [args...]   取文案（不换行，占位符 %s 由 printf 替换）
#   te <key> [args...]   取文案并换行
#   两者都直接写 stdout，写 stderr 用 `t key >&2` / `te key >&2`。
#
# 约定：
#   * 文案模板里只允许 printf 的 %s 占位符，不要出现裸 %（需要字面量请写 %%）。
#   * 新增语言：加一张 I18N_XX 表，并在 i18n_normalize / i18n_lookup 中登记。

# 当前语言（zh-CN / en-US）
I18N_LANG="${I18N_LANG:-zh-CN}"
# 是否由 --lang 显式指定（显式时 i18n_init 不再覆盖）
I18N_EXPLICIT="${I18N_EXPLICIT:-false}"

declare -A I18N_ZH I18N_EN

# ------------------------------------------------------------
# 消息表：简体中文
# ------------------------------------------------------------
I18N_ZH=(
    # gocryptfs-cli
    [cli.error.config_needs_value]='错误: -c/--config 需要参数'
    [cli.error.lang_needs_value]='错误: -l/--lang 需要语言代码'
    [cli.error.unsupported_lang]='不支持的语言: %s（可用: zh-CN, en-US）'
    [cli.error.unknown_command]='未知命令: %s'
    [cli.help]='gocryptfs-cli - gocryptfs-tui 的 Shell 后端
用法: gocryptfs-cli [-c <config>] [-l <lang>] <command> [options]
命令: list / info / ls / tree / mount / umount / create / remove
      config / edit / check-deps / log / help

全局选项:
  -c, --config FILE  配置文件路径
  -l, --lang CODE    界面语言: zh-CN | en-US
  --json             JSON 输出
  -v, --verbose      详细输出
  --dry-run          预览模式

log 子命令选项:
  --limit N        显示最近 N 条（默认 20）
  --src SRC        过滤来源: cli / tui
  --action ACTION  过滤操作: mount / umount / create / remove / tui.start / ...
  --result RESULT  过滤结果: success / failed / started / cancelled
  --since DATE     起始时间（ISO 8601）
  --follow         实时跟踪（类似 tail -f）
  --json           JSON 输出'

    # 库自检
    [lib.self_index]='gocryptfs-lib.sh — 函数索引（轮询版）'

    # 依赖检查
    [deps.missing]='[!] 缺少依赖: %s'
    [deps.ok]='[✔] 依赖齐全'

    # 通用错误
    [error.config_missing]='配置文件不存在: %s'
    [error.password_empty]='密码为空'
    [error.password_empty_retry]='密码不能为空'
    [error.password_mismatch]='两次密码不一致'
    [error.tmp_password_file]='无法创建密码临时文件'
    [error.tmp_output_file]='无法创建输出临时文件'
    [error.mount_timeout]='挂载超时（30 秒内挂载点未出现）'
    [error.source_missing]='源不存在: %s'
    [error.capacity_failed]='无法计算容量'
    [error.already_running]='另一个进程正在运行'
    [error.unknown_option]='未知选项: %s'
    [error.user_abort]='用户取消'
    [error.list_empty]='(配置中没有任何卷)'
    [error.rsync_failed]='rsync 迁移失败'

    # 交互提示
    [prompt.password]='请输入密码: '
    [prompt.password_again]='再次输入: '
    [prompt.delete_warning]='警告：将删除加密卷并还原明文。'
    [prompt.delete_confirm]='请输入 DELETE 确认: '
    [confirm.replace_mountpoint]='确认替换挂载点 %s ？'

    # 目录权限
    [log.dir_locked]='已锁定 (%s): %s'
    [log.dir_unlocked]='已解锁 (%s): %s'

    # 用法
    [usage.info]='用法: info <name>'
    [usage.ls]='用法: ls <name>'
    [usage.tree]='用法: tree <name>'
    [usage.mount]='用法: mount <name>'
    [usage.umount]='用法: umount <name> [--force]'
    [usage.remove]='用法: remove <name> [options]'

    # 卷状态
    [error.vault_missing]='卷不存在: %s'
    [error.not_mounted]='卷未挂载: %s'
    [error.already_mounted]='卷已挂载: %s'
    [error.cipher_dir_missing]='加密目录不存在: %s'
    [error.invalid_vault]='不是有效的 gocryptfs 加密卷（缺少 gocryptfs.conf）: %s'
    [error.mountpoint_not_empty]='挂载点非空: %s（需设置 nonempty: true）'
    [error.tree_missing]='tree 未安装'
    [error.no_space]='磁盘空间不足'

    # info 输出
    [info.id]='ID: %s'
    [info.name]='名称: %s'
    [info.cipher_path]='加密路径: %s'
    [info.mount_point]='挂载点: %s'
    [info.mounted]='已挂载: %s'
    [info.locked]='已锁定: %s'
    [info.mode]='权限: %s'
    [info.valid]='有效: %s'

    # 执行与日志
    [log.exec]='执行: %s'
    [log.mountpoint_created]='创建挂载点: %s'
    [log.auto_mount]='自动挂载: %s → %s'
    [log.mount_tmp]='挂载到临时点: %s'
    [log.mount_final]='正式挂载: %s → %s'
    [log.create_cipher]='创建加密后端: %s'
    [log.migrate_start]='开始迁移: %s → %s'
    [log.migrate]='迁移: %s → %s'
    [log.umount]='卸载: %s'
    [log.delete_cipher]='删除加密后端: %s'
    [log.keep_cipher]='保留加密后端: %s'
    [log.dry_run_keep]='[DRY-RUN] 保留 %s'
    [log.none]='（暂无日志: %s）'
    [log.force_umount_failed]='强制卸载也失败'

    # 结果
    [done.mounted]='已挂载: %s'
    [done.umounted]='已卸载: %s'
    [done.created]='创建完成: %s'
    [done.removed]='删除完成: %s'

    # 失败
    [error.wrong_password]='密码错误'
    [error.mount_failed]='挂载失败: %s'
    [error.mount_failed_notmounted]='挂载失败: %s（gocryptfs 未成功挂载）'
    [error.umount_failed]='卸载失败: %s（可尝试 --force）'
    [error.force_umount_failed]='强制卸载失败: %s'
    [error.umount_tmp_failed]='卸载临时点失败'
    [error.extra_arg]='多余参数: %s'
    [error.source_dir_missing]='缺少源目录'
    [error.name_missing]='缺少 --name'
    [error.source_dir_not_exist]='源目录不存在: %s'
    [error.source_in_use]='源目录 %s 已绑定卷「%s」，请先删除或使用其他源目录'
    [error.cannot_create_dir]='无法创建 %s'
    [error.init_failed]='gocryptfs -init 失败'
    [error.tmp_mount_failed]='临时挂载失败'
    [error.auto_mount_failed]='自动挂载失败'
    [error.move_source_failed]='移动源目录失败'
    [error.final_mount_failed]='最终挂载失败'
    [error.umount_failed_plain]='卸载失败'
    [error.delete_failed]='删除 %s 失败'
)

# ------------------------------------------------------------
# 消息表：English (en-US)
# ------------------------------------------------------------
I18N_EN=(
    [cli.error.config_needs_value]='Error: -c/--config needs a value'
    [cli.error.lang_needs_value]='Error: -l/--lang needs a language code'
    [cli.error.unsupported_lang]='Unsupported language: %s (available: zh-CN, en-US)'
    [cli.error.unknown_command]='Unknown command: %s'
    [cli.help]='gocryptfs-cli - shell backend of gocryptfs-tui
Usage: gocryptfs-cli [-c <config>] [-l <lang>] <command> [options]
Commands: list / info / ls / tree / mount / umount / create / remove
          config / edit / check-deps / log / help

Global options:
  -c, --config FILE  config file path
  -l, --lang CODE    UI language: zh-CN | en-US
  --json             JSON output
  -v, --verbose      verbose output
  --dry-run          preview only

log subcommand options:
  --limit N        show the latest N entries (default 20)
  --src SRC        filter by source: cli / tui
  --action ACTION  filter by action: mount / umount / create / remove / tui.start / ...
  --result RESULT  filter by result: success / failed / started / cancelled
  --since DATE     start time (ISO 8601)
  --follow         follow in real time (like tail -f)
  --json           JSON output'

    [lib.self_index]='gocryptfs-lib.sh — function index (polling build)'

    [deps.missing]='[!] Missing dependencies: %s'
    [deps.ok]='[✔] All dependencies present'

    [error.config_missing]='Config file not found: %s'
    [error.password_empty]='Password is empty'
    [error.password_empty_retry]='Password cannot be empty'
    [error.password_mismatch]='Passwords do not match'
    [error.tmp_password_file]='Cannot create temporary password file'
    [error.tmp_output_file]='Cannot create temporary output file'
    [error.mount_timeout]='Mount timed out (mount point did not appear within 30s)'
    [error.source_missing]='Source does not exist: %s'
    [error.capacity_failed]='Cannot compute capacity'
    [error.already_running]='Another process is already running'
    [error.unknown_option]='Unknown option: %s'
    [error.user_abort]='Cancelled by user'
    [error.list_empty]='(no vaults in config)'
    [error.rsync_failed]='rsync migration failed'

    [prompt.password]='Password: '
    [prompt.password_again]='Repeat password: '
    [prompt.delete_warning]='Warning: this deletes the encrypted vault and restores plaintext.'
    [prompt.delete_confirm]='Type DELETE to confirm: '
    [confirm.replace_mountpoint]='Replace mount point %s?'

    [log.dir_locked]='Locked (%s): %s'
    [log.dir_unlocked]='Unlocked (%s): %s'

    [usage.info]='Usage: info <name>'
    [usage.ls]='Usage: ls <name>'
    [usage.tree]='Usage: tree <name>'
    [usage.mount]='Usage: mount <name>'
    [usage.umount]='Usage: umount <name> [--force]'
    [usage.remove]='Usage: remove <name> [options]'

    [error.vault_missing]='Vault not found: %s'
    [error.not_mounted]='Vault is not mounted: %s'
    [error.already_mounted]='Vault is already mounted: %s'
    [error.cipher_dir_missing]='Cipher directory does not exist: %s'
    [error.invalid_vault]='Not a valid gocryptfs encrypted vault (missing gocryptfs.conf): %s'
    [error.mountpoint_not_empty]='Mount point is not empty: %s (set nonempty: true)'
    [error.tree_missing]='tree is not installed'
    [error.no_space]='Not enough disk space'

    [info.id]='ID: %s'
    [info.name]='Name: %s'
    [info.cipher_path]='Cipher path: %s'
    [info.mount_point]='Mount point: %s'
    [info.mounted]='Mounted: %s'
    [info.locked]='Locked: %s'
    [info.mode]='Mode: %s'
    [info.valid]='Valid: %s'

    [log.exec]='Running: %s'
    [log.mountpoint_created]='Creating mount point: %s'
    [log.auto_mount]='Auto-mounting: %s → %s'
    [log.mount_tmp]='Mounting to temp point: %s'
    [log.mount_final]='Final mount: %s → %s'
    [log.create_cipher]='Creating cipher backend: %s'
    [log.migrate_start]='Starting migration: %s → %s'
    [log.migrate]='Migrating: %s → %s'
    [log.umount]='Unmounting: %s'
    [log.delete_cipher]='Deleting cipher backend: %s'
    [log.keep_cipher]='Keeping cipher backend: %s'
    [log.dry_run_keep]='[DRY-RUN] keep %s'
    [log.none]='(no log yet: %s)'
    [log.force_umount_failed]='forced unmount also failed'

    [done.mounted]='Mounted: %s'
    [done.umounted]='Unmounted: %s'
    [done.created]='Created: %s'
    [done.removed]='Deleted: %s'

    [error.wrong_password]='Wrong password'
    [error.mount_failed]='Mount failed: %s'
    [error.mount_failed_notmounted]='Mount failed: %s (gocryptfs did not mount successfully)'
    [error.umount_failed]='Unmount failed: %s (try --force)'
    [error.force_umount_failed]='Forced unmount failed: %s'
    [error.umount_tmp_failed]='Failed to unmount temp point'
    [error.extra_arg]='Unexpected argument: %s'
    [error.source_dir_missing]='Missing source directory'
    [error.name_missing]='Missing --name'
    [error.source_dir_not_exist]='Source directory does not exist: %s'
    [error.source_in_use]='Source directory %s is already bound to vault "%s"; delete it first or use another source directory'
    [error.cannot_create_dir]='Cannot create %s'
    [error.init_failed]='gocryptfs -init failed'
    [error.tmp_mount_failed]='Temporary mount failed'
    [error.auto_mount_failed]='Auto-mount failed'
    [error.move_source_failed]='Failed to move source directory'
    [error.final_mount_failed]='Final mount failed'
    [error.umount_failed_plain]='Unmount failed'
    [error.delete_failed]='Failed to delete %s'
)

# ------------------------------------------------------------
# 语言解析
# ------------------------------------------------------------

# 规范化语言代码；无法识别时输出空串。
i18n_normalize() {
    local raw="${1:-}" code
    raw="$(printf '%s' "$raw" | tr '[:upper:]' '[:lower:]' | tr '_' '-')"
    raw="${raw%%.*}"
    raw="${raw%%@*}"
    code="${raw%%-*}"
    case "$code" in
        zh|cn|chs|chinese) printf 'zh-CN' ;;
        en|us|english)     printf 'en-US' ;;
        *)                 printf '' ;;
    esac
}

# 读取配置文件里的 language: / lang:
i18n_config_lang() {
    local cfg="${CONFIG_FILE:-}"
    [ -n "$cfg" ] && [ -f "$cfg" ] || return 0
    sed -n -E 's/^[[:space:]]*(language|lang)[[:space:]]*:[[:space:]]*//p' "$cfg" 2>/dev/null \
        | head -n 1 \
        | sed -E "s/^[\"']//; s/[\"'][[:space:]]*$//; s/[[:space:]]+$//"
}

# 按 POSIX/GNU 规则取系统 locale：LC_ALL > LC_MESSAGES > LANG，LANGUAGE 次之。
# C / POSIX 表示不本地化，返回 1。
i18n_env_lang() {
    local name val eff="" base list item code
    for name in LC_ALL LC_MESSAGES LANG; do
        val="${!name:-}"
        if [ -n "$val" ]; then eff="$val"; break; fi
    done
    [ -n "$eff" ] || return 1
    base="$(printf '%s' "$eff" | tr '[:upper:]' '[:lower:]')"
    base="${base%%.*}"
    base="${base%%@*}"
    case "$base" in c|posix) return 1 ;; esac
    list="${LANGUAGE:-}"
    if [ -n "$list" ]; then
        local IFS=':'
        for item in $list; do
            code="$(i18n_normalize "$item")"
            [ -n "$code" ] && { printf '%s' "$code"; return 0; }
        done
    fi
    code="$(i18n_normalize "$eff")"
    [ -n "$code" ] && { printf '%s' "$code"; return 0; }
    return 1
}

# 初始化语言（在解析完 -c/-l 之后调用；--lang 显式指定时不再覆盖）。
i18n_init() {
    [ "$I18N_EXPLICIT" = "true" ] && return 0
    local code=""
    code="$(i18n_normalize "${GOCRYPTFS_LANG:-}")"
    [ -n "$code" ] || code="$(i18n_normalize "${GOCRYPTFS_TUI_LANG:-}")"
    [ -n "$code" ] || code="$(i18n_normalize "$(i18n_config_lang)")"
    [ -n "$code" ] || code="$(i18n_env_lang || true)"
    I18N_LANG="${code:-zh-CN}"
    export I18N_LANG
}

# 显式设置语言（--lang），返回 0/1 表示是否受支持。
i18n_set_lang() {
    local code
    code="$(i18n_normalize "${1:-}")"
    [ -n "$code" ] || return 1
    I18N_LANG="$code"
    I18N_EXPLICIT=true
    export I18N_LANG I18N_EXPLICIT
}

# 查表：结果写入 _I18N_TPL（缺失时回退到另一种语言，再缺失则返回键名）。
i18n_lookup() {
    local key="$1"
    if [ "$I18N_LANG" = "en-US" ]; then
        _I18N_TPL="${I18N_EN[$key]:-}"
        [ -n "$_I18N_TPL" ] || _I18N_TPL="${I18N_ZH[$key]:-}"
    else
        _I18N_TPL="${I18N_ZH[$key]:-}"
        [ -n "$_I18N_TPL" ] || _I18N_TPL="${I18N_EN[$key]:-}"
    fi
    [ -n "$_I18N_TPL" ] || _I18N_TPL="$key"
}

# 取文案（不换行）。
t() {
    local key="$1"
    shift
    i18n_lookup "$key"
    if [ "$#" -gt 0 ]; then
        # shellcheck disable=SC2059  # 模板来自消息表，占位符仅 %s
        printf "$_I18N_TPL" "$@"
    else
        printf '%s' "$_I18N_TPL"
    fi
}

# 取文案并换行。
te() {
    t "$@"
    printf '\n'
}
