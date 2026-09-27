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
- [配置](#配置)
- [使用](#使用)
- [架构](#架构)
- [安全设计](#安全设计)
- [开发与测试](#开发与测试)
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

---

## 依赖

运行时依赖：

- `gocryptfs`
- `fusermount`（`gocryptfs` 自带或系统提供）
- `rsync`
- `yq`（[mikefarah/yq](https://github.com/mikefarah/yq)）
- `jq`
- `tree`（可选，用于 `t` 树状视图）

编译 TUI 需要：

- Rust 1.70+

Debian / Ubuntu 安装：

```bash
sudo apt install gocryptfs rsync jq
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

### 方式一：一键安装（推荐）

```bash
cd /opt/vault-tui
cargo build --release
./install.sh
```

`install.sh` 会：

1. 检查运行时依赖
2. 编译 TUI（以当前用户身份）
3. 将 shell CLI 安装到 `/usr/local/lib/gocryptfs-tui/`
4. 将 `gocryptfs-cli` 和 `gocryptfs-tui` 符号链接到 `/usr/local/bin/`

> 注意：`install.sh` **不要用 `sudo` 运行**（编译需要用户 cargo 环境），脚本内部会按需 `sudo`。

### 方式二：手动安装

```bash
cd /opt/vault-tui
cargo build --release

sudo mkdir -p /usr/local/lib/gocryptfs-tui/lib
sudo cp shell/lib/gocryptfs-lib.sh /usr/local/lib/gocryptfs-tui/lib/
sudo cp shell/gocryptfs-cli /usr/local/lib/gocryptfs-tui/
sudo chmod +x /usr/local/lib/gocryptfs-tui/gocryptfs-cli
sudo ln -sf /usr/local/lib/gocryptfs-tui/gocryptfs-cli /usr/local/bin/gocryptfs-cli
sudo cp target/release/gocryptfs-tui /usr/local/bin/
sudo chmod +x /usr/local/bin/gocryptfs-tui
```

### 卸载

```bash
sudo rm -rf /usr/local/lib/gocryptfs-tui
sudo rm -f /usr/local/bin/gocryptfs-cli
sudo rm -f /usr/local/bin/gocryptfs-tui
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

#### 全局按键（所有页面）

| 键 | 功能 |
|----|------|
| `Tab` | 换页 |
| `1` / `2` / `3` | 直接跳页 |
| `Alt+1` | 焦点到列表 |
| `Alt+2` | 焦点到详情 |
| `Alt+3` | 焦点到目录 |
| `Alt+4` | 焦点到输出 |
| `s` | 设置浮层 |
| `h` | 历史浮层 |
| `e` | 外部编辑器打开配置 |
| `r` | 刷新 |
| `?` | 帮助 |
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
| `l` | 列表（ls -la）→ 加载到目录区并切换焦点 |
| `t` | 树状（tree）→ 加载到目录区并切换焦点 |
| `o` | 打开挂载点（xdg-open） |

> **重要**：先按 `Space` 选中，再按 `Enter` 或 `m` 执行。移动光标会清除选中状态。

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
├── install.sh
├── README.md
├── LICENSE
├── SECURITY.md
├── CHANGELOG.md
├── .gitignore
├── src/
│   └── main.rs              # TUI 全部代码
├── shell/
│   ├── gocryptfs-cli        # CLI 主入口
│   └── lib/
│       └── gocryptfs-lib.sh # 全部库函数
├── test/
│   ├── create-test-env.sh   # 生成/清理测试环境
│   ├── cleanup-test-env.sh  # 强制清理残留
│   ├── test-batch1.sh       # list/info/ls/tree 测试
│   ├── test-batch2.sh       # mount/umount/create/remove 测试
│   └── test-all.sh          # 汇总入口
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

详细安全说明见 [SECURITY.md](SECURITY.md)。

---

## 开发与测试

### 编译

```bash
cargo build --release
```

### 生成测试环境

```bash
# 生成（含加密卷初始化、pending 示例）
bash test/create-test-env.sh

# 只清理
bash test/create-test-env.sh --clean

# 显式声明先清理再生成
bash test/create-test-env.sh --clean-first

# 保留旧环境
bash test/create-test-env.sh --keep
```

### 运行所有测试

```bash
bash test/test-all.sh
```

或单独运行：

```bash
bash test/test-batch1.sh
bash test/test-batch2.sh
```

### 强制清理残留

```bash
bash test/cleanup-test-env.sh
```

---

## 许可

MIT License，见 [LICENSE](LICENSE)。
