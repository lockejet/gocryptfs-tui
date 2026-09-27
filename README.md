# gocryptfs-tui

一个 gocryptfs 的 TUI + CLI 管理工具，用于在 Linux 上管理加密卷的挂载、卸载、创建和删除。

## 简介

`gocryptfs-tui` 提供两层界面：

- **CLI**（`gocryptfs-cli`）：功能完整的命令行工具，脚本友好
- **TUI**（`gocryptfs-tui`）：终端交互界面，日常操作直观

两者共享同一套核心逻辑，TUI 通过调用 CLI 完成所有实际工作，因此：

- CLI 与 TUI 的行为完全一致
- 可以在脚本或终端中独立使用 CLI
- TUI 只负责界面渲染和键盘事件，不做实际逻辑

## 特性

- 挂载 / 卸载加密卷（挂载点自动权限锁定）
- 从明文目录创建加密卷（含容量检查、断点续传、进度显示）
- 删除加密卷（还原明文、可选保留或删除加密后端）
- 支持全局设置与任务级覆盖（overrides）
- dry-run 预览模式
- 密码通过 stdin 传递，永不落盘
- 挂载点未挂载时自动 `chmod 555`（只读锁定），挂载时 `chmod 755`
- 删除加密时二次确认，未授权不会自动删除加密后端
- 支持 SMB 共享场景（挂载点始终存在）

## 依赖

- Rust（1.70+）—— 仅 TUI 需要
- `gocryptfs`
- `fusermount`
- `rsync`
- `yq`（https://github.com/mikefarah/yq）
- `jq`
- `tree`（可选，用于 `t` 树状视图）

Debian / Ubuntu 安装：

```
sudo apt install gocryptfs rsync jq
sudo wget -qO /usr/local/bin/yq https://github.com/mikefarah/yq/releases/latest/download/yq_linux_amd64
sudo chmod +x /usr/local/bin/yq
```

检查依赖：

```
gocryptfs-cli --check-deps
```

## 安装

### 方式一：一键安装（推荐）

```
cd gocryptfs-tui
cargo build --release
./install.sh
```

`install.sh` 会：

1. 检查依赖
2. 编译 TUI（若非 sudo 运行，会先自行检查）
3. 把 shell CLI 安装到 `/usr/local/lib/gocryptfs-tui/`
4. 把 `gocryptfs-cli` 和 `gocryptfs-tui` 符号链接到 `/usr/local/bin/`

> 注意：`install.sh` 不要用 `sudo` 运行（编译需要用户 cargo 环境），脚本内部会按需 `sudo`。

### 方式二：手动安装

```
cd gocryptfs-tui
cargo build --release

sudo mkdir -p /usr/local/lib/gocryptfs-tui/lib
sudo cp shell/lib/gocryptfs-lib.sh /usr/local/lib/gocryptfs-tui/lib/
sudo cp shell/gocryptfs-cli /usr/local/lib/gocryptfs-tui/
sudo chmod +x /usr/local/lib/gocryptfs-tui/gocryptfs-cli
sudo ln -sf /usr/local/lib/gocryptfs-tui/gocryptfs-cli /usr/local/bin/gocryptfs-cli
sudo cp target/release/gocryptfs-tui /usr/local/bin/
```

## 配置

### 配置文件位置

默认路径：

```
~/.config/gocryptfs-tui/config.yaml
```

可以用 `-c` 指定其他路径：

```
gocryptfs-cli -c /path/to/config.yaml list
```

### 配置示例

参考 `examples/config.yaml.example`，完整字段说明见文件内注释。

最小配置：

```
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

### 全局设置 vs 任务级覆盖

- `settings` 段：全局默认值
- 每个 vault 的 `overrides` 段：可覆盖全局（如某卷只读、某卷不用 `allow_other`）

生效顺序：

```
内置默认 < 全局 settings < 任务 overrides < 命令行参数
```

### 策略型选项

**只允许收紧，不允许在向导中放宽**：

| 配置项 | 值 | 含义 | 向导行为 |
|--------|----|------|---------|
| `create.keep_source` | `false` | 迁移后删除源（激进） | 可勾选保留 |
| `create.keep_source` | `true` | 永久保留源（保守） | 灰色只读 |
| `remove.restore` | `true` | 必须还原（保守） | 灰色只读 |
| `remove.restore` | `false` | 允许不还原（激进） | 可勾选还原 |
| `remove.direct_delete_cipher` | `true` | 允许删除加密后端（激进） | 可勾选取消 |
| `remove.direct_delete_cipher` | `false` | 保留加密后端（保守） | 灰色只读 |

## 使用

### CLI

```
gocryptfs-cli [-c <config>] <command> [options]
```

常用命令：

```
# 列出所有卷
gocryptfs-cli list

