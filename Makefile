# ============================================================
# gocryptfs-tui Makefile
# ============================================================
# 详细发布流程见 RELEASING.md
# 查看所有目标: make help
# 注意: 本文件使用 TAB 缩进，复制时请勿将 TAB 替换为空格
# ============================================================

BIN        := gocryptfs-tui
BUILD_DIR  := target/release
DIST_DIR   := dist
CHANGELOG          := CHANGELOG.md
CHANGELOG_DIR      := changelog.d
CHANGELOG_STAGE    := .staging/CHANGELOG.generated.md
CHANGELOG_PREVIEW  := .staging/CHANGELOG.preview.md

# ---------- 版本（从 git tag 推导）----------
BUILD_VERSION := $(shell git describe --tags --always --dirty 2>/dev/null || echo dev)
VERSION       ?= $(shell git describe --tags --abbrev=0 2>/dev/null || echo dev)
PKG_VERSION   ?= $(VERSION)
PKG_NAME      := $(BIN)-$(PKG_VERSION)

# ---------- 发布参数 ----------
REPO    ?= $(shell git remote get-url origin 2>/dev/null | \
             sed -E 's|^[^:]+://[^/]+/||; s|^git@[^:]+:||; s|\.git$$||')
LEVEL   ?= patch         # cargo release 的升级级别: patch / minor / major

# ---------- 安装路径（install-local 用）----------
PREFIX  ?= $(HOME)/.local
BINDIR  ?= $(PREFIX)/bin
# Shell 后端与 TUI 必须成套安装，否则 TUI 会调用旧的 gocryptfs-cli（输出语言不一致）
SHELL_LIBDIR ?= $(PREFIX)/lib/gocryptfs-tui
SHELL_BINDIR ?= $(BINDIR)
# 系统级安装（install-system / uninstall-system 用）
SYSTEM_PREFIX ?= /usr/local

# ---------- 交叉编译目标 ----------
CROSS_TARGETS := aarch64-unknown-linux-gnu \
                 x86_64-unknown-linux-musl \
                 aarch64-unknown-linux-musl

.DEFAULT_GOAL := build

# ============================================================
# 构建
# ============================================================

.PHONY: build
build:  ## 编译本机 release 二进制（默认目标）
	cargo build --release
	@echo ">>> $(BUILD_DIR)/$(BIN)"

.PHONY: build-debug
build-debug:  ## 编译本机 debug 二进制（快速迭代）
	cargo build
	@echo ">>> target/debug/$(BIN)"

.PHONY: build-arm64
build-arm64:  ## 交叉编译 Linux arm64（gnu，需要 cross）
	@command -v cross >/dev/null 2>&1 || { \
		echo "错误: 未安装 cross。安装: cargo install cross --git https://github.com/cross-rs/cross"; \
		exit 1; \
	}
	cross build --release --target aarch64-unknown-linux-gnu
	@echo ">>> target/aarch64-unknown-linux-gnu/release/$(BIN)"

.PHONY: build-musl
build-musl:  ## 交叉编译 Linux amd64 musl（静态，需要 cross）
	@command -v cross >/dev/null 2>&1 || { \
		echo "错误: 未安装 cross"; exit 1; \
	}
	cross build --release --target x86_64-unknown-linux-musl
	@echo ">>> target/x86_64-unknown-linux-musl/release/$(BIN)"

.PHONY: build-arm64-musl
build-arm64-musl:  ## 交叉编译 Linux arm64 musl（静态，需要 cross）
	@command -v cross >/dev/null 2>&1 || { \
		echo "错误: 未安装 cross"; exit 1; \
	}
	cross build --release --target aarch64-unknown-linux-musl
	@echo ">>> target/aarch64-unknown-linux-musl/release/$(BIN)"

.PHONY: build-all
build-all:  ## 交叉编译全部目标（gnu/musl × amd64/arm64）
	@for t in $(CROSS_TARGETS); do \
		echo ">>> cross build --release --target $$t"; \
		cross build --release --target $$t || exit 1; \
	done
	@echo ">>> 全部完成"

# ============================================================
# 检查与测试
# ============================================================

.PHONY: check
check: fmt-check lint i18n-check changelog-check test  ## 运行全部检查：fmt + clippy + i18n + changelog + 单元测试

.PHONY: i18n-check
i18n-check:  ## 校验 i18n 键表、调用点参数与中文残留
	python3 tools/i18n_check.py

.PHONY: lint
lint:  ## 运行 clippy（-D warnings）
	cargo clippy --all-targets --all-features -- -D warnings

