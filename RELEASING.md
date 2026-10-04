# 发版流程

本文档描述 gocryptfs-tui 从开发到发布 GitHub Release 的完整流程。

## 目录

- [工具链](#工具链)
- [一次性初始化](#一次性初始化)
- [日常开发](#日常开发)
- [发布新版本](#发布新版本)
- [CI 自动构建](#ci-自动构建)
- [验证 Release](#验证-release)
- [回滚](#回滚)
- [故障排查](#故障排查)

---

## 工具链

### 必需

| 工具 | 用途 | 安装 |
|------|------|------|
| `cargo-release` | bump 版本、打 tag、push | `cargo install cargo-release --locked` |
| `git-cliff` | 从 git log 生成 CHANGELOG | `cargo install git-cliff --locked` |
| `cargo-dist` | 多平台打包、创建 GitHub Release | `cargo install cargo-dist --locked` |
| `gh` | GitHub CLI，用于查看/操作 Release | `sudo apt install gh` 或 https://cli.github.com/ |

### 可选

| 工具 | 用途 | 安装 |
|------|------|------|
| `cross` | 本地交叉编译（arm64 / musl） | `cargo install cross --git https://github.com/cross-rs/cross` |

### 版本检查

```bash
cargo release --version
git-cliff --version
dist --version          # 注意：不是 cargo dist
gh --version

---

## CHANGELOG 与发布流程

**CHANGELOG.md 不会被自动改写。** `Cargo.toml` 中没有配置 `pre-release-hook`，
因此 `make dry-run` / `cargo release --dry-run` 不会产生任何文件写入。

| 命令 | 行为 |
|------|------|
| `make changelog-preview` | 只生成预览到 `.staging/CHANGELOG.preview.md`（`.staging/` 已 gitignore），**不动** `CHANGELOG.md` |
| `make changelog` | **显式重写** `CHANGELOG.md`：git-cliff 生成 + 合并 `changelog.d/` 片段（片段保留）；默认 `[Unreleased]` 段，`make changelog TAG=v0.2.0` 生成指定版本段并归档片段 |
| `make dry-run` | 只读预览 cargo-release 动作，不改动任何文件 |
| `make release` | `check` → `release-check` → `changelog` → 提交 `CHANGELOG.md` → `cargo release --execute` |

### 发布步骤

```bash
# 1. 确认代码与测试
make check

# 2. 先看 CHANGELOG 会变成什么（可选但推荐）
make changelog-preview
git diff --no-index -- CHANGELOG.md .staging/CHANGELOG.preview.md

# 3. 前置检查（除 CHANGELOG.md 外工作区必须干净）
make release-check

# 4. 预览 cargo-release（只读）
make dry-run

# 5. 一步发布（生成并提交 CHANGELOG，然后 bump + tag + push）
make release LEVEL=patch

# 6. 查看 CI 与 Release
gh run watch
gh release view v0.1.3 --web
```

### 只想更新 CHANGELOG、不发布

```bash
make changelog
git add CHANGELOG.md
git commit -m "docs: 更新 CHANGELOG"
```

> 之所以去掉 `pre-release-hook`：cargo-release 的 hook 会在此前的 `--dry-run`
> 中一并执行，导致"预览"也会重写 `CHANGELOG.md`（冲掉手工维护的 Unreleased 段落）。

### 手写内容：changelog.d/ 片段

提交标题表达不了的内容（长篇说明、升级注意）写成片段文件，随代码一起提交：

```bash
# 新增片段
cat > changelog.d/2026-10-03-my-change.md <<'EOF'
### Features
- 新增 xxx（详细说明……）
EOF

make changelog-check      # 校验片段语法
make changelog-preview    # 预览合并结果（不动 CHANGELOG.md）
make changelog            # 合并进 [Unreleased]（片段保留，可反复生成）
```

- 组名用 `Features` / `Bug Fixes` / `Documentation` / `Build` 等，与 git-cliff 同名会自动合并。
- 同一改动若已写片段，提交信息里加 `[skip changelog]`，避免 CHANGELOG 出现两条重复条目
  （`cliff.toml` 已配置跳过）。
- 发布时（`make release`）片段会并入新版本段并归档到 `changelog.d/archive/`。
- 详情见 `changelog.d/README.md`。

---

## 发布后检查（依赖与安装说明）

cargo-dist 会用 **CHANGELOG 生成 GitHub Release 说明**，因此运行时依赖清单随
`changelog.d/` 片段进入 Release 页面（shell 安装器本身不能提示系统依赖）。

发布后确认一次：

```bash
gh release view v0.3.0 --json body --jq .body | grep -A 3 "运行时依赖"
```

若说明里缺依赖清单（例如片段没写），手工补一段：

```bash
body="$(gh release view v0.3.0 --json body --jq .body)"
printf '%s\n\n## 运行时依赖\n\n- gocryptfs / fusermount(fuse3) / rsync / yq(Go v4) / jq / mountpoint(util-linux)\n- 可选: tree\n- 检查: `gocryptfs-tui --check-deps`\n' "$body" \
  | gh release edit v0.3.0 --notes-file -
```

同时确认两个安装脚本都在 Release 里，并冒烟一次（不需要源码）：

```bash
gh release view v0.3.0 --json assets --jq '.assets[].name' | grep -E 'installer\.sh|install\.sh'

curl -LsSf https://github.com/lockejet/gocryptfs-tui/releases/latest/download/gocryptfs-tui-install.sh \
  | sh -s -- --prefix /tmp/gocryptfs-tui-smoke --link-cli
/tmp/gocryptfs-tui-smoke/bin/gocryptfs-tui --version
/tmp/gocryptfs-tui-smoke/bin/gocryptfs-cli --version
curl -LsSf https://github.com/lockejet/gocryptfs-tui/releases/latest/download/gocryptfs-tui-install.sh \
  | sh -s -- --uninstall --prefix /tmp/gocryptfs-tui-smoke
```

确认安装落点已切到 `~/.local/bin`（由 CI 在发布时生成安装脚本）：

```bash
curl -LsSf https://github.com/lockejet/gocryptfs-tui/releases/latest/download/gocryptfs-tui-installer.sh \
  | grep -m1 '_install_dir='
```
