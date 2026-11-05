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

- `$XDG_STATE_HOME/gocryptfs-ui/app.log.jsonl` (default `~/.local/state/gocryptfs-ui/app.log.jsonl`)
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

### 7. Web UI threat model

The Web UI (`gocryptfs-web`) can mount/unmount vaults, so a stolen session is equivalent to
control over every configured vault.

- **Loopback by default**: it listens on `127.0.0.1:30032`; binding to a non-loopback address
  prints a loud warning. Remote access should go through a TLS reverse proxy.
- **Credentials**: access tokens are random 256-bit secrets stored only as SHA-256 hashes in
  `web.token` (0600). Password users are stored in `web.auth` (0600) with Argon2id hashes.
  TOTP secrets and one-time backup codes live in the same file; backup codes are stored hashed
  and are single-use.
- **Sessions**: random session ids, `HttpOnly` + `SameSite=Strict` cookies, 12 h TTL; writes
  require the `x-gfs-csrf: 1` header and a matching `Origin`/`Host`.
- **Rate limiting**: 5 failed password attempts per minute per (IP, user), then a 15-minute lockout.
- **Passphrases**: entered in the browser, POSTed over the connection and handed to
  `gocryptfs-cli` through stdin. They are never stored, logged, or sent back to the client.
- **Not a file server**: the Web UI manages mount lifecycle; it does not stream plaintext files.
- **Do not expose it to the internet** without TLS and authentication in front of it; a
  compromised web session can unmount every vault and mount arbitrary configured vaults.

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