.PHONY: fmt
fmt:  ## 格式化代码
	cargo fmt --all

.PHONY: fmt-check
fmt-check:  ## 检查代码格式（不修改）
	cargo fmt --all -- --check

.PHONY: test
test:  ## 运行 Rust 单元测试
	cargo test

.PHONY: test-i18n
test-i18n: build-debug  ## 运行 i18n 验收测试（TUI + Shell 语言解析矩阵）
	bash test/test-i18n.sh

.PHONY: test-cli
test-cli: build  ## 运行 CLI shell 测试批次
	bash test/test-all.sh

.PHONY: test-install
test-install: build  ## 安装/卸载回归测试（install.sh，本地源码模式）
	bash test/test-install.sh

.PHONY: test-env
test-env:  ## 生成 shell 测试环境（/tmp/gocryptfs-tui-test/）
	bash test/create-test-env.sh

.PHONY: test-env-clean
test-env-clean:  ## 清理 shell 测试环境
	bash test/create-test-env.sh --clean
	bash test/cleanup-test-env.sh

.PHONY: verify
verify: check test-i18n test-cli test-install  ## 完整验证：静态检查 + 单元测试 + i18n + CLI/安装测试
	@echo ">>> 验证二进制:"
	@$(BUILD_DIR)/$(BIN) --version || true
	@echo ">>> 全部通过"

# ============================================================
# 版本与 CHANGELOG
# ============================================================

.PHONY: changelog
.PHONY: _changelog-gen
_changelog-gen:  # 内部：git-cliff 生成到 .staging/（TAG 非空则用该标签命名段落）
	@command -v git-cliff >/dev/null 2>&1 || { \
		echo "错误: 未安装 git-cliff。安装: cargo install git-cliff"; \
		exit 1; \
	}
	@mkdir -p "$(dir $(CHANGELOG_STAGE))"
	@if [ -n "$(TAG)" ]; then echo ">>> 段落标题: $(TAG)"; git cliff --tag "$(TAG)" -o $(CHANGELOG_STAGE); \
	else echo ">>> 段落标题: [unreleased]"; git cliff -o $(CHANGELOG_STAGE); fi

.PHONY: changelog-check
changelog-check:  ## 校验 changelog.d/ 片段格式与合并逻辑
	@python3 tools/changelog_fragments.py --selftest
	@python3 tools/changelog_fragments.py check --fragments $(CHANGELOG_DIR)

.PHONY: changelog
changelog: _changelog-gen  ## 显式生成 CHANGELOG.md（重写文件；片段保留，发布时才归档）
	@echo ">>> 注意: 将重写 $(CHANGELOG)（先看差异可用 make changelog-preview）"
	python3 tools/changelog_fragments.py merge \
		--generated $(CHANGELOG_STAGE) --fragments $(CHANGELOG_DIR) \
		--merge-into $(if $(TAG),first,unreleased) --tag "$(TAG)" \
		$(if $(ARCHIVE_FRAGMENTS),--archive $(CHANGELOG_DIR)/archive,) \
		--output $(CHANGELOG)

.PHONY: changelog-preview
changelog-preview: _changelog-gen  ## 预览合并结果到 .staging/（不改动 CHANGELOG.md、不归档片段）
	python3 tools/changelog_fragments.py merge \
		--generated $(CHANGELOG_STAGE) --fragments $(CHANGELOG_DIR) \
		--merge-into $(if $(TAG),first,unreleased) --tag "$(TAG)" \
		--output $(CHANGELOG_PREVIEW)
	@echo ">>> 预览: $(CHANGELOG_PREVIEW)"
	@echo "    对比: git diff --no-index -- $(CHANGELOG) $(CHANGELOG_PREVIEW)"

.PHONY: version
version:  ## 显示版本相关信息（从 git 推导）
	@echo "BUILD_VERSION = $(BUILD_VERSION)"
	@echo "VERSION       = $(VERSION)"
	@echo "PKG_VERSION   = $(PKG_VERSION)"
	@echo "PKG_NAME      = $(PKG_NAME)"
	@echo "REPO          = $(REPO)"

# ============================================================
# 发布
# ============================================================

.PHONY: dry-run
dry-run:  ## 预览 cargo-release 动作（只读，不会改动任何文件；默认 LEVEL=patch）
	cargo release $(LEVEL)

