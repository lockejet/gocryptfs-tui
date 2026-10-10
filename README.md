# gocryptfs-tui

English | [中文](README.zh-CN.md)

A TUI + CLI tool for managing `gocryptfs` encrypted vaults on Linux.

- **CLI** (`gocryptfs-cli`): Shell backend, feature-complete, script-friendly
- **TUI** (`gocryptfs-tui`): interactive terminal UI, intuitive for everyday operations

The TUI does all of its real work by invoking the CLI, so the two behave identically and can be used independently.

---

## Table of contents

- [Features](#features)
- [Dependencies](#dependencies)
- [Installation](#installation)
- [Command-line options](#command-line-options)
- [Internationalization (i18n)](#internationalization-i18n)
- [Configuration](#configuration)
- [Usage](#usage)
- [Logging and history](#logging-and-history)
- [Architecture](#architecture)
- [Security design](#security-design)
- [Development](#development)
- [Releasing](#releasing)
- [License](#license)

---

## Features

- Mount / unmount vaults (mount point permissions are locked automatically)
- Create a vault from a plaintext directory (with capacity checks and resumable transfers)
- Remove a vault (restores plaintext, with the option to keep or delete the cipher backend)
- Global settings plus per-task overrides
- `--dry-run` preview mode
- Passwords are passed via stdin and **never touch disk**
- When a mount point is unmounted it is automatically `chmod 555` (read-only lock); when mounted it is `chmod 755`
- Removing a vault requires typing `DELETE` as a second confirmation
- Supports SMB sharing setups (the mount point always exists)
- **Unified JSONL operation log** (shared by the CLI + TUI)
- **TUI help menu** (with version information, paths, and key bindings, exportable to `HELP.md`)
- **Multilingual UI**: Simplified Chinese / English, switchable via `--lang`, the `language:` config key, an environment variable, or the `L` key at runtime
- **Mainstream TUI key conventions**: `Tab` / `Shift+Tab` to cycle panes, `1`/`2`/`3` to jump between pages
- **cargo-release + cargo-dist release pipeline** (a git tag triggers multi-platform CI builds)

---

## Dependencies

Runtime dependencies:

| Dependency | Purpose | Required |
|------|------|----------|
| `gocryptfs` | Mount/unmount vaults | Required |
| `fusermount` (`fuse3`) | Unmount FUSE mounts | Required |
| `rsync` | Migrate/restore data | Required |
| `yq` ([mikefarah/yq](https://github.com/mikefarah/yq) **Go version v4**) | Parse the YAML config | Required |
| `jq` | JSON output | Required |
| `mountpoint` (`util-linux`) | Detect mount status | Required |
| `tree` | `t` tree view | Optional |

Debian / Ubuntu install:

```bash
sudo apt install gocryptfs fuse3 rsync jq util-linux
sudo wget -qO /usr/local/bin/yq \
    https://github.com/mikefarah/yq/releases/latest/download/yq_linux_amd64
sudo chmod +x /usr/local/bin/yq
```

> ⚠️ `yq` must be **mikefarah's Go version v4**. The `yq` in the Debian/Ubuntu repos is the Python wrapper,
> which has different syntax and will fail to read the vault list (this shows up in the UI as an "empty list").

Check whether all dependencies are present (either command works; the TUI check does not depend on the Shell backend being on PATH):

```bash
gocryptfs-tui --check-deps     # recommended: prints what is missing and how to install it; exit code 1 when a required dependency is missing
gocryptfs-cli --check-deps
```

- All three install modes run this check automatically once installation finishes and print a hint (`--no-deps-check` skips it);
- On its **first run**, if a required dependency is missing (or the `yq` version is wrong), the TUI writes the hint to the output pane,
  instead of only showing an "empty list" or "execution failed".

---

## Installation

**Three modes, one script, the same set of options**, installing to identical locations with identical configuration:

| Mode | Best for | Command | Needs |
|---|---|---|---|
| ① **One-liner** | Just want it installed | `curl -LsSf <release>/gocryptfs-tui-install.sh \| sh -s -- --user` (or `--system`) | `curl` + POSIX sh |
| ② **git clone** | Want to keep the script / repeatable installs, without installing Rust | `git clone … && cd gocryptfs-tui && ./install.sh --from-release --user` | `git` + `curl` |
| ③ **Source build** | Build it yourself / change the code | `git clone … && cd gocryptfs-tui && make build && ./install.sh --local --user` | `git` + **cargo** |

Install locations (identical across all three modes):

| Option | Location | Notes |
|---|---|---|
| `--user` (default) | `~/.local` | No sudo required |
| `--system` | `/usr/local` | Uses sudo as needed, and links `gocryptfs-cli` into `/usr/local/bin` by default |
| `--prefix DIR` | Custom | e.g. `/opt/gocryptfs-tui` |

All three modes install `<prefix>/bin/gocryptfs-tui` and `<prefix>/lib/gocryptfs-tui/`
(the standalone Shell backend + `VERSION`) and write an install manifest; none of them **modify the shell profile** — they
only print the `export PATH=…` you need to run; config and data directories follow XDG (see below), independent of install method.

### ① One-liner (no source, no toolchain)

```bash
# User-level → ~/.local/bin (default, no sudo)
curl -LsSf https://github.com/lockejet/gocryptfs-tui/releases/latest/download/gocryptfs-tui-install.sh | sh

# System-level → /usr/local/bin (sudo as needed, and links the standalone CLI)
curl -LsSf https://github.com/lockejet/gocryptfs-tui/releases/latest/download/gocryptfs-tui-install.sh \
  | sh -s -- --system

# Pin a version
curl -LsSf .../gocryptfs-tui-install.sh | sh -s -- --user --version v0.5.0
```

The script will: detect the architecture/libc → download the matching `tar.xz` and **verify its sha256** → fetch
`shell/` from the same Release's `source.tar.gz` as the standalone backend (guaranteeing it matches the TUI version) → run
`--check-deps` → write the install manifest (`INSTALLED.json` / `INSTALLED.files`).

> **Offline / air-gapped**: just download `gocryptfs-tui-<target>.tar.xz` from
> [Releases](https://github.com/lockejet/gocryptfs-tui/releases), extract it, and put `gocryptfs-tui` in
> `~/.local/bin` (the embedded backend unpacks itself into the data directory on first run).

### ② git clone mode (no cargo required)

```bash
git clone https://github.com/lockejet/gocryptfs-tui.git
cd gocryptfs-tui
./install.sh --from-release            # defaults to the current checkout's tag; installs to ~/.local
./install.sh --from-release --system   # installs to /usr/local
```

`--from-release` uses the Release assets for the current checkout's most recent tag; outside a git repository it falls back to `latest`.
`./install.sh` without a mode argument also decides automatically: if the source is present and can be compiled → build locally; otherwise use the Release binary.

### ③ Source build mode (source + cargo)

```bash
git clone https://github.com/lockejet/gocryptfs-tui.git
cd gocryptfs-tui
make build                             # or cargo build --release
./install.sh --local                   # installs to ~/.local
./install.sh --local --system          # installs to /usr/local
```

`make install` / `make install-system` are convenience wrappers for this mode (equivalent to `--local --user` /
`--local --system`).

### Install options (common to all three modes)

| Option | Description |
|---|---|
| `--user` / `--system` / `--prefix DIR` | Install location (default `~/.local`) |
| `--local` / `--from-release[=vX.Y.Z]` / `--version vX.Y.Z` | Force a mode or pin a version |
| `--uninstall` | Uninstall exactly what the install manifest lists |
| `--link-cli` / `--no-link-cli` | Whether to link `gocryptfs-cli` into `<prefix>/bin` |
| `--no-deps-check` | Skip the post-install dependency check |

Post-install self-check:

```bash
command -v gocryptfs-tui        # expect ~/.local/bin/gocryptfs-tui (or /usr/local/bin)
gocryptfs-tui --version
gocryptfs-tui --check-deps
gocryptfs-tui --print-paths     # troubleshooting: see every path actually in use at a glance
```

> **Canonical location of the standalone CLI**: on first run the TUI unpacks the embedded backend to
> `${XDG_DATA_HOME:-~/.local/share}/gocryptfs-tui/backend/gocryptfs-cli`,
> identically in all three modes. To use it directly from the command line, create a symlink once:
> ```bash
> ln -sf "${XDG_DATA_HOME:-$HOME/.local/share}/gocryptfs-tui/backend/gocryptfs-cli" \
>        "${XDG_BIN_HOME:-$HOME/.local/bin}/gocryptfs-cli"
> ```
> `--system` links `<prefix>/lib/gocryptfs-tui/gocryptfs-cli` into `/usr/local/bin` automatically.

### Upgrading

The TUI and the Shell backend (`gocryptfs-cli` + `lib/*.sh`) must be **updated as a set**, otherwise you may see
inconsistencies such as "the UI is already in English but the output pane is still in Chinese". The embedded backend is updated
along with the binary (the unpack directory `${XDG_DATA_HOME:-~/.local/share}/gocryptfs-tui/backend/` is rewritten only when its contents change).

```bash
# ① One-liner: rerun the same command
curl -LsSf .../gocryptfs-tui-install.sh | sh -s -- --user

# ② clone: git pull, then rerun
git pull && ./install.sh --from-release --user

# ③ Source: rebuild, then install over the old version
git pull && make build && ./install.sh --local --user
```

If the TUI is not calling the newly installed backend, you can point it at one temporarily with
`GOCRYPTFS_CLI=$(command -v gocryptfs-cli) gocryptfs-tui`; `gocryptfs-tui --print-paths` shows the backend and paths actually in use.

### Uninstalling

```bash
# ① one-liner / ② clone / ③ source all uninstall exactly what the install manifest lists
curl -LsSf .../gocryptfs-tui-install.sh | sh -s -- --uninstall --user
curl -LsSf .../gocryptfs-tui-install.sh | sh -s -- --uninstall --system
./install.sh --uninstall --user            # when the script is already local
./install.sh --uninstall --system

# Convenience wrappers
make uninstall                             # equivalent to --user
make uninstall-system                      # equivalent to --system
make uninstall PREFIX=/opt/gocryptfs-tui   # custom prefix
```

> Uninstalling does **not** delete config or data: config `${XDG_CONFIG_HOME:-~/.config}/gocryptfs-tui/`,
> data `${XDG_DATA_HOME:-~/.local/share}/gocryptfs-tui/` (including logs, history, HELP.md,
> and the unpacked embedded backend). Delete these two directories yourself for a full cleanup.
>
> `install.sh --uninstall` reads `<prefix>/lib/gocryptfs-tui/INSTALLED.files` and deletes the entries one by one,
> and only allows deleting paths inside the prefix; `make uninstall` deletes `$PREFIX/bin/gocryptfs-tui`,
> `$PREFIX/bin/gocryptfs-cli`, and `$PREFIX/lib/gocryptfs-tui/`.
>
> If you installed 0.4.x or earlier with cargo-dist's `-installer.sh` (into `~/.cargo/bin` or `~/.local/bin`):
> `rm -f ~/.local/bin/gocryptfs-tui ~/.cargo/bin/gocryptfs-tui`.

---

## Command-line options

The TUI accepts the following options:

```
gocryptfs-tui [OPTIONS]

OPTIONS:
    -c, --config <PATH>    Config file path
                           [default: ~/.config/gocryptfs-tui/config.yaml]
    -D, --data-dir <DIR>   Data directory (logs, history, help)
                           [default: ~/.local/share/gocryptfs-tui]
    -l, --lang <CODE>      UI language, one of zh-CN / en-US
        --check-deps       Check runtime dependencies and print install commands
        --print-paths      Print all effective paths (troubleshooting)
    -h, --help             Show help
    -V, --version          Show version information
```

The Shell backend `gocryptfs-cli` provides the **same** global options (`-c` / `-D` / `-l` / `-V` / `-h` / `--check-deps`),
which makes it easy to call uniformly from scripts:

```bash
gocryptfs-cli --version
gocryptfs-cli -D /tmp/data -c /path/to/config.yaml list --json
```

Examples:

```bash
# Use the default config
gocryptfs-tui

# Specify a config file
gocryptfs-tui -c /path/to/config.yaml

# Specify a data directory
gocryptfs-tui -D /tmp/gocryptfs-tui-data

# Specify the UI language (for this run only)
gocryptfs-tui --lang en-US

# Show the version
gocryptfs-tui --version

# Show help
gocryptfs-tui --help

# Troubleshooting: see the binaries/backend/config/data/log paths actually in use at a glance
gocryptfs-tui --print-paths
```

The TUI top bar shows two lines of live information (line 1: Bin/CLI + version, line 2: Config/Data):

```
Bin: /usr/local/bin/gocryptfs-tui  CLI:  /usr/local/lib/gocryptfs-tui/gocryptfs-cli          v0.2.0
Config: /home/you/.config/gocryptfs-tui/config.yaml  Data: /home/you/.local/share/gocryptfs-tui
```

---

## Internationalization (i18n)

The UI text (**TUI + Shell backend**) supports **Simplified Chinese (`zh-CN`, the built-in default)** and **English (`en-US`)**.
The TUI language is determined at startup, after which it can be switched at runtime with the `L` key (or the `l` key inside the settings overlay, for the current session only);
when the TUI runs a command it passes the current language to `gocryptfs-cli` via `GOCRYPTFS_LANG`, so the output pane and the backend hints use the same language.

### Language selection priority

From highest to lowest:

```
--lang  >  GOCRYPTFS_TUI_LANG  >  config language:  >  system locale (LANGUAGE / LC_ALL / LC_MESSAGES / LANG)  >  built-in default zh-CN
```

- The system locale is resolved per POSIX as `LC_ALL` > `LC_MESSAGES` > `LANG`, and only supported languages are recognized.
- `LANGUAGE` (GNU's colon-separated preference list, e.g. `zh_CN:en_US`) takes the first recognizable language and
  takes precedence over the locales above, but is ignored when the effective locale is `C` / `POSIX`.
- `C` / `POSIX` means "no localization"; in that case it falls back to the built-in default `zh-CN`. A Chinese locale that is
  not installed (e.g. `LANG=zh_CN.UTF-8` with no such locale on the system) does not affect recognition.
- The `language:` key in the config file overrides the system locale, which is handy for pinning a language preference.

### Specifying the language

```bash
# Command line (highest priority)
gocryptfs-tui --lang en-US

# Environment variable (this session only)
GOCRYPTFS_TUI_LANG=en-US gocryptfs-tui

# Config file (persistent, overrides the system locale)
# ~/.config/gocryptfs-tui/config.yaml
language: zh-CN

# Follow the system locale
LANG=zh_CN.UTF-8 gocryptfs-tui

# The Shell backend supports this too (-l/--lang or the environment variable)
gocryptfs-cli --lang en-US list
GOCRYPTFS_LANG=en-US gocryptfs-cli -c ~/.config/gocryptfs-tui/config.yaml list
```

### Switching at runtime

| Where | Key | Description |
|------|----|------|
| Any page | `L` | Toggles between `zh-CN` / `en-US`; the status bar and output pane print a hint |
| Settings overlay | `l` / `L` | Same as above; the overlay stays open for side-by-side comparison |
| Help / history overlay | `L` | Same as above; the overlay stays open |

> Runtime switching only affects the current process; to make it permanent, write `language:` in the config or use `--lang`.
> The exported help file (`H`) follows the current language.
> After switching, scope names, directory pane contents, and task descriptions are rebuilt, with no stale-language cache left behind.
> The TUI passes the current language to `gocryptfs-cli` via `GOCRYPTFS_LANG`; if the output pane language does not match the UI,
> it is most likely calling an old installed backend (check the `CLI:` path in the startup output, or rerun `make install` / `make install-system`).
> In the English UI, if an old backend is detected the TUI prints an explicit warning in the output pane (`does not support --lang`).
> Old backends also append plain-text lines to `app.log.jsonl` (corrupting the JSONL); `gocryptfs-cli log` already skips them automatically,
> and to clean up a historical file you can run `grep '^{' app.log.jsonl > tmp && mv tmp app.log.jsonl`.

### Contributing translations / adding a language

The translation tables live in:

```
src/i18n.rs          # TUI: language enum, resolution priority, t!/tn! macros, key-table validation tests
src/i18n/zh_cn.rs    # TUI Simplified Chinese (key -> text, sorted by key ascending)
src/i18n/en_us.rs    # TUI English
shell/lib/i18n.sh    # Shell backend: message tables (I18N_ZH / I18N_EN) and language resolution
```

- Keys look like `<area>.<meaning>` (e.g. `status.ready`); placeholders in the text use `{}` (positional) or `{name}` (named).
- **Do not hand-write alignment spaces in label-style strings** (e.g. `"Name:      "`): the render layer pads all labels
  in the same group uniformly by display width (CJK counts as 2 columns), avoiding column-width mismatches between Chinese and English.
- Adding a language: create `<locale>.rs` under `src/i18n/` (copy the keys from `en_us.rs` and translate the values),
  then register it in the `Lang` enum, `ALL`, and `code()/display_name()/table()` in `src/i18n.rs`.
- Validation:

```bash
make i18n-check                              # key-table consistency + call-site arguments + leftover Chinese (with its own self-check)
cargo test i18n                              # translation-table and render-layer unit tests
make test-i18n                               # TUI + Shell language-resolution/text acceptance tests
```

Shell-side convention: message templates live in `shell/lib/i18n.sh` and may only use `printf`'s `%s` placeholders;
call them as `t <key> [args...]` (no newline) and `te <key> [args...]` (with newline).

---

## Configuration

### Config file location

Default (XDG-aware, `XDG_CONFIG_HOME` is honored):

```
${XDG_CONFIG_HOME:-~/.config}/gocryptfs-tui/config.yaml
```

Priority: `-c/--config` > `GOCRYPTFS_CONFIG` > `CONFIG_FILE` (legacy) > `XDG_CONFIG_HOME` > `~/.config`.

The data directory (log `app.log.jsonl`, history `history.jsonl`, `HELP.md`, embedded backend) works the same way:

```
${XDG_DATA_HOME:-~/.local/share}/gocryptfs-tui/
```

Priority: `-D/--data-dir` (supported by both the CLI and the TUI) > `GOCRYPTFS_DATA_DIR` >
`LOG_FILE`/`HISTORY_FILE` (used by the TUI for child processes) > `XDG_DATA_HOME` > `~/.local/share`.

Use `-c` to specify another path:

```bash
gocryptfs-cli -c /path/to/config.yaml list
gocryptfs-tui -c /path/to/config.yaml
```

### Minimal config

```yaml
# UI language (optional): zh-CN / en-US; follows the system locale when omitted
language: zh-CN

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

### Full config example

See `examples/config.yaml.example`; field meanings:

- `language`: UI language (`zh-CN` / `en-US`), takes priority over the system locale
- `settings.unlock_mode`: chmod permission before mounting, default `"755"`
- `settings.lock_mode`: chmod permission while unmounted, default `"555"`
- `settings.gocryptfs.*`: gocryptfs mount options (`allow_other`, `read_only`, etc.)
- `settings.rsync.*`: rsync migration options (`archive`, `compress`, `partial`, etc.)
- `settings.filters[]`: rsync filter rules
- `settings.create.*`: creation policy (`keep_source`, `tmp_mount_suffix`)
- `settings.remove.*`: removal policy (`restore`, `direct_delete_cipher`)
- `settings.logging.*`: log level and rotation (`level`, `max_size`, `max_files`)
- `vaults[]`: vault list; each entry has `id`, `name`, `path`, `mount_point`, and an optional `overrides`
- `pending[]`: list of directories to process (used by the TAB2 creation wizard)

### Precedence

```
built-in defaults < global settings < task overrides < command-line options
```

### Policy options

The following options follow the "**only tighten, never loosen**" principle in the wizard:

| Config key | Value | Wizard behavior |
|--------|----|---------|
| `create.keep_source` | `false` | Keeping source files can be checked |
| `create.keep_source` | `true` | Greyed out, read-only |
| `remove.restore` | `true` | Greyed out, read-only |
| `remove.restore` | `false` | Restoring can be checked |
| `remove.direct_delete_cipher` | `true` | Can be unchecked |
| `remove.direct_delete_cipher` | `false` | Greyed out, read-only |

### Logging settings

```yaml
settings:
  logging:
    # operation   log business operations only (mount/umount/create/remove) — recommended
    # interactive business operations + user interaction (keys/pages/wizard steps)
    # debug       log everything (including internal events)
    level: operation
    # Maximum size of a single log file (bytes); rotates once exceeded
    max_size: 5242880      # 5 MB
    # Number of historical files to keep
    max_files: 3
```

---

## Usage

### CLI

```
gocryptfs-cli [-c <config>] [-l <lang>] <command> [options]
```

Common commands:

```bash
# List vaults
gocryptfs-cli list
gocryptfs-cli list --json

# Vault details
gocryptfs-cli info my_vault

# Mount (password via stdin)
echo 'your-password' | gocryptfs-cli mount my_vault

# Unmount (add --force if it fails)
gocryptfs-cli umount my_vault
gocryptfs-cli umount my_vault --force

# View the mount point directory
gocryptfs-cli ls my_vault
gocryptfs-cli tree my_vault

# Create a vault from a plaintext directory (dry-run preview)
gocryptfs-cli create /srv/photos --name photos --dry-run

# Real creation (--yes skips the second confirmation)
echo 'your-password' | gocryptfs-cli create /srv/photos --name photos --yes

# Remove a vault (restore plaintext)
echo 'your-password' | gocryptfs-cli remove photos --yes

# Remove a vault (keep the cipher backend)
echo 'your-password' | gocryptfs-cli remove photos --keep-cipher --yes

# View the operation log
gocryptfs-cli log                       # last 20 entries
gocryptfs-cli log --src tui             # TUI only
gocryptfs-cli log --action mount        # mount only
gocryptfs-cli log --result failed       # failures only
gocryptfs-cli log --follow              # follow in real time
gocryptfs-cli log --json                # JSON output

# Check dependencies
gocryptfs-cli --check-deps
```

Exit codes:

| Code | Meaning |
|----|------|
| 0 | Success |
| 1 | Generic error |
| 2 | Wrong password |
| 3 | Wrong state |
| 4 | Mount point problem |
| 5 | Not enough disk space |
| 6 | Unmount failed |
| 7 | Forced unmount failed |
| 8 | Configuration error |

### TUI

```bash
gocryptfs-tui
```

The UI is divided into 3 pages:

| Page | Name | Function |
|------|------|------|
| `[1]` | Mount / unmount | Everyday mounting, unmounting, and browsing directories |
| `[2]` | Create vault | Create from a plaintext directory |
| `[3]` | Remove vault | Remove a vault and restore plaintext |

The UI is divided into 4 focus panes (list / details / directory / output); cycle through them with `Tab`, or jump directly with `Alt+number`.

#### Page and pane switching (all pages)

| Key | Function | Category |
|----|------|------|
| `1` / `2` / `3` | Jump to page (mount / create / remove) | Page |
| `[` / `]` | Cycle pages (previous / next, both wrap around) | Page |
| `Tab` / `Shift+Tab` | Cycle panes (forward / backward) | Pane |
| `Alt+1` / `Alt+2` / `Alt+3` / `Alt+4` | Jump to pane (list / details / directory / output) | Pane |

#### Global keys (all pages)

| Key | Function |
|----|------|
| `s` | Settings overlay |
| `h` | History overlay |
| `e` | Open the config in an external editor |
| `r` | Refresh |
| `L` | Switch the UI language (zh-CN / en-US) |
| `?` | Help overlay (includes `H` to export) |
| `q` | Quit |
| `Ctrl+C` | Interrupt the current task |
| `Ctrl+D` × 3 (within 2 seconds) | Force quit |

#### List pane keys

| Key | Function |
|----|------|
| `j` / `k` / `↑` / `↓` | Move the selection |
| `Space` | Confirm the current item (a `▶` prefix appears) |
| `Enter` | Confirm inside overlays only (wizard/password/history, etc.); no longer used on list pages |
| `m` | Mount (TAB1); when already mounted, only a grey hint is shown |
| `u` | Unmount (TAB1); when not mounted, only a grey hint is shown |
| `c` | Creation wizard (TAB2) |
| `d` | Removal wizard (TAB3) |
| `l` | List (ls -la) → load into the directory pane and move focus there |
| `t` | Tree (tree) → load into the directory pane and move focus there |

> **Important**: press `Space` to select first, then `m`/`u`/`c`/`d` to act (on TAB1 use `m` to mount and `u` to unmount; on TAB2/TAB3 use `c`/`d` to enter the wizard — `Enter` no longer triggers it). Moving the cursor clears the selection.

#### Details / directory / output pane keys

| Key | Function |
|----|------|
| `↑` / `↓` | Scroll vertically |
| `←` / `→` | Scroll horizontally |
| `PgUp` / `PgDn` | Page up / down |
| `g` / `G` | Jump to the top / bottom |
| `c` | Clear (directory pane / output pane) |
| `l` / `t` | Refresh the directory view (directory focus only) |

#### Wizard keys

| Key | Function |
|----|------|
| `Space` | Toggle an option |
| `j` / `k` | Move between options |
| `Enter` | Next step |
| `Esc` | Cancel |
| `Backspace` / `Delete` / `Ctrl+H` | Delete a password character |

#### History overlay keys

| Key | Function |
|----|------|
| `j` / `k` / `↑` / `↓` | Move the selection |
| `s` | Cycle the source filter (all → cli → tui → all) |
| `r` | Cycle the result filter (all → success → failed → started → cancelled → all) |
| `a` | Cycle the action filter (all → mount → umount → create → remove → all) |
| `Esc` | Close |

#### Help overlay keys

| Key | Function |
|----|------|
| `H` | Export help to `<data_dir>/HELP.md` (follows the current language) |
| `Esc` / `?` / `q` | Close |

#### Settings overlay keys

| Key | Function |
|----|------|
| `Tab` | Switch scope (global / individual vaults) |
| `g` / `r` / `f` / `p` | Switch category (gocryptfs / rsync / filters / permissions) |
| `l` | Switch the UI language |
| `e` | Open the config in an external editor |
| `Esc` | Close |

---

## Logging and history

### Unified log

Every operation from the CLI and the TUI is recorded in **the same file**:

```
~/.local/share/gocryptfs-tui/app.log.jsonl
```

One JSON record per line:

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

Field meanings:

| Field | Description |
|------|------|
| `ts` | ISO 8601 timestamp |
| `src` | Source: `cli` or `tui` |
| `action` | Action: `mount` / `umount` / `create` / `remove` / `tui.start` / `tui.quit` |
| `target` | Target: vault name or description |
| `result` | `started` / `success` / `failed` / `cancelled` |
| `detail` | Extra information (e.g. command line, error details) |
| `pid` | Associated process PID (set by the TUI, null on the CLI side) |
| `duration_ms` | Duration in milliseconds (set by the TUI, null on the CLI side) |

### Log rotation

Once a single file exceeds `settings.logging.max_size`:

```
app.log.jsonl  →  app.log.jsonl.1  →  app.log.jsonl.2  →  ...
```

`settings.logging.max_files` historical files are kept.

### Migrating old history

Earlier versions used `history.jsonl` (with the field names `name` / `status`). On its first run, the new version automatically migrates the old file's contents into `app.log.jsonl` and deletes the old file. Field mapping:

| Old | New |
|----|----|
| `name` | `target` |
| `status` | `result` |
| (none) | `src: "cli"` |

### Querying

CLI side:

```bash
gocryptfs-cli log                        # last 20 entries (human-readable, colored)
gocryptfs-cli log --limit 100            # last 100 entries
gocryptfs-cli log --src tui              # TUI only
gocryptfs-cli log --action mount         # mount only
gocryptfs-cli log --result failed        # failures only
gocryptfs-cli log --since 2026-09-29     # filter by time
gocryptfs-cli log --follow               # follow in real time
gocryptfs-cli log --json                 # JSON output
```

TUI side:

- Press `h` to open the history overlay
- Press `s` / `r` / `a` to switch the source / result / action filter
- Press `j` / `k` to move the selection

---

## Architecture

```
┌──────────────────────────────────────────────────────┐
│                         User                         │
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

### Directory structure

```
gocryptfs-tui/
├── Cargo.toml
├── build.rs                 # compile-time version injection
├── Makefile
├── release.toml             # cargo-release configuration
├── cliff.toml               # git-cliff configuration
├── Cross.toml               # cross cross-compilation configuration
├── dist-workspace.toml      # cargo-dist configuration
├── RELEASING.md             # release process documentation
├── README.md
├── CHANGELOG.md
├── LICENSE
├── SECURITY.md
├── .github/
│   └── workflows/
│       └── release.yml      # cargo-dist CI workflow
├── src/
│   ├── main.rs              # TUI entry point + rendering
│   ├── cli.rs               # command-line argument parsing (including --lang)
│   ├── logger.rs            # logging module
│   ├── i18n.rs              # i18n API (language resolution, t!/tn! macros, key-table tests)
│   ├── i18n_render_tests.rs # render-layer i18n tests (test builds only)
│   └── i18n/
│       ├── zh_cn.rs         # Simplified Chinese translation table
│       └── en_us.rs         # English translation table
├── shell/
│   ├── gocryptfs-cli        # CLI entry point (including -l/--lang)
│   └── lib/
│       ├── gocryptfs-lib.sh # all library functions
│       └── i18n.sh          # Shell-side message tables and language resolution
├── tools/
│   └── i18n_check.py        # TUI + Shell i18n key-table/call-site consistency check
├── test/
│   ├── create-test-env.sh
│   ├── cleanup-test-env.sh
│   ├── test-batch1.sh
│   ├── test-batch2.sh
│   ├── test-i18n.sh         # CLI-side i18n acceptance tests
│   └── test-all.sh
└── examples/
    └── config.yaml.example
```

### CLI ↔ TUI interaction protocol

The CLI reports progress and results to the TUI through **stderr protocol lines**:

| Protocol line | Meaning |
|--------|------|
| `@@PROGRESS@@ <pct> <done> <total>` | Migration progress |
| `@@CHECK@@ <key> <value>` | Checkpoint data (e.g. capacity) |
| `@@DONE@@ <message>` | Operation succeeded |
| `@@ERROR@@ <code> <message>` | Operation failed |

The TUI captures these lines and updates the UI state; ordinary stdout/stderr is displayed in the output pane.

---

## Security design

This project **does not reimplement cryptography**; all encryption is provided by `gocryptfs`. The project's security responsibilities are:

### Password handling

- The password is passed from the TUI to the CLI via stdin
- Inside the CLI it is passed to gocryptfs through a **temporary file** (mode `600`)
- The temporary file is **deleted immediately** after mounting completes
- Passwords are **never written to the config file, logs, or history**

### Config file

- **Contains no passwords or keys**
- Contains only paths, permission modes, and option toggles

### Mount point permissions

- `chmod 555` while unmounted (read-only lock)
- `chmod 755` while mounted
- Uses the system `chmod` command (preserving setgid/setuid/sticky special bits)

### Deletion protection

- By default the **cipher backend is not deleted**
- It is deleted only when the config explicitly authorizes it (`remove.direct_delete_cipher: true`) and the user types `DELETE`
- The wizard cannot be used to "escalate" into enabling backend deletion

### Log privacy

- Logs never record passwords
- Logs never record plaintext contents
- Logs record only operation metadata (vault name, path, result)

For the detailed security notes, see [SECURITY.md](SECURITY.md).

---

## Development

### Building

```bash
# Native release build
make build

# Debug build
make build-debug

# Cross-compilation (requires cross)
make build-arm64
make build-musl
make build-arm64-musl
make build-all
```

### Checks and tests

```bash
# All checks: fmt + clippy + unit tests
make check

# Run individually
make fmt           # format the code
make lint          # clippy (-D warnings)
make i18n-check    # i18n key-table/call-site validation
make test          # Rust unit tests (including i18n render tests)
make test-i18n     # i18n acceptance: TUI + Shell language-resolution matrix and text
make test-env      # generate the shell test environment
make test-cli      # run the shell test batches
make test-env-clean  # clean up the test environment

# Full verification
make verify
```

### Local installation

`make install` / `make install-system` are convenience wrappers for **mode ③ (source + cargo)**:

```bash
make install            # = ./install.sh --local --user    (~/.local)
make install-system     # = ./install.sh --local --system  (/usr/local, sudo as needed)
make uninstall          # = ./install.sh --uninstall --user
make uninstall-system   # = ./install.sh --uninstall --system
```

Modes ① (one-liner) / ② (clone) do not need cargo; just use the install script (see "Installation").

### All Make targets

```bash
make help
```

---

## Releasing

This project uses `cargo-release` + `cargo-dist` together:

- **cargo-release**: bump the version, tag, push (**no longer rewrites the CHANGELOG automatically**)
- **cargo-dist**: multi-platform CI builds, generates installers, creates the GitHub Release

> CHANGELOG.md is generated only when you explicitly run `make changelog` (or the explicit step in `make release`);
> `make dry-run` / `cargo release --dry-run` do not modify any file.
> To document something a commit title cannot express, drop a fragment in `changelog.d/` (`make changelog` merges it automatically,
> and `make changelog-check` validates the format); see `changelog.d/README.md` for details.

### Prerequisite tools

```bash
cargo install cargo-release --locked
cargo install git-cliff --locked
cargo install cargo-dist --locked
cargo install cross --git https://github.com/cross-rs/cross    # optional
gh auth login
```

### Release steps

```bash
# 1. Preview the CHANGELOG diff (writes only to .staging/, leaves CHANGELOG.md untouched)
make changelog-preview

# 2. Pre-flight checks (the working tree must be clean except for CHANGELOG.md)
make release-check

# 3. Preview the cargo-release actions (read-only, changes no files)
make dry-run

# 4. One-step release: generate CHANGELOG → commit → cargo release (bump + tag + push)
make release

# Or specify a level
make release LEVEL=minor
make release LEVEL=major

# 5. Watch CI progress
gh run watch

# 6. Verify the Release
gh release view v0.2.0 --web
```

After the tag is pushed, GitHub Actions automatically triggers `.github/workflows/release.yml`, builds 4 platforms (gnu/musl × amd64/arm64), and creates the Release.

For the detailed process, see [RELEASING.md](RELEASING.md).

---

## License

MIT License, see [LICENSE](LICENSE).
