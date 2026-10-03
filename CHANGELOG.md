# Changelog

本项目遵循 [Semantic Versioning](https://semver.org/)。

## [0.2.0] - 2026-10-03

### Features
- 新增 i18n 国际化支持：简体中文（zh-CN）与 English（en-US）界面文案
- 新增 `-l, --lang <CODE>` 命令行参数，新增配置项 `language:` 与环境变量 `GOCRYPTFS_TUI_LANG`
- 语言选择优先级：`--lang` > `GOCRYPTFS_TUI_LANG` > `language:` > 系统 locale > 默认 `zh-CN`；
  系统 locale 按 POSIX 取 `LC_ALL`/`LC_MESSAGES`/`LANG`，`LANGUAGE` 偏好列表次之，`C`/`POSIX` 视为不本地化
- TUI 内可用 `L` 键（帮助/历史浮层内同样有效，设置浮层 `l`/`L`）在运行期切换语言；
  切换后重建作用域名、目录区与任务描述缓存；导出的帮助文件跟随当前语言
- Shell 后端（`gocryptfs-cli` / `gocryptfs-lib.sh`）接入同一套语言：新增 `-l/--lang`、
  `shell/lib/i18n.sh` 消息表；TUI 通过 `GOCRYPTFS_LANG` 把当前语言传给子进程
- TUI 界面调整：移除 `o`（xdg-open 打开挂载点）功能及其键位/帮助提示；
  底部 GLOBAL 行行首新增 `En/中文[L]` 语言切换提示；底部状态栏去除背景色
- 顶部栏改为 2 行：第 1 行 `Bin: <可执行文件路径>  CLI: <后端路径>` + 右对齐版本号；
  第 2 行 `Config: <配置路径>  Data: <数据目录>`（不再显示日志/历史文件）；
  窄终端下两个路径按长度比例分配宽度并保留尾部

### Bug Fixes
- 标签列宽改为渲染时按显示宽度计算（翻译表不再内嵌对齐空格），修复英文界面标签错列
- 修复英文标签（`Command: `）更宽导致右上角版本被裁成 `v0.` 的问题，行宽改为按显示宽度排版
- 配置/数据目录路径统一解析为绝对路径（展开 `~`、相对路径基于当前工作目录），
  修复 `-c demo.yaml` 这类相对路径在顶栏只显示文件名的问题

### Documentation
- README 新增「多语言（i18n）」章节，补充 `--lang` 与语言键位说明

### Build
- 新增 `make i18n-check` 与 `tools/i18n_check.py`：覆盖 TUI 的 `t!`/`tn!` 与 Shell 的 `t`/`te`
  全部调用点（含嵌套）、两张消息表的一致性、占位符参数与中文残留，并自带扫描器自检
- 新增 `make test-i18n` 与 `test/test-i18n.sh`：覆盖 TUI 与 Shell 的语言解析矩阵、帮助与报错文案
- `make install-local` / `make install` 现在会连同 Shell 后端一起安装到 `~/.local`
  （此前只装 TUI 二进制，容易与系统里的旧 `gocryptfs-cli` 混用）
- Makefile 新增 `install` / `uninstall` / `install-system` / `uninstall-system`
  （`uninstall` 带路径护栏）；`install.sh` 支持 `--prefix` / `--uninstall` / `--no-build`、
  按需 sudo，并在安装后自检 PATH 中的 CLI 是否为本次安装
- TUI 启动输出打印解析后的 CLI 实际路径；英文界面下启动时探测后端是否支持 `--lang`，
  旧版后端会给出显式告警
- `gocryptfs-cli log` 跳过日志中的非 JSON 行（旧版后端曾向 `app.log.jsonl` 追加纯文本）
- 移除 cargo-release 的 `pre-release-hook`：`make dry-run` / `cargo release --dry-run`
  不再改写 `CHANGELOG.md`；新增 `changelog.d/` 片段机制（`make changelog-preview` 预览、
  `make changelog` 合并片段且片段保留，发布时才归档），手写内容随代码提交可长期保留

### Behavior Changes
- TAB1 键位语义调整：`m` 只挂载、`u` 只卸载（取消 `m`/`Enter` 的挂载/卸载 toggle）；
  已挂载时按 `m`、未挂载时按 `u` 仅显示灰色信息提示（`Bin`/`Config` 等路径展示不受影响）
- `Enter` 不再参与 TAB1 的挂载/卸载（仅 TAB2/TAB3 保留进入向导）
- 英文界面文案统一首字母大写（`Page` / `Focus` / `Settings` / `Mount` / `Umount` …），
  `refresh` 改为 `Reload`；状态栏去掉 `or`/`或`并写作 `Page[1/2/3] [/]`；
  语言切换提示由 `EN/中文[L]` 改为 `En/中文[L]`

## [0.1.2] - 2026-09-30

### Documentation
- 更新 README 与 CHANGELOG 键位说明


### Features
- 换页与换区键位对齐主流 TUI 惯例 (tui)

## [0.1.1] - 2026-09-30

### Documentation
- 更新 README 并生成 CHANGELOG


### Features
- 新增命令行参数解析 (cli)
- 新增统一 JSONL 日志模块 (logger)
- 统一日志格式并新增 log 子命令 (cli)
- 接入参数解析与日志，新增帮助导出和历史过滤 (tui)


### Miscellaneous
- 初始化 cargo-dist


### Build
- 新增版本注入与 cargo-release/cargo-dist 发布流程


### Revert
- 撤回配置文件 git 版本管理
