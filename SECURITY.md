# Security Policy

English | [中文](SECURITY.zh-CN.md)


## Reporting a Vulnerability

**Please report through a private GitHub Security Advisory, not a public Issue.**

How:

1. Open the repository home page
2. Click the **Security** tab
3. Click **Report a vulnerability**
4. Fill in the vulnerability details

We respond within **n days** and publish a fixed release within **m days**.

If the GitHub Security Advisory cannot be used, you can also email:
`<dopamineplz@outlook.com>`

## Supported Versions

Only the latest stable version receives security updates. Please upgrade older versions.

## Known Security Design

This project does not reimplement cryptography; all encryption is provided by `gocryptfs`. Our
security responsibility is limited to:

### 1. Password Handling

- The password is passed from the TUI to the CLI through a **stdin pipe**
- Inside the CLI it is handed to `gocryptfs` via a **temporary file** (mode `600`)
- The temporary file is **deleted immediately** once the mount completes
- The password is **never written to config files, logs, or history**

### 2. Config Files

- Config files **contain no passwords or keys**
- They hold metadata only: paths, permission modes, option switches

### 3. Logs

- `$XDG_STATE_HOME/gocryptfs-tui/app.log.jsonl` (default `~/.local/state/gocryptfs-tui/app.log.jsonl`)
  records actions and paths only, **never passwords**
- Paths in the log can expose your directory structure; do not share them publicly

### 4. Mount Point Permissions

- When unmounted, `chmod 555` is applied automatically (read-only lock)
- When mounted, `chmod 755`
- The command-line `chmod` is used (special setgid/setuid/sticky bits are preserved)
- The mount point owner is never changed

### 5. Protection When Deleting the Encryption

- Deleting the encryption **does not delete the encrypted backing store** by default
- Deletion happens only when the config explicitly permits it (`remove.direct_delete_cipher: true`) and the user ticks the box
- The wizard offers no way to "exceed its authority" and enable backing-store deletion

### 6. Protection When Unmounted

- The mount point must exist (for example, for SMB sharing)
- When unmounted, writes are blocked via `chmod 555`
- No reliance on `chattr` (it would block gocryptfs mounts)

## Out of Threat Model

The following are outside this project's protection:

- The attacker already has root privileges
- The attacker has installed a keylogger on the system
- The attacker can physically access unencrypted memory
- The user's own system is fully compromised

Security in these scenarios is provided by the operating system and physical security.

## Dependency Security

The security of this project depends on these upstream components:

- `gocryptfs` — provides encryption
- `fusermount` — provides mounting
- `rsync` — provides file migration

Please keep these tools up to date.
