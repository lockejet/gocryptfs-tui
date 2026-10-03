# gocryptfs-tui

一个用于管理 `gocryptfs` 加密卷的 TUI + CLI 工具，运行于 Linux。

- **CLI**（`gocryptfs-cli`）：Shell 后端，功能完整，脚本友好
- **TUI**（`gocryptfs-tui`）：终端交互界面，日常操作直观

TUI 通过调用 CLI 完成所有实际工作，因此两者行为完全一致，可分别独立使用。

---

## 目录

- [特性](#特性)
- [依赖](#依赖)
- [安装](#安装)
- [命令行参数](#命令行参数)
- [多语言（i18n）](#多语言i18n)
- [配置](#配置)
- [使用](#使用)
- [日志与历史](#日志与历史)
- [架构](#架构)
- [安全设计](#安全设计)
- [开发](#开发)
- [发布](#发布)
- [许可](#许可)

---

## 特性

- 挂载 / 卸载加密卷（挂载点自动权限锁定）
- 从明文目录创建加密卷（含容量检查、断点续传）
- 删除加密卷（还原明文，可选保留或删除加密后端）
- 支持全局设置与任务级覆盖（overrides）
- 支持 `--dry-run` 预览模式
- 密码通过 stdin 传递，**永不落盘**
- 挂载点未挂载时自动 `chmod 555`（只读锁定），挂载时 `chmod 755`
- 删除加密需输入 `DELETE` 二次确认
- 支持 SMB 共享场景（挂载点始终存在）
- **统一的 JSONL 操作日志**（CLI + TUI 共用一份）
- **TUI 帮助菜单**（含版本信息、路径、快捷键，可导出为 `HELP.md`）
- **多语言界面**：简体中文 / English，支持 `--lang`、配置 `language:`、环境变量与运行期 `L` 键切换
- **主流 TUI 键位惯例**：`Tab` / `Shift+Tab` 轮转换区，`1`/`2`/`3` 直达换页
- **cargo-release + cargo-dist 发布流程**（git tag 触发 CI 多平台构建）

---

## 依赖

运行时依赖：

- `gocryptfs`
- `fusermount`（`gocryptfs` 自带或系统提供）
- `rsync`
- `yq`（[mikefarah/yq](https://github.com/mikefarah/yq)）
- `jq`
- `tree`（可选，用于 `t` 树状视图）

Debian / Ubuntu 安装：

```bash
sudo apt install gocryptfs rsync jq fuse3
sudo wget -qO /usr/local/bin/yq \
    https://github.com/mikefarah/yq/releases/latest/download/yq_linux_amd64
sudo chmod +x /usr/local/bin/yq
```

检查依赖是否齐全：

```bash
gocryptfs-cli --check-deps
```

---

## 安装

### 方式一：官方安装脚本（推荐）

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
    https://github.com/lockejet/gocryptfs-tui/releases/latest/download/gocryptfs-tui-installer.sh | sh
```

脚本会自动：

1. 检测系统架构（x86_64 / aarch64）
2. 检测 libc（glibc / musl）
3. 下载对应 tar.gz
4. 解压到 `~/.cargo/bin` 或 `~/.local/bin`
5. 提示 PATH 配置

### 方式二：手动下载

从 [Releases 页面](https://github.com/lockejet/gocryptfs-tui/releases) 下载对应平台的包：

| 平台 | 文件 |
|------|------|
| Linux amd64（glibc） | `gocryptfs-tui-*-linux-amd64.tar.gz` |
| Linux arm64（glibc） | `gocryptfs-tui-*-linux-arm64.tar.gz` |
| Linux amd64（musl 静态） | `gocryptfs-tui-*-linux-amd64-musl.tar.gz` |
| Linux arm64（musl 静态） | `gocryptfs-tui-*-linux-arm64-musl.tar.gz` |

解压后：

```bash
tar xzf gocryptfs-tui-*-linux-amd64.tar.gz
cd gocryptfs-tui-*-linux-amd64

# 安装 TUI
sudo install -m 0755 gocryptfs-tui /usr/local/bin/

# 安装 CLI
sudo mkdir -p /usr/local/lib/gocryptfs-tui/lib
sudo cp gocryptfs-cli /usr/local/lib/gocryptfs-tui/
sudo cp lib/gocryptfs-lib.sh /usr/local/lib/gocryptfs-tui/lib/
sudo chmod +x /usr/local/lib/gocryptfs-tui/gocryptfs-cli
sudo ln -sf /usr/local/lib/gocryptfs-tui/gocryptfs-cli /usr/local/bin/gocryptfs-cli
```

### 方式三：从源码安装

```bash
git clone https://github.com/lockejet/gocryptfs-tui.git
cd gocryptfs-tui
make build
make install           # 安装 TUI + Shell 后端到 ~/.local（无需 sudo）
# 或系统级安装：
make install-system    # 安装到 /usr/local（内部按需 sudo，请勿直接 sudo 运行）
```

### 升级

TUI 与 Shell 后端（`gocryptfs-cli` + `lib/*.sh`）必须**成套更新**，
否则 TUI 可能调用到 `/usr/local/bin` 下的旧后端，出现「界面已是英文、输出区仍是中文」这类不一致。

```bash
make install           # 覆盖安装到 ~/.local（TUI + Shell 后端，无需 sudo）
make install-system    # 覆盖安装到 /usr/local
```

`install.sh` 也保留了同样能力（并会做依赖检查与安装后自检）：

```bash
./install.sh --prefix ~/.local     # 等价 make install
./install.sh                        # 等价 make install-system
./install.sh --uninstall            # 等价 make uninstall-system
```

排查提示：TUI 启动时输出区会打印**解析后的 CLI 实际路径**，例如
`CLI:  /usr/local/bin/gocryptfs-cli`；若它不是新装的那份，可用
`GOCRYPTFS_CLI=/path/to/shell/gocryptfs-cli gocryptfs-tui` 临时指定。

### 卸载

```bash
# 源码安装（~/.local）
make uninstall

# 系统安装（/usr/local，内部按需 sudo）
make uninstall-system

# 指定前缀
make uninstall PREFIX=/opt/gocryptfs-tui

# 如果是 cargo-dist/shell installer 安装的
gocryptfs-tui-installer.sh uninstall
```

> `make uninstall` 只删除 `$PREFIX/bin/gocryptfs-tui`、`$PREFIX/bin/gocryptfs-cli`
> 与 `$PREFIX/lib/gocryptfs-tui/`，并在命令末尾提示 PATH 中是否仍残留其他安装。
> 手工清理等价于：
> `rm -rf <PREFIX>/lib/gocryptfs-tui <PREFIX>/bin/gocryptfs-cli <PREFIX>/bin/gocryptfs-tui`。

---

## 命令行参数

TUI 支持以下参数：

```
gocryptfs-tui [OPTIONS]

OPTIONS:
    -c, --config <PATH>    配置文件路径
                           [默认: ~/.config/gocryptfs-tui/config.yaml]
    -D, --data-dir <DIR>   数据目录（日志、历史、帮助）
                           [默认: ~/.local/share/gocryptfs-tui]
    -l, --lang <CODE>      界面语言，可选 zh-CN / en-US
    -h, --help             显示帮助
    -V, --version          显示版本信息
```

示例：

```bash
# 使用默认配置
gocryptfs-tui

# 指定配置文件
gocryptfs-tui -c /path/to/config.yaml

# 指定数据目录
gocryptfs-tui -D /tmp/gocryptfs-tui-data

# 指定界面语言（本次运行）
gocryptfs-tui --lang en-US

# 查看版本
gocryptfs-tui --version

# 查看帮助
gocryptfs-tui --help
```

TUI 顶部栏实时显示三行信息：

```
Bin: /usr/local/bin/gocryptfs-tui  CLI:  /usr/local/lib/gocryptfs-tui/gocryptfs-cli          v0.2.0
Config: /home/you/.config/gocryptfs-tui/config.yaml  Data: /home/you/.local/share/gocryptfs-tui
```

---

## 多语言（i18n）

界面文案（**TUI + Shell 后端**）支持 **简体中文（`zh-CN`，内置默认）** 与 **English（`en-US`）**。
TUI 语言在启动时确定，之后可用 `L` 键（或设置浮层内的 `l` 键）在运行期切换（仅当前会话有效）；
TUI 执行命令时会把当前语言通过 `GOCRYPTFS_LANG` 传给 `gocryptfs-cli`，因此输出区与后端提示语言一致。

### 语言选择优先级

从高到低：

```
--lang  >  GOCRYPTFS_TUI_LANG  >  配置 language:  >  系统 locale(LANGUAGE / LC_ALL / LC_MESSAGES / LANG)  >  内置默认 zh-CN
```

- 系统 locale 按 POSIX 取 `LC_ALL` > `LC_MESSAGES` > `LANG`，且只识别受支持的语言。
- `LANGUAGE`（GNU 的冒号分隔偏好列表，如 `zh_CN:en_US`）取第一个可识别的语言，
  优先于上述 locale，但当有效 locale 为 `C` / `POSIX` 时被忽略。
- `C` / `POSIX` 表示「不本地化」，此时回落到内置默认 `zh-CN`；未安装的中文 locale
  （如 `LANG=zh_CN.UTF-8` 但系统无该 locale）不影响识别。
- 配置文件里的 `language:` 会覆盖系统 locale，适合固定语言偏好。

### 指定语言

```bash
# 命令行（最高优先级）
gocryptfs-tui --lang en-US

# 环境变量（仅本次会话）
GOCRYPTFS_TUI_LANG=en-US gocryptfs-tui

# 配置文件（持久化，覆盖系统 locale）
# ~/.config/gocryptfs-tui/config.yaml
language: zh-CN

# 跟随系统 locale
LANG=zh_CN.UTF-8 gocryptfs-tui

# Shell 后端同样支持（-l/--lang 或环境变量）
gocryptfs-cli --lang en-US list
GOCRYPTFS_LANG=en-US gocryptfs-cli -c ~/.config/gocryptfs-tui/config.yaml list
```

### 运行期切换

| 位置 | 键 | 说明 |
|------|----|------|
| 任意页面 | `L` | 在 `zh-CN` / `en-US` 间切换，状态栏与输出区给出提示 |
| 设置浮层 | `l` / `L` | 同上，浮层保持打开，便于对照 |
| 帮助 / 历史浮层 | `L` | 同上，浮层保持打开 |

> 运行期切换只影响当前进程；要长期生效请写入配置 `language:` 或使用 `--lang`。
> 导出的帮助文件（`H`）会跟随当前语言。
> 切换后会重建作用域名称、目录区内容与任务描述，不留旧语言缓存。
> TUI 会把当前语言通过 `GOCRYPTFS_LANG` 传给 `gocryptfs-cli`；若输出区语言与界面不符，
> 多半是调用到了旧版已安装后端（看启动输出里的 `CLI:` 路径，或重新 `make install` / `make install-system`）。
> 英文界面下若探测到旧版后端，TUI 会在输出区显式告警（`does not support --lang`）。
> 旧版后端还会向 `app.log.jsonl` 追加纯文本行（破坏 JSONL）；`gocryptfs-cli log` 已能自动跳过，
> 需要清理历史文件可执行 `grep '^{' app.log.jsonl > tmp && mv tmp app.log.jsonl`。

### 参与翻译 / 新增语言

翻译表位于：

```
src/i18n.rs          # TUI：语言枚举、解析优先级、t!/tn! 宏、键表校验测试
src/i18n/zh_cn.rs    # TUI 简体中文（键 -> 文案，按 key 升序）
src/i18n/en_us.rs    # TUI English
shell/lib/i18n.sh    # Shell 后端：消息表（I18N_ZH / I18N_EN）与语言解析
```

- 键名形如 `<区域>.<语义>`（如 `status.ready`），文案占位符用 `{}`（按顺序）或 `{name}`（具名）。
- **标签类文案不要手写对齐空格**（如 `"名称:      "`）：渲染层会按显示宽度
  （CJK 记 2 列）对同一组标签统一补齐，避免中英文列宽不一致。
- 新增语言：在 `src/i18n/` 下新建 `<locale>.rs`（复制 `en_us.rs` 的键，翻译值），
  在 `src/i18n.rs` 的 `Lang` 枚举、`ALL`、`code()/display_name()/table()` 中登记即可。
- 校验：

```bash
make i18n-check                              # 键表一致性 + 调用点参数 + 中文残留（自带自检）
cargo test i18n                              # 翻译表与渲染层单元测试
make test-i18n                               # TUI + Shell 的语言解析/文案验收
```

Shell 侧约定：文案模板放在 `shell/lib/i18n.sh`，只允许 `printf` 的 `%s` 占位符；
调用方式为 `t <key> [args...]`（不换行）与 `te <key> [args...]`（换行）。

---

## 配置

### 配置文件位置

默认：

```
~/.config/gocryptfs-tui/config.yaml
```

可用 `-c` 指定其他路径：

```bash
gocryptfs-cli -c /path/to/config.yaml list
gocryptfs-tui -c /path/to/config.yaml
```

### 最小配置

```yaml
# 界面语言（可选）：zh-CN / en-US；省略时跟随系统 locale
language: zh-CN

settings:
  unlock_mode: "755"
  lock_mode: "555"

vaults:
  - id: 1
    name: my_vault
    path: /srv/data/.cipher.d/my_vault
    mount_point: /srv/data/my_vault

pending: []
```

### 完整配置示例

参考 `examples/config.yaml.example`，字段含义：

- `language`：界面语言（`zh-CN` / `en-US`），优先级高于系统 locale
- `settings.unlock_mode`：挂载前 chmod 权限，默认 `"755"`
- `settings.lock_mode`：未挂载时 chmod 权限，默认 `"555"`
- `settings.gocryptfs.*`：gocryptfs 挂载选项（`allow_other`、`read_only` 等）
- `settings.rsync.*`：rsync 迁移选项（`archive`、`compress`、`partial` 等）
- `settings.filters[]`：rsync 过滤器规则
- `settings.create.*`：创建策略（`keep_source`、`tmp_mount_suffix`）
- `settings.remove.*`：删除策略（`restore`、`direct_delete_cipher`）
- `settings.logging.*`：日志级别与轮转（`level`、`max_size`、`max_files`）
- `vaults[]`：卷列表，每项含 `id`、`name`、`path`、`mount_point`、可选 `overrides`
- `pending[]`：待处理目录列表（用于 TAB2 创建向导）

### 生效顺序

```
内置默认 < 全局 settings < 任务 overrides < 命令行参数
```

### 策略型选项

以下选项在向导中遵循"**只允许收紧，不允许放宽**"原则：

| 配置项 | 值 | 向导行为 |
|--------|----|---------|
| `create.keep_source` | `false` | 可勾选保留源文件 |
| `create.keep_source` | `true` | 灰色只读 |
| `remove.restore` | `true` | 灰色只读 |
| `remove.restore` | `false` | 可勾选还原 |
| `remove.direct_delete_cipher` | `true` | 可勾选取消 |
| `remove.direct_delete_cipher` | `false` | 灰色只读 |

### 日志设置

```yaml
settings:
  logging:
    # operation   只记业务操作（mount/umount/create/remove）——推荐
    # interactive 业务操作 + 用户交互（按键/页面/向导步骤）
    # debug       全部记录（含内部事件）
    level: operation
    # 单个日志文件上限（字节），超出后轮转
    max_size: 5242880      # 5 MB
    # 保留的历史文件个数
    max_files: 3
```

---

## 使用

### CLI

```
gocryptfs-cli [-c <config>] [-l <lang>] <command> [options]
```

常用命令：

```bash
# 列出卷
gocryptfs-cli list
gocryptfs-cli list --json

# 卷详情
gocryptfs-cli info my_vault

# 挂载（密码通过 stdin）
echo 'your-password' | gocryptfs-cli mount my_vault

# 卸载（失败可加 --force）
gocryptfs-cli umount my_vault
gocryptfs-cli umount my_vault --force

# 查看挂载点目录
gocryptfs-cli ls my_vault
gocryptfs-cli tree my_vault

# 从明文目录创建加密卷（dry-run 预览）
gocryptfs-cli create /srv/photos --name photos --dry-run

# 真实创建（--yes 跳过二次确认）
echo 'your-password' | gocryptfs-cli create /srv/photos --name photos --yes

# 删除加密（还原明文）
echo 'your-password' | gocryptfs-cli remove photos --yes

# 删除加密（保留加密后端）
echo 'your-password' | gocryptfs-cli remove photos --keep-cipher --yes

# 查看操作日志
gocryptfs-cli log                       # 最近 20 条
gocryptfs-cli log --src tui             # 只看 TUI
gocryptfs-cli log --action mount        # 只看挂载
gocryptfs-cli log --result failed       # 只看失败
gocryptfs-cli log --follow              # 实时跟踪
gocryptfs-cli log --json                # JSON 输出

# 检查依赖
gocryptfs-cli --check-deps
```

退出码：

| 码 | 含义 |
|----|------|
| 0 | 成功 |
| 1 | 通用错误 |
| 2 | 密码错误 |
| 3 | 状态错误 |
| 4 | 挂载点问题 |
| 5 | 磁盘空间不足 |
| 6 | 卸载失败 |
| 7 | 强制卸载失败 |
| 8 | 配置错误 |

### TUI

```bash
gocryptfs-tui
```

界面分为 3 个页面：

| 页面 | 名称 | 功能 |
|------|------|------|
| `[1]` | 挂载 / 卸载 | 日常挂载、卸载、查看目录 |
| `[2]` | 创建加密 | 从明文目录创建 |
| `[3]` | 删除加密 | 删除加密卷还原明文 |

界面分为 4 个焦点区（列表 / 详情 / 目录 / 输出），通过 `Tab` 轮转换区，或 `Alt+数字` 直达。

#### 换页与换区（所有页面）

| 键 | 功能 | 类别 |
|----|------|------|
| `1` / `2` / `3` | 直达换页（挂载 / 创建 / 删除） | 换页 |
| `[` / `]` | 顺序换页（上一页 / 下一页，均循环） | 换页 |
| `Tab` / `Shift+Tab` | 轮转换区（正向 / 反向） | 换区 |
| `Alt+1` / `Alt+2` / `Alt+3` / `Alt+4` | 直达换区（列表 / 详情 / 目录 / 输出） | 换区 |

#### 全局按键（所有页面）

| 键 | 功能 |
|----|------|
| `s` | 设置浮层 |
| `h` | 历史浮层 |
| `e` | 外部编辑器打开配置 |
| `r` | 刷新 |
| `L` | 切换界面语言（zh-CN / en-US） |
| `?` | 帮助浮层（内含 `H` 导出） |
| `q` | 退出 |
| `Ctrl+C` | 中断当前任务 |
| `Ctrl+D` × 3（2 秒内） | 强制退出 |

#### 列表焦点按键

| 键 | 功能 |
|----|------|
| `j` / `k` / `↑` / `↓` | 移动选中 |
| `Space` | 确认选中当前项（出现 `▶` 前缀） |
| `Enter` | 创建向导（TAB2）/ 删除向导（TAB3）；TAB1 不再参与挂载/卸载 |
| `m` | 挂载（TAB1）；已挂载时只给灰色提示 |
| `u` | 卸载（TAB1）；未挂载时只给灰色提示 |
| `c` | 创建向导（TAB2，同 Enter） |
| `d` | 删除向导（TAB3，同 Enter） |
| `l` | 列表（ls -la）→ 加载到目录区并切换焦点 |
| `t` | 树状（tree）→ 加载到目录区并切换焦点 |

> **重要**：先按 `Space` 选中，再按 `m`/`u`/`c`/`d` 执行（TAB1 用 `m` 挂载、`u` 卸载；`Enter` 只用于 TAB2/TAB3 进入向导）。移动光标会清除选中状态。

#### 详情 / 目录 / 输出焦点按键

| 键 | 功能 |
|----|------|
| `↑` / `↓` | 垂直滚动 |
| `←` / `→` | 水平滚动 |
| `PgUp` / `PgDn` | 翻页 |
| `g` / `G` | 跳到首 / 尾 |
| `c` | 清空（目录区 / 输出区） |
| `l` / `t` | 刷新目录视图（仅目录焦点） |

#### 向导内按键

| 键 | 功能 |
|----|------|
| `Space` | 切换选项 |
| `j` / `k` | 选项间移动 |
| `Enter` | 下一步 |
| `Esc` | 取消 |
| `Backspace` / `Delete` / `Ctrl+H` | 删除密码字符 |

#### 历史浮层按键

| 键 | 功能 |
|----|------|
| `j` / `k` / `↑` / `↓` | 移动选中 |
| `s` | 循环切换来源过滤（全部 → cli → tui → 全部） |
| `r` | 循环切换结果过滤（全部 → success → failed → started → cancelled → 全部） |
| `a` | 循环切换操作过滤（全部 → mount → umount → create → remove → 全部） |
| `Esc` | 关闭 |

#### 帮助浮层按键

| 键 | 功能 |
|----|------|
| `H` | 导出帮助到 `<data_dir>/HELP.md`（跟随当前语言） |
| `Esc` / `?` / `q` | 关闭 |

#### 设置浮层按键

| 键 | 功能 |
|----|------|
| `Tab` | 切换作用域（全局 / 各卷） |
| `g` / `r` / `f` / `p` | 切换分类（gocryptfs / rsync / filters / 权限） |
| `l` | 切换界面语言 |
| `e` | 外部编辑器打开配置 |
| `Esc` | 关闭 |

---

## 日志与历史

### 统一日志

CLI 和 TUI 的所有操作记录到**同一个文件**：

```
~/.local/share/gocryptfs-tui/app.log.jsonl
```

每行一条 JSON 记录：

```json
{
  "ts": "2026-09-30T15:00:07+08:00",
  "src": "cli",
  "action": "mount",
  "target": "test_plain",
  "result": "success",
  "detail": "",
  "pid": 123456,
  "duration_ms": 2050
}
```

字段含义：

| 字段 | 说明 |
|------|------|
| `ts` | ISO 8601 时间戳 |
| `src` | 来源：`cli` 或 `tui` |
| `action` | 动作：`mount` / `umount` / `create` / `remove` / `tui.start` / `tui.quit` |
| `target` | 目标：卷名或描述 |
| `result` | `started` / `success` / `failed` / `cancelled` |
| `detail` | 附加信息（如命令行、错误细节） |
| `pid` | 关联进程 PID（TUI 侧填写，CLI 侧为 null） |
| `duration_ms` | 耗时（毫秒，TUI 侧填写，CLI 侧为 null） |

### 日志轮转

单文件超过 `settings.logging.max_size` 后：

```
app.log.jsonl  →  app.log.jsonl.1  →  app.log.jsonl.2  →  ...
```

保留 `settings.logging.max_files` 个历史文件。

### 旧历史迁移

早期版本使用 `history.jsonl`（字段名 `name` / `status`）。首次运行新版本时，TUI 会自动把旧文件内容迁移到 `app.log.jsonl` 并删除旧文件。字段映射：

| 旧 | 新 |
|----|----|
| `name` | `target` |
| `status` | `result` |
| （无） | `src: "cli"` |

### 查询

CLI 侧：

```bash
gocryptfs-cli log                        # 最近 20 条（人类可读，带颜色）
gocryptfs-cli log --limit 100            # 最近 100 条
gocryptfs-cli log --src tui              # 只看 TUI
gocryptfs-cli log --action mount         # 只看挂载
gocryptfs-cli log --result failed        # 只看失败
gocryptfs-cli log --since 2026-09-29     # 按时间过滤
gocryptfs-cli log --follow               # 实时跟踪
gocryptfs-cli log --json                 # JSON 输出
```

TUI 侧：

- 按 `h` 打开历史浮层
- 按 `s` / `r` / `a` 切换来源 / 结果 / 操作过滤
- 按 `j` / `k` 移动选中

---

## 架构

```
┌──────────────────────────────────────────────────────┐
│                   用户                                │
└────────────┬─────────────────────┬───────────────────┘
             │ TUI                 │ CLI
             ▼                     ▼
   ┌─────────────────┐    ┌─────────────────┐
   │ gocryptfs-tui   │    │ gocryptfs-cli   │
   │ (Rust)          │───▶│ (Bash)          │
   └─────────────────┘    └────────┬────────┘
                                   │
                          ┌────────▼────────┐
                          │  gocryptfs-cli  │
                          │  shell/lib/     │
                          │  gocryptfs-lib  │
                          └────────┬────────┘
                                   │
              ┌────────────────────┼────────────────────┐
              ▼                    ▼                    ▼
        ┌──────────┐         ┌──────────┐        ┌──────────┐
        │gocryptfs │         │fusermount│        │  rsync   │
        └──────────┘         └──────────┘        └──────────┘
```

### 目录结构

```
gocryptfs-tui/
├── Cargo.toml
├── build.rs                 # 编译期版本注入
├── Makefile
├── release.toml             # cargo-release 配置
├── cliff.toml               # git-cliff 配置
├── Cross.toml               # cross 交叉编译配置
├── dist-workspace.toml      # cargo-dist 配置
├── RELEASING.md             # 发版流程文档
├── README.md
├── CHANGELOG.md
├── LICENSE
├── SECURITY.md
├── .github/
│   └── workflows/
│       └── release.yml      # cargo-dist CI workflow
├── src/
│   ├── main.rs              # TUI 主入口 + 渲染
│   ├── cli.rs               # 命令行参数解析（含 --lang）
│   ├── logger.rs            # 日志模块
│   ├── i18n.rs              # i18n API（语言解析、t!/tn! 宏、键表测试）
│   ├── i18n_render_tests.rs # 渲染层多语言测试（仅测试构建）
│   └── i18n/
│       ├── zh_cn.rs         # 简体中文翻译表
│       └── en_us.rs         # English 翻译表
├── shell/
│   ├── gocryptfs-cli        # CLI 主入口（含 -l/--lang）
│   └── lib/
│       ├── gocryptfs-lib.sh # 全部库函数
│       └── i18n.sh          # Shell 侧消息表与语言解析
├── tools/
│   └── i18n_check.py        # TUI + Shell 的 i18n 键表/调用点一致性校验
├── test/
│   ├── create-test-env.sh
│   ├── cleanup-test-env.sh
│   ├── test-batch1.sh
│   ├── test-batch2.sh
│   ├── test-i18n.sh         # CLI 侧 i18n 验收测试
│   └── test-all.sh
└── examples/
    └── config.yaml.example
```

### CLI 与 TUI 的交互协议

CLI 通过 **stderr 协议行** 向 TUI 汇报进度和结果：

| 协议行 | 含义 |
|--------|------|
| `@@PROGRESS@@ <pct> <done> <total>` | 迁移进度 |
| `@@CHECK@@ <key> <value>` | 检查点数据（如容量） |
| `@@DONE@@ <message>` | 操作成功 |
| `@@ERROR@@ <code> <message>` | 操作失败 |

TUI 捕获这些行，更新界面状态；普通 stdout/stderr 显示在输出区。

---

## 安全设计

本项目**不重复实现密码学**，所有加密由 `gocryptfs` 提供。项目的安全职责：

### 密码处理

- 密码通过 stdin 从 TUI 传给 CLI
- CLI 内部用**临时文件**（权限 `600`）传给 gocryptfs
- 挂载完成后**立即删除**临时文件
- 密码**永不写入配置文件、日志或历史**

### 配置文件

- **不包含任何密码或密钥**
- 只包含路径、权限模式、选项开关

### 挂载点权限

- 未挂载时 `chmod 555`（只读锁定）
- 挂载时 `chmod 755`
- 使用系统 `chmod` 命令（保留 setgid/setuid/sticky 特殊位）

### 删除保护

- 默认**不删除加密后端**
- 只有在配置显式授权（`remove.direct_delete_cipher: true`）且用户输入 `DELETE` 后才删除
- 无法在向导中"越权"启用删除后端

### 日志隐私

- 日志不记录密码
- 日志不记录明文内容
- 日志只记录操作元数据（卷名、路径、结果）

详细安全说明见 [SECURITY.md](SECURITY.md)。

---

## 开发

### 编译

```bash
# 本机 release 编译
make build

# 调试版
make build-debug

# 交叉编译（需要 cross）
make build-arm64
make build-musl
make build-arm64-musl
make build-all
```

### 检查与测试

```bash
# 全部检查：fmt + clippy + 单元测试
make check

# 单独跑
make fmt           # 格式化代码
make lint          # clippy（-D warnings）
make i18n-check    # i18n 键表/调用点校验
make test          # Rust 单元测试（含多语言渲染测试）
make test-i18n     # i18n 验收：TUI + Shell 的语言解析矩阵与文案
make test-env      # 生成 shell 测试环境
make test-cli      # 运行 shell 测试批次
make test-env-clean  # 清理测试环境

# 完整验证
make verify
```

### 本地安装

```bash
make install            # 装到 ~/.local（TUI + Shell 后端）
make install-system     # 装到 /usr/local（按需 sudo）
make uninstall          # 卸载 ~/.local
make uninstall-system   # 卸载 /usr/local
```

### 全部 Make 目标

```bash
make help
```

---

## 发布

本项目使用 `cargo-release` + `cargo-dist` 组合：

- **cargo-release**：bump 版本、打 tag、push（**不再自动改写 CHANGELOG**）
- **cargo-dist**：CI 多平台构建、生成 installer、创建 GitHub Release

> CHANGELOG.md 只在显式执行 `make changelog`（或 `make release` 的显式步骤）时生成；
> `make dry-run` / `cargo release --dry-run` 不会改动任何文件。
> 需要写提交标题表达不了的说明时，在 `changelog.d/` 放片段（`make changelog` 自动合并，
> `make changelog-check` 校验格式），详见 `changelog.d/README.md`。

### 前置工具

```bash
cargo install cargo-release --locked
cargo install git-cliff --locked
cargo install cargo-dist --locked
cargo install cross --git https://github.com/cross-rs/cross    # 可选
gh auth login
```

### 发布步骤

```bash
# 1. 预览 CHANGELOG 差异（只写 .staging/，不动 CHANGELOG.md）
make changelog-preview

# 2. 前置检查（除 CHANGELOG.md 外工作区需干净）
make release-check

# 3. 预览 cargo-release 动作（只读，不改文件）
make dry-run

# 4. 一步发布：生成 CHANGELOG → 提交 → cargo release（bump + tag + push）
make release

# 或指定级别
make release LEVEL=minor
make release LEVEL=major

# 5. 查看 CI 进度
gh run watch

# 6. 验证 Release
gh release view v0.2.0 --web
```

push tag 后 GitHub Actions 自动触发 `.github/workflows/release.yml`，构建 4 个平台（gnu/musl × amd64/arm64）并创建 Release。

详细流程见 [RELEASING.md](RELEASING.md)。

---

## 许可

MIT License，见 [LICENSE](LICENSE)。