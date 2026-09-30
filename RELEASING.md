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