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
check: fmt-check lint test  ## 运行全部检查：fmt + clippy + 单元测试

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

.PHONY: test-cli
test-cli: build  ## 运行 CLI shell 测试批次
	bash test/test-all.sh

.PHONY: test-env
test-env:  ## 生成 shell 测试环境（/tmp/gocryptfs-tui-test/）
	bash test/create-test-env.sh

.PHONY: test-env-clean
test-env-clean:  ## 清理 shell 测试环境
	bash test/create-test-env.sh --clean
	bash test/cleanup-test-env.sh

.PHONY: verify
verify: check test-cli  ## 完整验证：静态检查 + 单元测试 + CLI 测试
	@echo ">>> 验证二进制:"
	@$(BUILD_DIR)/$(BIN) --version || true
	@echo ">>> 全部通过"

# ============================================================
# 版本与 CHANGELOG
# ============================================================

.PHONY: changelog
changelog:  ## 用 git-cliff 生成 CHANGELOG.md
	@command -v git-cliff >/dev/null 2>&1 || { \
		echo "错误: 未安装 git-cliff。安装: cargo install git-cliff"; \
		exit 1; \
	}
	git-cliff -o CHANGELOG.md
	@echo ">>> CHANGELOG.md"

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
dry-run:  ## 预览 cargo-release 动作（不执行，默认 LEVEL=patch）
	cargo release $(LEVEL)

.PHONY: release
release: check  ## 执行 cargo-release（bump + changelog + tag + push，触发 CI）
	cargo release $(LEVEL) --execute

.PHONY: release-check
release-check:  ## 检查发布前置条件（git 干净、gh 已登录、tag 可用）
	@git diff --quiet || { echo "错误: 工作区有未提交改动"; exit 1; }
	@git diff --cached --quiet || { echo "错误: 有已暂存未提交改动"; exit 1; }
	@[ -n "$(REPO)" ] || { echo "错误: 无法从 git remote 解析仓库"; exit 1; }
	@command -v gh >/dev/null 2>&1 || { echo "错误: 未安装 gh CLI"; exit 1; }
	@gh auth status >/dev/null 2>&1 || { echo "错误: gh 未登录"; exit 1; }
	@echo ">>> 前置检查通过"
	@echo "    REPO = $(REPO)"
	@echo "    下一版本 = $(VERSION)"

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

.PHONY: install-local
install-local: build  ## 安装到 ~/.local/bin（开发用；正式安装请用 cargo-dist 生成的 installer）
	@install -d "$(BINDIR)"
	install -m 0755 "$(BUILD_DIR)/$(BIN)" "$(BINDIR)/$(BIN)"
	@echo ">>> 已安装到 $(BINDIR)/$(BIN)"
	@echo "    确保 $(BINDIR) 在 PATH 中"

.PHONY: uninstall-local
uninstall-local:  ## 卸载 ~/.local/bin 中的二进制
	-rm -f "$(BINDIR)/$(BIN)"
	@echo ">>> 已卸载 $(BINDIR)/$(BIN)"

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
	@echo "  make test-env          生成 shell 测试环境"
	@echo "  make test-env-clean    清理 shell 测试环境"
	@echo "  make verify            完整验证（check + test-cli + 二进制自检）"
	@echo ""
	@echo "版本与 CHANGELOG:"
	@echo "  make version           显示 git 推导的版本信息"
	@echo "  make changelog         用 git-cliff 生成 CHANGELOG.md"
	@echo ""
	@echo "发布:"
	@echo "  make dry-run           预览 cargo-release 动作（不执行）"
	@echo "  make release           执行 cargo-release（bump + tag + push）"
	@echo "  make release-check     检查发布前置条件"
	@echo "  make dist-plan         预览 cargo-dist 构建计划"
	@echo "  make dist-build        本地构建所有平台产物到 dist/"
	@echo ""
	@echo "  发布参数:"
	@echo "    LEVEL=patch|minor|major   默认 patch"
	@echo "    例: make release LEVEL=minor"
	@echo ""
	@echo "安装:"
	@echo "  make install-local     安装到 $(BINDIR)"
	@echo "  make uninstall-local   卸载"
	@echo ""
	@echo "清理:"
	@echo "  make clean             清空所有产物"
	@echo "  make dist-clean        仅清空 dist/"
	@echo ""
	@echo "详细流程见 RELEASING.md"
	@echo ""