.PHONY: release
release: check release-check  ## 生成并提交 CHANGELOG（含片段）后发布（bump + tag + push）
	@next=$$(cargo release version $(LEVEL) 2>&1 | sed -n 's/.*Upgrading .* to \([0-9][^ ]*\).*/\1/p' | tail -1); \
	if [ -n "$$next" ]; then \
		echo ">>> 下一版本: v$$next"; \
		$(MAKE) --no-print-directory changelog TAG="v$$next" ARCHIVE_FRAGMENTS=1; \
	else \
		echo "[!] 未能解析下一版本，CHANGELOG 段落将使用 [Unreleased]"; \
		$(MAKE) --no-print-directory changelog; \
	fi
	@git add $(CHANGELOG) $(CHANGELOG_DIR)
	@if ! git diff --cached --quiet; then \
		git commit -m "docs: 更新 CHANGELOG"; \
		echo ">>> 已提交 CHANGELOG（含片段归档）"; \
	else \
		echo ">>> CHANGELOG 无变化，无需提交"; \
	fi
	cargo release $(LEVEL) --execute

.PHONY: release-check
release-check:  ## 检查发布前置条件（除 CHANGELOG.md 外工作区干净、gh 已登录）
	@git diff --quiet -- . ':(exclude)$(CHANGELOG)' || { \
		echo "错误: 工作区有未提交改动（$(CHANGELOG) 除外）"; exit 1; }
	@git diff --cached --quiet || { echo "错误: 有已暂存未提交改动"; exit 1; }
	@[ -n "$(REPO)" ] || { echo "错误: 无法从 git remote 解析仓库"; exit 1; }
	@command -v gh >/dev/null 2>&1 || { echo "错误: 未安装 gh CLI"; exit 1; }
	@gh auth status >/dev/null 2>&1 || { echo "错误: gh 未登录"; exit 1; }
	@echo ">>> 前置检查通过"
	@echo "    REPO = $(REPO)"
	@echo "    当前版本 = $(VERSION)"
	@next=$$(cargo release version $(LEVEL) 2>&1 | sed -n 's/.*Upgrading .* to \([0-9][^ ]*\).*/\1/p' | tail -1); \
	echo "    下一版本 = $${next:-未知（请手动确认）}"

.PHONY: dist-build
dist-build:  ## 用 cargo-dist 本地构建所有平台（生成 dist/ 产物）
	@command -v dist >/dev/null 2>&1 || { \
		echo "错误: 未安装 cargo-dist。安装: cargo install cargo-dist --locked"; \
		exit 1; \
	}
	dist build
	@echo ">>> dist/"

.PHONY: dist-plan
dist-plan:  ## 预览 cargo-dist 构建计划
	dist plan

# ============================================================
# 安装
# ============================================================

.PHONY: install
install: install-local  ## 安装到 $(PREFIX)（默认 ~/.local，等价 install-local）

.PHONY: install-local
install-local: build  ## 安装到 $(PREFIX)（默认 ~/.local；TUI + Shell 后端成套安装）
	@install -d "$(BINDIR)" "$(SHELL_LIBDIR)/lib"
	install -m 0755 "$(BUILD_DIR)/$(BIN)" "$(BINDIR)/$(BIN)"
	install -m 0644 shell/lib/*.sh "$(SHELL_LIBDIR)/lib/"
	install -m 0755 shell/gocryptfs-cli "$(SHELL_LIBDIR)/gocryptfs-cli"
	ln -sf "$(SHELL_LIBDIR)/gocryptfs-cli" "$(SHELL_BINDIR)/gocryptfs-cli"
	@echo ">>> 已安装到 $(BINDIR)/$(BIN)"
	@echo ">>> 已安装 shell 后端: $(SHELL_BINDIR)/gocryptfs-cli -> $(SHELL_LIBDIR)/gocryptfs-cli"
	@echo "    确保 $(BINDIR) 在 PATH 中且在 /usr/local/bin 之前（避免调用到旧版 CLI）"

.PHONY: install-system
install-system:  ## 安装到 $(SYSTEM_PREFIX)（默认 /usr/local，内部按需 sudo）
	bash install.sh --prefix "$(SYSTEM_PREFIX)"

.PHONY: uninstall
uninstall:  ## 卸载 $(PREFIX) 中的安装（默认 ~/.local；改 PREFIX=/usr/local 可卸系统安装）
	@case "$(SHELL_LIBDIR)" in */lib/gocryptfs-tui) ;; *) \
		echo "[!] 拒绝删除异常路径: $(SHELL_LIBDIR)"; exit 1;; esac
	@[ "$(PREFIX)" != "/" ] || { echo "[!] PREFIX 不能是 /"; exit 1; }
	-rm -f "$(BINDIR)/$(BIN)"
	-rm -f "$(SHELL_BINDIR)/gocryptfs-cli"
	-rm -rf "$(SHELL_LIBDIR)"
	@echo ">>> 已卸载 $(BINDIR)/$(BIN) 与 $(SHELL_LIBDIR)"
	@resolved="$$(command -v gocryptfs-cli 2>/dev/null || true)"; \
	if [ -n "$$resolved" ]; then \
		echo "    提示: PATH 中仍能解析到 $$resolved（可能是另一份旧安装）"; \
	fi