# JSON 输出（TUI 使用）
gocryptfs-cli list --json

# 卷详情
gocryptfs-cli info my_vault

# 挂载（密码通过 stdin）
echo 'your-password' | gocryptfs-cli mount my_vault

# 卸载
gocryptfs-cli umount my_vault

# 列出挂载点目录
gocryptfs-cli ls my_vault
gocryptfs-cli tree my_vault

# 从明文目录创建加密卷（dry-run 预览）
gocryptfs-cli create /srv/photos --name photos --dry-run

# 真实创建（--yes 跳过二次确认）
echo 'your-password' | gocryptfs-cli create /srv/photos --name photos --yes

# 删除加密
echo 'your-password' | gocryptfs-cli remove photos --yes

# 保留加密后端
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
| 3 | 状态错误（已挂载 / 未挂载） |
| 4 | 挂载点问题 |
| 5 | 磁盘空间不足 |
| 6 | 卸载失败 |
| 7 | 强制卸载失败 |
| 8 | 配置错误 |

### TUI

```
gocryptfs-tui
```

两个页面，`Tab` 切换：

- **[1] 挂载/卸载** —— 日常操作
- **[2] 创建/删除** —— 生命周期管理

通用按键：

| 键 | 功能 |
|----|------|
| `Tab` | 切换页面 |
| `1` / `2` | 直接跳页 |
| `s` | 设置浮层 |
| `h` | 历史浮层 |
| `e` | 外部编辑器打开配置 |
| `r` | 刷新 |
| `?` | 帮助 |
| `q` | 退出 |

页面 1：

| 键 | 功能 |
|----|------|
| `j` / `k` | 移动选择 |
| `m` / `Space` | 挂载 |
| `u` | 卸载 |
| `l` | 列表（ls -la） |
| `t` | 树状（tree） |

页面 2：

| 键 | 功能 |
|----|------|
| `j` / `k` | 移动选择 |
| `c` | 创建模式 |
| `d` | 删除模式 |
| `Enter` / `Space` | 进入向导 |
| `l` / `t` | 列表 / 树状（仅删除模式） |

## 安全设计

- **密码永不落盘**：密码通过 stdin 传递给 CLI，CLI 内部用临时文件（权限 600）传给 gocryptfs，用完立即删除
- **配置不含密码**
- **挂载点保护**：未挂载时自动 `chmod 555`（保留 setgid 等特殊位），挂载时 `chmod 755`
- **不自动解密**：挂载必须由用户手动触发并提供密码
- **删除加密需二次确认**

报告漏洞请见 [SECURITY.md](SECURITY.md)。

## 项目结构

```
gocryptfs-tui/
├── Cargo.toml
├── install.sh
├── README.md
├── LICENSE
├── SECURITY.md
├── CHANGELOG.md
├── shell/
│   ├── gocryptfs-cli           # CLI 主入口
│   └── lib/
│       └── gocryptfs-lib.sh    # 全部库函数（单文件）
├── test/
│   ├── create-test-env.sh      # 生成测试环境
│   ├── cleanup-test-env.sh     # 强制清理
│   ├── test-batch1.sh          # list/info/ls/tree
│   ├── test-batch2.sh          # mount/umount/create/remove
│   └── test-all.sh             # 汇总
├── src/
│   └── main.rs                 # Rust TUI
└── examples/
    └── config.yaml.example
```

## 测试

```
# 生成测试环境（含加密卷初始化）
bash test/create-test-env.sh

# 运行所有测试
bash test/test-all.sh

# 强制清理测试环境
bash test/cleanup-test-env.sh
```

## 许可

MIT License，见 [LICENSE](LICENSE)。
