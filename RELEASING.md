# Releasing

English | [中文](RELEASING.zh-CN.md)


This document describes the complete gocryptfs-tui workflow from development to publishing a GitHub Release.

## Table of Contents

- [Toolchain](#toolchain)
- [One-time setup](#one-time-setup)
- [Day-to-day development](#day-to-day-development)
- [Publishing a new release](#publishing-a-new-release)
- [CI automated builds](#ci-automated-builds)
- [Verifying the Release](#verifying-the-release)
- [Rollback](#rollback)
- [Troubleshooting](#troubleshooting)

---

## Toolchain

### Required

| Tool | Purpose | Installation |
|------|---------|--------------|
| `cargo-release` | bump the version, tag, push | `cargo install cargo-release --locked` |
| `git-cliff` | generate CHANGELOG from the git log | `cargo install git-cliff --locked` |
| `cargo-dist` | multi-platform packaging, create the GitHub Release | `cargo install cargo-dist --locked` |
| `gh` | GitHub CLI, for viewing/operating Releases | `sudo apt install gh` or https://cli.github.com/ |

### Optional

| Tool | Purpose | Installation |
|------|---------|--------------|
| `cross` | local cross-compilation (arm64 / musl) | `cargo install cross --git https://github.com/cross-rs/cross` |

### Version check

```bash
cargo release --version
git-cliff --version
dist --version          # Note: not cargo dist
gh --version
```

---

## CHANGELOG and the release process

**CHANGELOG.md is never rewritten automatically.** No `pre-release-hook` is configured in
`Cargo.toml`, so `make dry-run` / `cargo release --dry-run` performs no file writes at all.

| Command | Behavior |
|---------|----------|
| `make changelog-preview` | writes a preview only to `.staging/CHANGELOG.preview.md` (`.staging/` is gitignored); **does not touch** `CHANGELOG.md` |
| `make changelog` | **explicitly rewrites** `CHANGELOG.md`: git-cliff generation + merging of `changelog.d/` fragments (fragments are kept); the `[Unreleased]` section by default, or `make changelog TAG=v0.2.0` to generate the given version's section and archive the fragments |
| `make dry-run` | read-only preview of the cargo-release actions; changes no files |
| `make release` | `check` → `release-check` → `changelog` → commit `CHANGELOG.md` → `cargo release --execute` |

### Release steps

```bash
# 1. Verify the code and tests
make check

# 2. First see what CHANGELOG will become (optional but recommended)
make changelog-preview
git diff --no-index -- CHANGELOG.md .staging/CHANGELOG.preview.md

# 3. Pre-flight checks (the working tree must be clean except for CHANGELOG.md)
make release-check

# 4. Preview cargo-release (read-only)
make dry-run

# 5. Release in one step (generate and commit the CHANGELOG, then bump + tag + push)
make release LEVEL=patch

# 6. Watch CI and the Release
gh run watch
gh release view v0.1.3 --web
```

### Updating only the CHANGELOG, without releasing

```bash
make changelog
git add CHANGELOG.md
git commit -m "docs: update CHANGELOG"
```

> Why the `pre-release-hook` was dropped: a cargo-release hook also runs during the earlier
> `--dry-run`, so a "preview" would rewrite `CHANGELOG.md` as well (wiping out the
> hand-maintained Unreleased section).

### Hand-written content: changelog.d/ fragments

Content that a commit subject cannot express (long explanations, upgrade notes) goes into a
fragment file, committed together with the code:

```bash
# Add a fragment
cat > changelog.d/2026-10-03-my-change.md <<'EOF'
### Features
- Add xxx (detailed explanation ...)
EOF

make changelog-check      # validate fragment syntax
make changelog-preview    # preview the merged result (does not touch CHANGELOG.md)
make changelog            # merge into [Unreleased] (fragments are kept, so this can be re-run)
```

- Use group names such as `Features` / `Bug Fixes` / `Documentation` / `Build`; names matching git-cliff merge into the same group automatically.
- If a change already has a fragment, add `[skip changelog]` to its commit message so the CHANGELOG
  does not list it twice (`cliff.toml` is configured to skip it).
- On release (`make release`) fragments are merged into the new version's section and archived to `changelog.d/archive/`.
- See `changelog.d/README.md` for details.

---

## Post-release checks (dependencies and install instructions)

cargo-dist uses the **CHANGELOG to generate the GitHub Release notes**, so the runtime
dependency list reaches the Release page through a `changelog.d/` fragment (the shell installer
itself cannot prompt for system dependencies).

Check once after releasing:

```bash
gh release view v0.3.0 --json body --jq .body | grep -A 3 "Runtime dependencies"
```

If the notes are missing the dependency list (for example because no fragment covered it), add one by hand:

```bash
body="$(gh release view v0.3.0 --json body --jq .body)"
printf '%s\n\n## Runtime dependencies\n\n- gocryptfs / fusermount(fuse3) / rsync / yq(Go v4) / jq / mountpoint(util-linux)\n- Optional: tree\n- Check: `gocryptfs-tui --check-deps`\n' "$body" \
  | gh release edit v0.3.0 --notes-file -
```

Confirm the Release contains **exactly one** install script (`gocryptfs-tui-install.sh`;
cargo-dist's `-installer.sh` is retired), and smoke-test both paths (neither needs source or a toolchain):

```bash
gh release view v0.5.0 --json assets --jq '.assets[].name' | grep -E 'install\.sh|\.tar\.xz$'

# User-level
curl -LsSf https://github.com/lockejet/gocryptfs-tui/releases/latest/download/gocryptfs-tui-install.sh \
  | sh -s -- --user --prefix /tmp/gocryptfs-tui-smoke
/tmp/gocryptfs-tui-smoke/bin/gocryptfs-tui --version

# Uninstall (from the manifest)
curl -LsSf https://github.com/lockejet/gocryptfs-tui/releases/latest/download/gocryptfs-tui-install.sh \
  | sh -s -- --uninstall --prefix /tmp/gocryptfs-tui-smoke

# System-level (requires sudo; confirm it installs to /usr/local/bin and links gocryptfs-cli)
curl -LsSf .../gocryptfs-tui-install.sh | sh -s -- --system
```

Then check once more that the install script itself is usable (POSIX sh, no cargo needed):

```bash
curl -LsSf https://github.com/lockejet/gocryptfs-tui/releases/latest/download/gocryptfs-tui-install.sh \
  | sh -n /dev/stdin && echo "syntax OK"
```
