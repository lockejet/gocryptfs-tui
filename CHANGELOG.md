# 更新日志

本项目遵循 [Keep a Changelog](https://keepachangelog.com/) 和
[语义化版本](https://semver.org/)。

## [Unreleased]

### 计划

- 向导支持中止正在运行的 CLI（信号处理）
- 设置浮层就地编辑（不依赖外部编辑器）
- 待处理目录交互式添加
- 历史浮层详情视图
- 支持 `gocryptfs -reverse` 反向模式

## [0.1.0] - 2026-09-27

### 新增

#### CLI（`gocryptfs-cli`）

- `list` / `info` —— 列出卷、显示卷详情
- `mount` / `umount` —— 挂载、卸载（支持 `--force` 强制卸载）
- `ls` / `tree` —— 列出挂载点目录
- `create` —— 从明文目录创建加密卷
  - 容量检查
  - 断点续传（检测已存在的加密后端）
  - 进度输出（`@@PROGRESS@@` 协议行）
  - `--dry-run` 预览
  - `--keep-source` 保留源文件
- `remove` —— 删除加密卷
  - 自动挂载（若未挂载）
  - 还原明文
  - `--keep-cipher` / `--delete-cipher`
  - `--dry-run` 预览
- `config` / `edit` —— 显示配置路径、外部编辑器打开
- `check-deps` —— 检查依赖

#### TUI（`gocryptfs-tui`）

- 两页面结构：`[1] 挂载/卸载` 与 `[2] 创建/删除`，`Tab` 切换
- 卷列表、详情、输出区三段布局
- 挂载 / 卸载（密码输入框，Backspace / Delete / Ctrl+H 均可删除）
- 创建 / 删除向导（多步状态机）
- 目录视图（`l` / `t`，支持 `PgDn` / `PgUp` / `g` / `G` 滚动）
- 设置浮层（`s`）：gocryptfs / rsync / filters / 权限
- 历史浮层（`h`）：读 `history.jsonl`
- 外部编辑器（`e`）：退出 TUI 启动 `$EDITOR`
- 帮助浮层（`?`）

#### 安全

- 密码通过 stdin 传递，**永不落盘**
- 挂载点未挂载时自动 `chmod 555`（只读锁定）
- 挂载时 `chmod 755`
- 删除加密默认保留加密后端
- 配置策略型选项（保守值在向导中灰色只读，激进值可收紧）

#### 测试

- `test/create-test-env.sh` —— 生成测试环境（含加密卷初始化、pending 示例）
- `test/cleanup-test-env.sh` —— 强制清理所有挂载和目录
- `test/test-batch1.sh` —— list / info / ls / tree 测试
- `test/test-batch2.sh` —— mount / umount / create / remove 测试
- `test/test-all.sh` —— 汇总入口

### 已知限制

- 向导无法中止正在运行的 CLI（需等待完成）
- 设置浮层为只读展示，修改需按 `e` 打开外部编辑器
- 待处理目录的交互式添加未实现（需编辑配置文件）
- 挂载点的权限切换依赖命令行 `chmod`（不支持 `chattr`，因其会阻止 gocryptfs 挂载）

[Unreleased]: https://github.com/lockejet/gocryptfs-tui/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/lockejet/gocryptfs-tui/releases/tag/v0.1.0
