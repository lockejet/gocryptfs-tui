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
make install-local     # 安装到 ~/.local/bin
```

### 卸载

```bash
# 如果是 shell installer 安装的
gocryptfs-tui-installer.sh uninstall

# 如果是手动安装的
sudo rm -rf /usr/local/lib/gocryptfs-tui
sudo rm -f /usr/local/bin/gocryptfs-cli
sudo rm -f /usr/local/bin/gocryptfs-tui
```

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

# 查看版本
gocryptfs-tui --version

# 查看帮助
gocryptfs-tui --help
```

TUI 顶部栏实时显示三行信息：

```
命令: /usr/local/bin/gocryptfs-tui                              v0.2.0
CLI:  gocryptfs-cli   配置: ~/.config/gocryptfs-tui/config.yaml
数据: ~/.local/share/gocryptfs-tui  日志: app.log.jsonl  历史: history.jsonl
```

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
gocryptfs-cli [-c <config>] <command> [options]
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
| `?` | 帮助浮层（内含 `H` 导出） |
| `q` | 退出 |
| `Ctrl+C` | 中断当前任务 |
| `Ctrl+D` × 3（2 秒内） | 强制退出 |

#### 列表焦点按键

| 键 | 功能 |
|----|------|
| `j` / `k` / `↑` / `↓` | 移动选中 |
| `Space` | 确认选中当前项（出现 `▶` 前缀） |
| `Enter` | 挂载 / 卸载（TAB1）/ 创建向导（TAB2）/ 删除向导（TAB3） |
| `m` | 挂载 / 卸载（TAB1，同 Enter） |
| `u` | 卸载（TAB1） |
| `c` | 创建向导（TAB2，同 Enter） |
| `d` | 删除向导（TAB3，同 Enter） |
| `l` | 列表（ls -la）→ 加载到目录区并切换焦点 |
| `t` | 树状（tree）→ 加载到目录区并切换焦点 |
| `o` | 打开挂载点（xdg-open） |

> **重要**：先按 `Space` 选中，再按 `Enter` 或 `m`/`c`/`d` 执行。移动光标会清除选中状态。

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
| `H` | 导出帮助到 `<data_dir>/HELP.md` |
| `Esc` / `?` / `q` | 关闭 |

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
│   ├── cli.rs               # 命令行参数解析
│   └── logger.rs            # 日志模块
├── shell/
│   ├── gocryptfs-cli        # CLI 主入口
│   └── lib/
│       └── gocryptfs-lib.sh # 全部库函数
├── test/
│   ├── create-test-env.sh
│   ├── cleanup-test-env.sh
│   ├── test-batch1.sh
│   ├── test-batch2.sh
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
make test          # Rust 单元测试
make test-env      # 生成 shell 测试环境
make test-cli      # 运行 shell 测试批次
make test-env-clean  # 清理测试环境

# 完整验证
make verify
```

### 本地安装

```bash
make install-local    # 装到 ~/.local/bin
make uninstall-local  # 卸载
```

### 全部 Make 目标

```bash
make help
```

---

## 发布

本项目使用 `cargo-release` + `cargo-dist` 组合：

- **cargo-release**：bump 版本、生成 CHANGELOG、打 tag、push
- **cargo-dist**：CI 多平台构建、生成 installer、创建 GitHub Release

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
# 1. 前置检查
make release-check

# 2. 预览（不执行）
make dry-run

# 3. 执行（patch 版本）
make release

# 或指定级别
make release LEVEL=minor
make release LEVEL=major

# 4. 查看 CI 进度
gh run watch

# 5. 验证 Release
gh release view v0.2.0 --web
```

push tag 后 GitHub Actions 自动触发 `.github/workflows/release.yml`，构建 4 个平台（gnu/musl × amd64/arm64）并创建 Release。

详细流程见 [RELEASING.md](RELEASING.md)。

---

## 许可

MIT License，见 [LICENSE](LICENSE)。