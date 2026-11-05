//! 认证核心：多 token 存储（JSON + 旧格式迁移）、用户/2FA 存储、
//! 密码哈希、登录限流与二维码渲染。
//!
//! 该模块不依赖 HTTP 层，便于单元测试与 CLI 复用。

use std::collections::{HashMap, VecDeque};
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::{DateTime, SecondsFormat, Utc};
use qrcode::render::svg;
use qrcode::QrCode;
use rand::{Rng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use totp_rs::{Algorithm, Secret, TOTP};

use gocryptfs_ui_core as core;

/// 一次性登录链接（QR）有效期。
pub const LOGIN_NONCE_TTL: Duration = Duration::from_secs(120);

/// 登录失败计数窗口。
pub const RATE_WINDOW: Duration = Duration::from_secs(60);
/// 一个窗口内允许的失败次数；达到后拒绝后续尝试。
pub const RATE_MAX_FAILURES: usize = 5;
/// 累计失败多少次后触发锁定。
pub const LOCKOUT_AFTER: u32 = 10;
/// 锁定时长。
pub const LOCKOUT_DURATION: Duration = Duration::from_secs(15 * 60);

// ============================================================
// 通用工具
// ============================================================

/// 32 字节 CSPRNG → base64url（无填充）。
pub fn random_secret() -> String {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    URL_SAFE_NO_PAD.encode(raw)
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub fn sha256_hex(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

/// 常数时间字符串比较（长度不同直接返回 false）。
pub fn ct_eq_str(a: &str, b: &str) -> bool {
    ct_eq_bytes(a.as_bytes(), b.as_bytes())
}

/// 常数时间字节比较。
pub fn ct_eq_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// 写入 0600 文件（父目录按需创建）。已存在的文件也会被收紧到 0600。
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

// ============================================================
// 多 token 存储
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenEntry {
    pub id: String,
    pub label: String,
    pub hash: String,
    pub created_at: String,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenStore {
    #[serde(default)]
    pub tokens: Vec<TokenEntry>,
}

impl TokenStore {
    /// 由明文 token 构造（仅内存；用于测试/首次启动）。
    pub fn with_token(token: &str) -> Self {
        Self {
            tokens: vec![TokenEntry {
                id: "t1".to_string(),
                label: "default".to_string(),
                hash: sha256_hex(token),
                created_at: now_rfc3339(),
                expires_at: None,
            }],
        }
    }

    fn next_id(&self) -> String {
        let mut n = 1usize;
        loop {
            let id = format!("t{n}");
            if !self.tokens.iter().any(|t| t.id == id) {
                return id;
            }
            n += 1;
        }
    }

    /// 校验明文 token（比对 sha256；忽略已过期条目）。
    pub fn verify(&self, token: &str) -> bool {
        let digest = Sha256::digest(token.as_bytes());
        let now = Utc::now();
        let mut ok = false;
        for entry in &self.tokens {
            if let Some(expires) = &entry.expires_at {
                match DateTime::parse_from_rfc3339(expires) {
                    Ok(dt) if dt.with_timezone(&Utc) > now => {}
                    _ => continue,
                }
            }
            let stored = match hex::decode(&entry.hash) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if ct_eq_bytes(&digest, &stored) {
                ok = true;
            }
        }
        ok
    }

    /// 新增一个 token；返回（条目，明文 token）。明文只在此处出现一次。
    pub fn add_token(
        &mut self,
        label: &str,
        expires_at: Option<String>,
    ) -> Result<(TokenEntry, String), String> {
        let label = label.trim();
        if label.is_empty() {
            return Err("token label must not be empty".to_string());
        }
        if self.tokens.iter().any(|t| t.label == label) {
            return Err(format!("token label already exists: {label}"));
        }
        if let Some(expires) = &expires_at {
            let dt = DateTime::parse_from_rfc3339(expires)
                .map_err(|e| format!("invalid expiry {expires:?}: {e}"))?;
            if dt.with_timezone(&Utc) <= Utc::now() {
                return Err(format!("expiry must be in the future: {expires}"));
            }
        }
        let token = random_secret();
        let entry = TokenEntry {
            id: self.next_id(),
            label: label.to_string(),
            hash: sha256_hex(&token),
            created_at: now_rfc3339(),
            expires_at,
        };
        self.tokens.push(entry.clone());
        Ok((entry, token))
    }

    /// 按 id 或 label 撤销；返回撤销数量。
    pub fn revoke(&mut self, id_or_label: &str) -> Result<usize, String> {
        let target = id_or_label.trim();
        if target.is_empty() {
            return Err("revoke requires a token id or label".to_string());
        }
        let before = self.tokens.len();
        self.tokens.retain(|t| t.id != target && t.label != target);
        let removed = before - self.tokens.len();
        if removed == 0 {
            return Err(format!("no token with id or label {target:?}"));
        }
        Ok(removed)
    }

    /// 当前仍然有效的 token 数量（用于展示）。
    pub fn active_count(&self) -> usize {
        let now = Utc::now();
        self.tokens
            .iter()
            .filter(|entry| match &entry.expires_at {
                None => true,
                Some(expires) => DateTime::parse_from_rfc3339(expires)
                    .map(|dt| dt.with_timezone(&Utc) > now)
                    .unwrap_or(false),
            })
            .count()
    }
}

pub fn save_token_store(path: &Path, store: &TokenStore) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(store).map_err(|e| e.to_string())?;
    let mut bytes = data;
    bytes.push(b'\n');
    write_private(path, &bytes)
}

/// 加载 token 存储；文件不存在时生成一个默认 token。
/// 旧版裸 token 文件会就地迁移为 JSON（只保存 sha256）。
///
/// 返回 `(store, newly_created_plaintext)`。仅当文件不存在/为空、刚刚生成
/// 新 token 时第二个元素为 `Some`。
pub fn load_token_store(path: &Path) -> Result<(TokenStore, Option<String>), String> {
    match read_token_file(path)? {
        Some(content) => Ok((decode_token_store(path, &content)?, None)),
        None => {
            let token = random_secret();
            let store = TokenStore::with_token(&token);
            save_token_store(path, &store)?;
            Ok((store, Some(token)))
        }
    }
}

/// 供 CLI 编辑使用：文件不存在时返回空 store（不自动生成 token）。
pub fn load_token_store_for_update(path: &Path) -> Result<TokenStore, String> {
    match read_token_file(path)? {
        Some(content) => decode_token_store(path, &content),
        None => Ok(TokenStore::default()),
    }
}

fn read_token_file(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn decode_token_store(path: &Path, content: &str) -> Result<TokenStore, String> {
    if content.starts_with('{') {
        return serde_json::from_str(content)
            .map_err(|e| format!("{}: invalid token store: {e}", path.display()));
    }
    // 旧格式：文件内容就是裸 token。迁移为 JSON（仅保存 sha256）。
    let store = TokenStore::with_token(content);
    save_token_store(path, &store)?;
    Ok(store)
}

// ============================================================
// 用户 / 2FA 存储
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub name: String,
    pub argon2_hash: String,
    #[serde(default)]
    pub totp_secret: Option<String>,
    #[serde(default)]
    pub totp_enabled: bool,
    #[serde(default)]
    pub backup_code_hashes: Vec<String>,
    pub created_at: String,
}

impl User {
    pub fn verify_totp(&self, code: &str) -> bool {
        match &self.totp_secret {
            Some(secret) => verify_totp(secret, code),
            None => false,
        }
    }

    /// 校验并消费一个备份码（常数时间比较全部哈希）。
    pub fn consume_backup_code(&mut self, code: &str) -> bool {
        let normalized = normalize_backup_code(code);
        if normalized.len() != 10 {
            return false;
        }
        let hash = sha256_hex(&normalized);
        let mut found = None;
        for (i, stored) in self.backup_code_hashes.iter().enumerate() {
            if ct_eq_str(stored, &hash) {
                found = Some(i);
            }
        }
        match found {
            Some(i) => {
                self.backup_code_hashes.remove(i);
                true
            }
            None => false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthStore {
    #[serde(default)]
    pub users: Vec<User>,
}

impl AuthStore {
    pub fn find(&self, name: &str) -> Option<&User> {
        self.users.iter().find(|u| u.name == name)
    }

    pub fn find_mut(&mut self, name: &str) -> Option<&mut User> {
        self.users.iter_mut().find(|u| u.name == name)
    }

    pub fn is_empty(&self) -> bool {
        self.users.is_empty()
    }

    pub fn add_user(&mut self, name: &str, password: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("user name must not be empty".to_string());
        }
        if name.contains(':') {
            return Err("user name must not contain ':'".to_string());
        }
        if self.users.iter().any(|u| u.name == name) {
            return Err(format!("user already exists: {name}"));
        }
        let argon2_hash = hash_password(password)?;
        self.users.push(User {
            name: name.to_string(),
            argon2_hash,
            totp_secret: None,
            totp_enabled: false,
            backup_code_hashes: Vec::new(),
            created_at: now_rfc3339(),
        });
        Ok(())
    }

    pub fn set_password(&mut self, name: &str, password: &str) -> Result<(), String> {
        let hash = hash_password(password)?;
        match self.find_mut(name) {
            Some(user) => {
                user.argon2_hash = hash;
                Ok(())
            }
            None => Err(format!("user not found: {name}")),
        }
    }

    pub fn remove_user(&mut self, name: &str) -> bool {
        let before = self.users.len();
        self.users.retain(|u| u.name != name);
        before != self.users.len()
    }

    /// 校验用户名 + 密码。用户不存在时也执行一次哈希校验，降低枚举时序差异。
    pub fn verify_password(&self, name: &str, password: &str) -> bool {
        match self.find(name) {
            Some(user) => verify_password_hash(&user.argon2_hash, password),
            None => {
                // 固定 dummy 哈希（对应随机口令），只用于消耗时间。
                verify_password_hash(dummy_argon2_hash(), password)
            }
        }
    }
}

/// 未知用户时用于时间对齐的 Argon2id 哈希（口令不可知）。
fn dummy_argon2_hash() -> &'static str {
    use std::sync::OnceLock;
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| hash_password("__timing_dummy_password__").unwrap_or_default())
}

pub fn hash_password(password: &str) -> Result<String, String> {
    if password.is_empty() {
        return Err("password must not be empty".to_string());
    }
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| format!("argon2: {e}"))
}

pub fn verify_password_hash(hash: &str, password: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(p) => p,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub fn load_auth_store(path: &Path) -> Result<AuthStore, String> {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            if content.trim().is_empty() {
                return Ok(AuthStore::default());
            }
            serde_json::from_str(&content)
                .map_err(|e| format!("{}: invalid auth store: {e}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AuthStore::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub fn save_auth_store(path: &Path, store: &AuthStore) -> Result<(), String> {
    let mut data = serde_json::to_vec_pretty(store).map_err(|e| e.to_string())?;
    data.push(b'\n');
    write_private(path, &data)
}

// ============================================================
// TOTP / 备份码
// ============================================================

/// 备份码字符集：去掉易混淆的 0/O/1/I。
const BACKUP_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
pub const BACKUP_CODE_LEN: usize = 10;

fn totp_for(secret_b32: &str, account: &str) -> Result<TOTP, String> {
    let bytes = Secret::Encoded(secret_b32.to_string())
        .to_bytes()
        .map_err(|e| format!("invalid totp secret: {e}"))?;
    // issuer / account 中的 ':' 会破坏 otpauth URL，替换掉（不影响校验）。
    let account = account.replace(':', "_");
    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        bytes,
        Some(core::PROJECT_NAME.to_string()),
        account,
    )
    .map_err(|e| format!("invalid totp parameters: {e}"))
}

/// 生成 base32 编码的 TOTP secret（160 bit）。
pub fn generate_totp_secret() -> String {
    let mut raw = [0u8; 20];
    rand::thread_rng().fill_bytes(&mut raw);
    Secret::Raw(raw.to_vec()).to_encoded().to_string()
}

pub fn otpauth_url(secret_b32: &str, account: &str) -> Result<String, String> {
    Ok(totp_for(secret_b32, account)?.get_url())
}

pub fn verify_totp(secret_b32: &str, code: &str) -> bool {
    let code = code.trim();
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    match totp_for(secret_b32, "") {
        Ok(totp) => totp.check_current(code).unwrap_or(false),
        Err(_) => false,
    }
}

/// 仅用于测试/CLI 调试：生成当前 TOTP。
#[allow(dead_code)]
pub fn current_totp(secret_b32: &str) -> Result<String, String> {
    let totp = totp_for(secret_b32, "")?;
    totp.generate_current().map_err(|e| e.to_string())
}

pub fn generate_backup_codes(count: usize) -> Vec<String> {
    let mut rng = rand::thread_rng();
    (0..count)
        .map(|_| {
            (0..BACKUP_CODE_LEN)
                .map(|_| BACKUP_ALPHABET[rng.gen_range(0..BACKUP_ALPHABET.len())] as char)
                .collect()
        })
        .collect()
}

pub fn normalize_backup_code(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

pub fn hash_backup_code(code: &str) -> String {
    sha256_hex(&normalize_backup_code(code))
}

// ============================================================
// 二维码
// ============================================================

pub fn qr_svg(data: &str) -> Result<String, String> {
    let code = QrCode::new(data.as_bytes()).map_err(|e| format!("qr encode failed: {e}"))?;
    Ok(code
        .render::<svg::Color>()
        .min_dimensions(256, 256)
        .quiet_zone(true)
        .build())
}

/// 终端 ASCII 二维码（每个模块两个字符宽）。
pub fn qr_ascii(data: &str) -> Result<String, String> {
    let code = QrCode::new(data.as_bytes()).map_err(|e| format!("qr encode failed: {e}"))?;
    Ok(code
        .render::<&str>()
        .dark_color("##")
        .light_color("  ")
        .build())
}

// ============================================================
// 登录限流
// ============================================================

#[derive(Debug, Default)]
struct Attempt {
    /// 当前一分钟窗口内的失败时间。
    failures: VecDeque<Instant>,
    /// 自上次成功/锁定以来的累计失败数。
    total: u32,
    locked_until: Option<Instant>,
}

impl Attempt {
    fn prune(&mut self, now: Instant) {
        while let Some(t) = self.failures.front() {
            if now.duration_since(*t) >= RATE_WINDOW {
                self.failures.pop_front();
            } else {
                break;
            }
        }
    }
}

/// 进程内 (IP, username) 登录限流器。
#[derive(Debug, Default)]
pub struct RateLimiter {
    attempts: HashMap<(String, String), Attempt>,
}

impl RateLimiter {
    /// 是否允许本次尝试。锁定期 / 窗口内失败过多 → false。
    pub fn check(&mut self, ip: &str, username: &str) -> bool {
        let key = (ip.to_string(), username.to_string());
        let now = Instant::now();
        let Some(attempt) = self.attempts.get_mut(&key) else {
            return true;
        };
        attempt.prune(now);
        if let Some(until) = attempt.locked_until {
            if now < until {
                return false;
            }
            attempt.locked_until = None;
            attempt.total = 0;
        }
        attempt.failures.len() < RATE_MAX_FAILURES
    }

    pub fn record_failure(&mut self, ip: &str, username: &str) {
        let key = (ip.to_string(), username.to_string());
        let now = Instant::now();
        let attempt = self.attempts.entry(key).or_default();
        attempt.prune(now);
        attempt.failures.push_back(now);
        attempt.total += 1;
        if attempt.total >= LOCKOUT_AFTER {
            attempt.locked_until = Some(now + LOCKOUT_DURATION);
            attempt.total = 0;
            attempt.failures.clear();
        }
    }

    pub fn record_success(&mut self, ip: &str, username: &str) {
        self.attempts
            .remove(&(ip.to_string(), username.to_string()));
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn token_store_add_verify_revoke() {
        let mut store = TokenStore::default();
        let (entry, token) = store.add_token("phone", None).unwrap();
        assert_eq!(entry.id, "t1");
        assert!(store.verify(&token));
        assert!(!store.verify("nope"));
        assert_eq!(store.revoke("phone").unwrap(), 1);
        assert!(!store.verify(&token));
    }

    #[test]
    fn expired_token_is_rejected() {
        let mut store = TokenStore::default();
        let past =
            (Utc::now() - chrono::Duration::days(1)).to_rfc3339_opts(SecondsFormat::Secs, true);
        let (_entry, token) = store
            .add_token("old", Some("2099-01-01T00:00:00Z".to_string()))
            .unwrap();
        assert!(store.verify(&token));
        store.tokens[0].expires_at = Some(past);
        assert!(!store.verify(&token));
    }

    #[test]
    fn backup_codes_are_consumed_once() {
        let mut user = User {
            name: "a".to_string(),
            argon2_hash: hash_password("pw").unwrap(),
            totp_secret: None,
            totp_enabled: false,
            backup_code_hashes: Vec::new(),
            created_at: now_rfc3339(),
        };
        let codes = generate_backup_codes(3);
        user.backup_code_hashes = codes.iter().map(|c| hash_backup_code(c)).collect();
        let code = codes[0].to_lowercase();
        assert!(user.consume_backup_code(&code));
        assert!(!user.consume_backup_code(&code));
    }
}
