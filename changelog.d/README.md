# changelog.d — 手写 CHANGELOG 片段

`CHANGELOG.md` 由 git-cliff 从提交历史生成，**直接手改会在下次生成时丢失**。
需要写"提交标题表达不了"的内容（长篇说明、升级注意、多条要点）时，在这里放一个片段文件。

- `make changelog` / `make changelog-preview`：把片段合并进 `[Unreleased]` 段，**片段保留**
  （因此反复生成结果稳定，不会丢内容）。
- `make release`（或 `make changelog TAG=v0.2.0`）：把片段并入该版本段，并归档到
  `changelog.d/archive/`；归档目录只是留档，不会被再次合并。

## 片段格式

文件名：`YYYY-MM-DD-<主题>.md`（例如 `2026-10-03-i18n.md`）。

```markdown
### Features
- 新增 xxx
- 新增 yyy

### Bug Fixes
- 修复 zzz
```

- 用 `### 分组名` 开头；组名与 git-cliff 一致（`Features` / `Bug Fixes` / `Documentation` /
  `Performance` / `Refactor` / `Styling` / `Testing` / `Miscellaneous` / `Build`）时会并入同一组。
- 没有 `###` 的纯列表会归入 `Miscellaneous`。
- 不要写 `##`（版本标题由生成器负责）。
- `README.md`、`_` 开头的文件、`archive/` 目录不会被当成片段。

## 与提交的关系（避免重复）

| 场景 | 做法 |
|------|------|
| 普通改动 | 只写规范提交（`feat:` / `fix:` / `docs:` …），`make changelog` 自动收录 |
| 需要长说明的改动 | 写片段；该改动的提交信息里加 `[skip changelog]`，避免同一件事出现两条 |

`cliff.toml` 已配置：含 `[skip changelog]` 的提交会被跳过。

## 常用命令

```bash
make changelog-check      # 校验片段语法
make changelog-preview    # 只预览合并结果到 .staging/，不改动 CHANGELOG.md
make changelog            # 正式生成（重写 CHANGELOG.md，片段保留）
make changelog TAG=v0.2.0 # 生成指定版本段（发布用；片段会归档）
```