.PHONY: uninstall-local
uninstall-local: uninstall  ## 兼容别名，等价 make uninstall

.PHONY: uninstall-system
uninstall-system:  ## 卸载 $(SYSTEM_PREFIX) 中的安装（默认 /usr/local，内部按需 sudo）
	bash install.sh --uninstall --prefix "$(SYSTEM_PREFIX)"

# ============================================================
# 清理
# ============================================================

.PHONY: clean
clean:  ## 清空所有编译产物（target/ + dist/ + .staging/）
	cargo clean
	rm -rf $(DIST_DIR) .staging
	@echo ">>> 已清理"

.PHONY: dist-clean
dist-clean:  ## 仅清空 dist/ 和 .staging/
	rm -rf $(DIST_DIR) .staging
	@echo ">>> 已清理 dist/"

# ============================================================
# 帮助
# ============================================================

.PHONY: help
help:  ## 显示本帮助
	@echo ""
	@echo "gocryptfs-tui 构建入口"
	@echo "======================"
	@echo ""
	@echo "当前状态:"
	@echo "  版本:   $(BUILD_VERSION)"
	@echo "  仓库:   $(REPO)"
	@echo ""
	@echo "构建:"
	@echo "  make build             编译本机 release（默认）"
	@echo "  make build-debug       编译本机 debug"
	@echo "  make build-arm64       交叉编译 arm64 gnu（需 cross）"
	@echo "  make build-musl        交叉编译 amd64 musl（需 cross）"
	@echo "  make build-arm64-musl  交叉编译 arm64 musl（需 cross）"
	@echo "  make build-all         交叉编译全部三个目标"
	@echo ""
	@echo "检查与测试:"
	@echo "  make check             fmt + clippy + 单元测试"
	@echo "  make lint              clippy（-D warnings）"
	@echo "  make fmt               格式化代码"
	@echo "  make fmt-check         检查格式（不修改）"
	@echo "  make test              Rust 单元测试"
	@echo "  make test-cli          运行 CLI shell 测试批次"
	@echo "  make test-install      运行安装/卸载回归测试"
	@echo "  make test-env          生成 shell 测试环境"
	@echo "  make test-env-clean    清理 shell 测试环境"
	@echo "  make verify            完整验证（check + test-cli + 二进制自检）"
	@echo ""
	@echo "版本与 CHANGELOG（均为显式操作，dry-run 不会改动文件）:"
	@echo "  make version           显示 git 推导的版本信息"
	@echo "  make changelog         生成/重写 CHANGELOG.md（合并并归档 changelog.d/ 片段）"
	@echo "  make changelog-preview 仅生成预览到 .staging/（不动 CHANGELOG.md）"
	@echo "  make changelog-check   校验 changelog.d/ 片段格式与合并逻辑"
	@echo ""
	@echo "发布:"
	@echo "  make dry-run           预览 cargo-release 动作（只读，不改文件）"
	@echo "  make release           生成并提交 CHANGELOG，然后 cargo-release"
	@echo "  make release-check     检查发布前置条件"
	@echo "  make dist-plan         预览 cargo-dist 构建计划"
	@echo "  make dist-build        本地构建所有平台产物到 dist/"
	@echo ""
	@echo "  发布参数:"
	@echo "    LEVEL=patch|minor|major   默认 patch"
	@echo "    例: make release LEVEL=minor"
	@echo ""
	@echo "安装:"
	@echo "  make install           安装到 $(PREFIX)（TUI + Shell 后端）"
	@echo "  make install-system    安装到 $(SYSTEM_PREFIX)（按需 sudo）"
	@echo "  make uninstall         卸载 $(PREFIX) 中的安装"
	@echo "  make uninstall-system  卸载 $(SYSTEM_PREFIX) 中的安装"
	@echo ""
	@echo "清理:"
	@echo "  make clean             清空所有产物"
	@echo "  make dist-clean        仅清空 dist/"
	@echo ""
	@echo "详细流程见 RELEASING.md"
	@echo ""