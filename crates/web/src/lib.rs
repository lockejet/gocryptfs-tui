//! `gocryptfs-ui-web` — gocryptfs-tui 的 Web UI 后端。
//!
//! 由 `gocryptfs-web` 二进制调用。提供：
//!   * 基于 token + cookie 的会话认证（12h TTL）；
//!   * 卷 mount / umount 的后台任务与 SSE 事件流；
//!   * 元信息、任务列表、JSONL 日志查询；
//!   * 内嵌前端静态资源（SPA 回退）。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, IsTerminal, Write as _};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::connect_info::ConnectInfo;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::broadcast;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use gocryptfs_ui_core as core;
use rust_embed::RustEmbed;

mod auth;

#[cfg(test)]
mod tests;

const SESSION_TTL: Duration = Duration::from_secs(12 * 60 * 60);
const SESSION_COOKIE: &str = "gfs_session";
const CSRF_HEADER: &str = "x-gfs-csrf";
// 端口约定：gocryptfs-ui 用 30032（反代 44332 → 本机 30032）
const DEFAULT_LISTEN: &str = "127.0.0.1:30032";

// ============================================================
// 内嵌前端
// ============================================================

/// 前端构建产物（`web/dist`）。占位页已随仓库提供，真实构建会覆盖它。
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../web/dist"]
struct Assets;

// ============================================================
// 状态
// ============================================================

/// Web 服务共享状态（测试可直接构造）。
#[derive(Clone)]
pub struct AppState {
    /// 多 token 存储（只含 sha256，不含明文）。
    pub tokens: Arc<Mutex<auth::TokenStore>>,
    /// 用户名 / 密码 / TOTP 存储。
    pub users: Arc<Mutex<auth::AuthStore>>,
    /// `web.auth` 路径（CLI 与 HTTP 共用）。
    pub auth_path: PathBuf,
    /// token 文件路径（CLI 与 HTTP 共用，用于热加载）。
    pub token_path: PathBuf,
    /// 上次加载 `web.auth` 的 mtime（CLI 改用户后自动热加载）。
    pub auth_mtime: Arc<Mutex<Option<std::time::SystemTime>>>,
    /// 上次加载 token 文件的 mtime。
    pub token_mtime: Arc<Mutex<Option<std::time::SystemTime>>>,
    /// 对外可访问的基址，例如 `http://127.0.0.1:30032`。
    pub listen_url: String,
    /// 用户配置的对外公开基址（`--public-url`，反代后使用），优先于 listen_url。
    pub public_url: Option<String>,
    /// 是否信任反向代理头（X-Forwarded-Proto / Host / For）。
    pub trust_proxy: bool,
    /// 一次性登录 nonce → 签发时间。
    pub nonces: Arc<Mutex<HashMap<String, Instant>>>,
    /// 进程内登录限流器。
    pub rate_limiter: Arc<Mutex<auth::RateLimiter>>,
    pub sessions: Arc<Mutex<HashMap<String, Instant>>>,
    /// 密码登录时记录 session 对应的用户名（token 登录为 None）。
    pub session_users: Arc<Mutex<HashMap<String, String>>>,
    pub config: String,
    pub data_dir: PathBuf,
    pub data_overridden: bool,
    pub state_dir: PathBuf,
    pub log_path: PathBuf,
    pub jobs: Arc<Mutex<HashMap<u64, Arc<Job>>>>,
    pub next_job_id: Arc<AtomicU64>,
}

/// 文件 mtime（不存在则 None），用于判断 `web.auth`/token 文件是否被 CLI 改动。
fn file_mtime(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

impl AppState {
    /// 热加载：CLI（`user add` / `token add` 等）改动文件后，无需重启即可生效。
    pub fn reload_auth_if_changed(&self) {
        let auth_stamp = file_mtime(&self.auth_path);
        {
            let mut last = self.auth_mtime.lock().unwrap();
            if *last != auth_stamp {
                if let Ok(store) = auth::load_auth_store(&self.auth_path) {
                    *self.users.lock().unwrap() = store;
                }
                *last = auth_stamp;
            }
        }
        let token_stamp = file_mtime(&self.token_path);
        {
            let mut last = self.token_mtime.lock().unwrap();
            if *last != token_stamp {
                if let Ok((store, _)) = auth::load_token_store(&self.token_path) {
                    *self.tokens.lock().unwrap() = store;
                }
                *last = token_stamp;
            }
        }
    }

    pub fn new(
        token: String,
        config: String,
        data_dir: PathBuf,
        data_overridden: bool,
        state_dir: PathBuf,
    ) -> Self {
        Self::with_token_store(
            auth::TokenStore::with_token(&token),
            config,
            data_dir,
            data_overridden,
            state_dir,
        )
    }

    /// 用已有的 token store 构造（服务启动时从 JSON 文件加载）。
    pub fn with_token_store(
        store: auth::TokenStore,
        config: String,
        data_dir: PathBuf,
        data_overridden: bool,
        state_dir: PathBuf,
    ) -> Self {
        let log_path = state_dir.join("app.log.jsonl");
        let auth_path = state_dir.join("web.auth");
        let token_path = PathBuf::new();
        Self {
            tokens: Arc::new(Mutex::new(store)),
            users: Arc::new(Mutex::new(auth::AuthStore::default())),
            auth_path,
            token_path,
            auth_mtime: Arc::new(Mutex::new(None)),
            token_mtime: Arc::new(Mutex::new(None)),
            listen_url: format!("http://{DEFAULT_LISTEN}"),
            public_url: None,
            trust_proxy: false,
            nonces: Arc::new(Mutex::new(HashMap::new())),
            rate_limiter: Arc::new(Mutex::new(auth::RateLimiter::default())),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            session_users: Arc::new(Mutex::new(HashMap::new())),
            config,
            data_dir,
            data_overridden,
            state_dir,
            log_path,
            jobs: Arc::new(Mutex::new(HashMap::new())),
            next_job_id: Arc::new(AtomicU64::new(1)),
        }
    }

    fn session_valid(&self, id: &str) -> bool {
        let mut map = self.sessions.lock().unwrap();
        match map.get(id).copied() {
            Some(started) if started.elapsed() < SESSION_TTL => true,
            Some(_) => {
                map.remove(id);
                self.session_users.lock().unwrap().remove(id);
                false
            }
            None => false,
        }
    }

    fn session_user(&self, id: &str) -> Option<String> {
        if !self.session_valid(id) {
            return None;
        }
        self.session_users.lock().unwrap().get(id).cloned()
    }

    fn start_session(&self, user: Option<&str>) -> String {
        let mut raw = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut raw);
        let session = URL_SAFE_NO_PAD.encode(raw);
        self.sessions
            .lock()
            .unwrap()
            .insert(session.clone(), Instant::now());
        if let Some(user) = user {
            self.session_users
                .lock()
                .unwrap()
                .insert(session.clone(), user.to_string());
        }
        session
    }

    /// 签发一次性登录 nonce（32 字节随机，120s TTL）。
    pub fn issue_login_nonce(&self) -> String {
        let nonce = auth::random_secret();
        let mut map = self.nonces.lock().unwrap();
        // 顺手清理过期 nonce，避免长期运行时 map 无限增长。
        map.retain(|_, started| started.elapsed() < auth::LOGIN_NONCE_TTL);
        map.insert(nonce.clone(), Instant::now());
        nonce
    }

    /// 消费 nonce；无论成功与否都会从存储中删除（single-use）。
    pub fn consume_login_nonce(&self, nonce: &str) -> bool {
        let started = self.nonces.lock().unwrap().remove(nonce);
        match started {
            Some(started) => started.elapsed() < auth::LOGIN_NONCE_TTL,
            None => false,
        }
    }

    pub fn login_url(&self, nonce: &str) -> String {
        format!(
            "{}/#t={nonce}",
            self.public_url
                .as_deref()
                .unwrap_or(&self.listen_url)
                .trim_end_matches('/')
        )
    }
}

/// 一个后台任务（mount / umount）
pub struct Job {
    pub id: u64,
    pub kind: String,
    pub target: String,
    pub state: Mutex<JobState>,
    pub child: Arc<Mutex<Option<std::process::Child>>>,
    pub tx: broadcast::Sender<JobEvent>,
}

#[derive(Debug, Clone, Default)]
pub struct JobState {
    pub running: bool,
    pub exit_code: Option<i32>,
    pub cancelled: bool,
}

impl Job {
    fn finished(&self) -> Option<i32> {
        let st = self.state.lock().unwrap();
        if st.running {
            None
        } else {
            Some(st.exit_code.unwrap_or(-1))
        }
    }
}

/// SSE / 广播事件
#[derive(Debug, Clone)]
pub enum JobEvent {
    Progress { pct: u8, done: u64, total: u64 },
    Log { line: String },
    Check { key: String, value: String },
    Done { code: i32 },
    Error { message: String },
}

impl JobEvent {
    fn into_sse(self) -> Event {
        let (event, data) = match self {
            JobEvent::Progress { pct, done, total } => (
                "progress",
                json!({ "pct": pct, "done": done, "total": total }),
            ),
            JobEvent::Log { line } => ("log", json!({ "line": line })),
            JobEvent::Check { key, value } => ("check", json!({ "key": key, "value": value })),
            JobEvent::Done { code } => ("done", json!({ "code": code })),
            JobEvent::Error { message } => ("error", json!({ "message": message })),
        };
        let payload = serde_json::to_string(&data).unwrap_or_else(|_| "{}".to_string());
        Event::default().event(event).data(payload)
    }
}

// ============================================================
// 认证与安全中间件
// ============================================================

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    for value in headers.get_all(header::COOKIE) {
        let Ok(text) = value.to_str() else { continue };
        for part in text.split(';') {
            let part = part.trim();
            if let Some(rest) = part.strip_prefix(&prefix) {
                return Some(rest.to_string());
            }
        }
    }
    None
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "unauthorized" })),
    )
        .into_response()
}

fn forbidden(error: &str) -> Response {
    (StatusCode::FORBIDDEN, Json(json!({ "error": error }))).into_response()
}

/// 取 `Origin` 的 host[:port] 部分。
fn origin_host(origin: &str) -> Option<String> {
    let rest = origin.split_once("://").map(|(_, r)| r).unwrap_or(origin);
    let authority = rest.split('/').next()?;
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    if authority.is_empty() {
        None
    } else {
        Some(authority.to_ascii_lowercase())
    }
}

/// 反向代理后的实际 Host（信任代理时优先 `X-Forwarded-Host`）。
fn effective_host(state: &AppState, headers: &HeaderMap) -> Option<String> {
    if state.trust_proxy {
        if let Some(xfh) = headers
            .get("x-forwarded-host")
            .and_then(|v| v.to_str().ok())
        {
            if let Some(first) = xfh
                .split(',')
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(first.to_ascii_lowercase());
            }
        }
    }
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|h| h.trim().to_ascii_lowercase())
        .filter(|h| !h.is_empty())
}

/// `Origin` 是否与请求 Host 或配置的 public-url 匹配。
fn origin_allowed(state: &AppState, origin: &str, headers: &HeaderMap) -> bool {
    let Some(origin_authority) = origin_host(origin) else {
        return false;
    };
    if effective_host(state, headers).as_deref() == Some(origin_authority.as_str()) {
        return true;
    }
    state
        .public_url
        .as_deref()
        .and_then(origin_host)
        .map(|h| h == origin_authority)
        .unwrap_or(false)
}

async fn auth_mw(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    if !path.starts_with("/api") {
        return next.run(req).await;
    }
    // 登录入口与认证能力探测无需会话。
    if path == "/api/session" || path == "/api/auth/status" {
        return next.run(req).await;
    }

    let session = cookie_value(req.headers(), SESSION_COOKIE);
    let valid = match &session {
        Some(id) => state.session_valid(id),
        None => false,
    };
    if !valid {
        return unauthorized();
    }

    let method = req.method().clone();
    if method != Method::GET && method != Method::HEAD {
        if req.headers().get(CSRF_HEADER).and_then(|v| v.to_str().ok()) != Some("1") {
            return forbidden("missing csrf header");
        }
        if let Some(origin) = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
        {
            if !origin_allowed(&state, origin, req.headers()) {
                return forbidden("origin mismatch");
            }
        }
    }

    next.run(req).await
}

async fn security_headers(req: Request<axum::body::Body>, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
             img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'",
        ),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    res
}

// ============================================================
// 会话 / 登录
// ============================================================

#[derive(Deserialize, Default)]
struct LoginRequest {
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    totp: Option<String>,
}

/// 请求是否可视为 HTTPS：反代（信任时）转发 `X-Forwarded-Proto: https`，
/// 或配置的 `--public-url` 本身就是 https。
fn request_is_secure(state: &AppState, headers: &HeaderMap) -> bool {
    if state.trust_proxy {
        let proto = headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.trim().to_ascii_lowercase());
        if proto.as_deref() == Some("https") {
            return true;
        }
    }
    state
        .public_url
        .as_deref()
        .map(|u| u.starts_with("https://"))
        .unwrap_or(false)
}

fn session_cookie(state: &AppState, headers: &HeaderMap, value: &str, max_age: u64) -> String {
    let secure = if request_is_secure(state, headers) {
        "; Secure"
    } else {
        ""
    };
    format!(
        "{SESSION_COOKIE}={value}; HttpOnly; SameSite=Strict; Path=/{secure}; Max-Age={max_age}"
    )
}

fn session_response(state: &AppState, user: Option<&str>, headers: &HeaderMap) -> Response {
    let session = state.start_session(user);
    let cookie = session_cookie(state, headers, &session, SESSION_TTL.as_secs());
    (StatusCode::NO_CONTENT, [(header::SET_COOKIE, cookie)]).into_response()
}

fn error_response(status: StatusCode, error: &str) -> Response {
    (status, Json(json!({ "error": error }))).into_response()
}

fn internal_error(message: impl std::fmt::Display) -> Response {
    error_response(StatusCode::INTERNAL_SERVER_ERROR, &message.to_string())
}

fn invalid_credentials() -> Response {
    error_response(StatusCode::UNAUTHORIZED, "invalid credentials")
}

fn invalid_totp() -> Response {
    error_response(StatusCode::UNAUTHORIZED, "invalid totp")
}

fn totp_required() -> Response {
    error_response(StatusCode::UNAUTHORIZED, "totp_required")
}

fn too_many_attempts() -> Response {
    error_response(StatusCode::TOO_MANY_REQUESTS, "too many attempts")
}

/// 追加一条认证审计事件（绝不包含 token / nonce / 哈希 / 密码）。
fn audit(state: &AppState, action: &str, target: &str, result: &str, detail: &str) {
    let entry = json!({
        "ts": auth::now_rfc3339(),
        "src": "web",
        "action": action,
        "target": target,
        "result": result,
        "detail": detail,
        "pid": null,
        "duration_ms": null,
    });
    if let Some(parent) = state.log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.log_path)
    {
        let _ = writeln!(file, "{entry}");
    }
}

fn client_ip(
    state: &AppState,
    headers: &HeaderMap,
    conn: Option<ConnectInfo<SocketAddr>>,
) -> String {
    if state.trust_proxy {
        if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            if let Some(first) = xff
                .split(',')
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return first.to_string();
            }
        }
    }
    conn.map(|ConnectInfo(addr)| addr.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn password_login(
    state: &AppState,
    headers: &HeaderMap,
    ip: &str,
    username: &str,
    password: &str,
    totp: Option<&str>,
) -> Response {
    if !state.rate_limiter.lock().unwrap().check(ip, username) {
        audit(state, "login", username, "failed", "rate limited");
        return too_many_attempts();
    }

    let mut users = state.users.lock().unwrap();
    if !users.verify_password(username, password) {
        drop(users);
        state
            .rate_limiter
            .lock()
            .unwrap()
            .record_failure(ip, username);
        audit(state, "login", username, "failed", "invalid credentials");
        return invalid_credentials();
    }

    let mut backup_used = false;
    let mut need_totp = false;
    let mut totp_ok = true;
    if let Some(user) = users.find_mut(username) {
        if user.totp_enabled {
            let code = totp.unwrap_or("").trim();
            if code.is_empty() {
                need_totp = true;
                totp_ok = false;
            } else if user.verify_totp(code) {
                // 正常 TOTP
            } else if user.consume_backup_code(code) {
                backup_used = true;
            } else {
                totp_ok = false;
            }
        }
    }

    if need_totp {
        drop(users);
        audit(state, "login", username, "failed", "totp required");
        return totp_required();
    }
    if !totp_ok {
        drop(users);
        state
            .rate_limiter
            .lock()
            .unwrap()
            .record_failure(ip, username);
        audit(state, "login", username, "failed", "invalid totp");
        return invalid_totp();
    }
    if backup_used {
        if let Err(e) = auth::save_auth_store(&state.auth_path, &users) {
            drop(users);
            return internal_error(e);
        }
    }
    drop(users);

    state
        .rate_limiter
        .lock()
        .unwrap()
        .record_success(ip, username);
    audit(state, "login", username, "success", "password");
    session_response(state, Some(username), headers)
}

async fn login(
    State(state): State<AppState>,
    conn: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Response {
    state.reload_auth_if_changed();
    if let Some(token) = body.token.as_deref() {
        if state.tokens.lock().unwrap().verify(token) {
            // 兼容既有日志测试：token 登录成功不写入 app.log.jsonl，
            // 仅记录失败，避免改变 /api/logs 的既有语义。
            return session_response(&state, None, &headers);
        }
        audit(&state, "login", "token", "failed", "invalid token");
        return unauthorized();
    }

    if let Some(nonce) = body.nonce.as_deref() {
        if !state.consume_login_nonce(nonce) {
            audit(&state, "login", "login-link", "failed", "invalid nonce");
            return unauthorized();
        }
        audit(&state, "login", "login-link", "success", "nonce");
        return session_response(&state, None, &headers);
    }

    if let Some(username) = body.username.as_deref() {
        let ip = client_ip(&state, &headers, conn);
        let password = body.password.as_deref().unwrap_or("");
        return password_login(
            &state,
            &headers,
            &ip,
            username,
            password,
            body.totp.as_deref(),
        );
    }

    invalid_credentials()
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(session) = cookie_value(&headers, SESSION_COOKIE) {
        state.sessions.lock().unwrap().remove(&session);
        state.session_users.lock().unwrap().remove(&session);
    }
    let cookie = session_cookie(&state, &headers, "", 0);
    (StatusCode::NO_CONTENT, [(header::SET_COOKIE, cookie)]).into_response()
}

// ============================================================
// 一次性登录链接 / 认证状态
// ============================================================

fn request_base_url(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(public_url) = &state.public_url {
        return public_url.trim_end_matches('/').to_string();
    }
    let https = state.trust_proxy
        && headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.trim().eq_ignore_ascii_case("https"))
            .unwrap_or(false);
    match effective_host(state, headers) {
        Some(host) => format!("{}://{host}", if https { "https" } else { "http" }),
        None => state.listen_url.trim_end_matches('/').to_string(),
    }
}

async fn login_links(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let nonce = state.issue_login_nonce();
    let base = request_base_url(&state, &headers);
    let url = format!("{base}/#t={nonce}");
    let svg = match auth::qr_svg(&url) {
        Ok(svg) => svg,
        Err(e) => return internal_error(e),
    };
    if !cfg!(test) {
        println!();
        println!("one-time login link (valid 120s):");
        println!("{url}");
        if let Ok(ascii) = auth::qr_ascii(&url) {
            println!("{ascii}");
        }
    }
    audit(&state, "login_link", "-", "success", "created");
    Json(json!({
        "url": url,
        "expires_in": auth::LOGIN_NONCE_TTL.as_secs(),
        "svg": svg,
    }))
    .into_response()
}

async fn auth_status(State(state): State<AppState>) -> Response {
    state.reload_auth_if_changed();
    let password_auth = !state.users.lock().unwrap().is_empty();
    Json(json!({
        "token_auth": true,
        "password_auth": password_auth,
        "2fa_available": true,
    }))
    .into_response()
}

// ============================================================
// 2FA
// ============================================================

#[derive(Deserialize, Default)]
struct CodeRequest {
    #[serde(default)]
    code: String,
}

/// 2FA 操作需要知道“当前用户”。密码登录的 session 带用户名；
/// token 登录没有用户名时，若系统恰好只有一个用户则自动使用它。
#[allow(clippy::result_large_err)]
fn resolve_session_user(state: &AppState, headers: &HeaderMap) -> Result<String, Response> {
    if let Some(session) = cookie_value(headers, SESSION_COOKIE) {
        if let Some(user) = state.session_user(&session) {
            return Ok(user);
        }
    }
    let users = state.users.lock().unwrap();
    match users.users.len() {
        1 => Ok(users.users[0].name.clone()),
        0 => Err(error_response(
            StatusCode::BAD_REQUEST,
            "no users configured",
        )),
        _ => Err(error_response(StatusCode::CONFLICT, "no user session")),
    }
}

async fn twofa_enroll(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let username = match resolve_session_user(&state, &headers) {
        Ok(user) => user,
        Err(resp) => return resp,
    };
    let secret = auth::generate_totp_secret();
    let otpauth_url = match auth::otpauth_url(&secret, &username) {
        Ok(url) => url,
        Err(e) => return internal_error(e),
    };
    let svg = match auth::qr_svg(&otpauth_url) {
        Ok(svg) => svg,
        Err(e) => return internal_error(e),
    };
    {
        let mut users = state.users.lock().unwrap();
        let Some(user) = users.find_mut(&username) else {
            return error_response(StatusCode::NOT_FOUND, "user not found");
        };
        // 重新 enroll：覆盖待启用 secret，并保持未启用状态。
        user.totp_secret = Some(secret.clone());
        user.totp_enabled = false;
        user.backup_code_hashes.clear();
        if let Err(e) = auth::save_auth_store(&state.auth_path, &users) {
            return internal_error(e);
        }
    }
    audit(&state, "2fa_enroll", &username, "success", "pending");
    Json(json!({
        "secret": secret,
        "otpauth_url": otpauth_url,
        "svg": svg,
    }))
    .into_response()
}

async fn twofa_activate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CodeRequest>,
) -> Response {
    let username = match resolve_session_user(&state, &headers) {
        Ok(user) => user,
        Err(resp) => return resp,
    };
    let mut users = state.users.lock().unwrap();
    let Some(user) = users.find_mut(&username) else {
        return error_response(StatusCode::NOT_FOUND, "user not found");
    };
    let Some(secret) = user.totp_secret.clone() else {
        return error_response(StatusCode::BAD_REQUEST, "2fa not enrolled");
    };
    if !auth::verify_totp(&secret, &body.code) {
        drop(users);
        audit(&state, "2fa_activate", &username, "failed", "invalid totp");
        return invalid_totp();
    }
    user.totp_enabled = true;
    let backup_codes = auth::generate_backup_codes(8);
    user.backup_code_hashes = backup_codes
        .iter()
        .map(|code| auth::hash_backup_code(code))
        .collect();
    if let Err(e) = auth::save_auth_store(&state.auth_path, &users) {
        return internal_error(e);
    }
    drop(users);
    audit(&state, "2fa_activate", &username, "success", "enabled");
    Json(json!({ "backup_codes": backup_codes })).into_response()
}

async fn twofa_disable(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CodeRequest>,
) -> Response {
    let username = match resolve_session_user(&state, &headers) {
        Ok(user) => user,
        Err(resp) => return resp,
    };
    let mut users = state.users.lock().unwrap();
    let Some(user) = users.find_mut(&username) else {
        return error_response(StatusCode::NOT_FOUND, "user not found");
    };
    if !user.totp_enabled {
        return error_response(StatusCode::BAD_REQUEST, "2fa not enabled");
    }
    let code = body.code.trim();
    let ok = user.verify_totp(code) || user.consume_backup_code(code);
    if !ok {
        drop(users);
        audit(&state, "2fa_disable", &username, "failed", "invalid code");
        return invalid_totp();
    }
    user.totp_secret = None;
    user.totp_enabled = false;
    user.backup_code_hashes.clear();
    if let Err(e) = auth::save_auth_store(&state.auth_path, &users) {
        return internal_error(e);
    }
    drop(users);
    audit(&state, "2fa_disable", &username, "success", "disabled");
    StatusCode::NO_CONTENT.into_response()
}

/// 当前会话的 2FA 状态：用于「登录后未启用 2FA 时提示启用」。
/// token 会话无法绑定 2FA（2FA 属于用户名密码账户），返回 can_enable=false。
async fn twofa_status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let username = match resolve_session_user(&state, &headers) {
        Ok(u) => u,
        Err(_) => {
            return Json(json!({
                "user": null,
                "enabled": false,
                "can_enable": false,
                "backup_codes": 0,
            }))
            .into_response();
        }
    };
    let users = state.users.lock().unwrap();
    let (enabled, backup_codes) = match users.find(&username) {
        Some(u) => (u.totp_enabled, u.backup_code_hashes.len()),
        None => (false, 0),
    };
    Json(json!({
        "user": username,
        "enabled": enabled,
        "can_enable": true,
        "backup_codes": backup_codes,
    }))
    .into_response()
}

async fn twofa_backup_codes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CodeRequest>,
) -> Response {
    let username = match resolve_session_user(&state, &headers) {
        Ok(user) => user,
        Err(resp) => return resp,
    };
    let mut users = state.users.lock().unwrap();
    let Some(user) = users.find_mut(&username) else {
        return error_response(StatusCode::NOT_FOUND, "user not found");
    };
    if !user.totp_enabled {
        return error_response(StatusCode::BAD_REQUEST, "2fa not enabled");
    }
    if !user.verify_totp(&body.code) {
        drop(users);
        audit(
            &state,
            "2fa_backup_codes",
            &username,
            "failed",
            "invalid totp",
        );
        return invalid_totp();
    }
    let backup_codes = auth::generate_backup_codes(8);
    user.backup_code_hashes = backup_codes
        .iter()
        .map(|code| auth::hash_backup_code(code))
        .collect();
    if let Err(e) = auth::save_auth_store(&state.auth_path, &users) {
        return internal_error(e);
    }
    drop(users);
    audit(
        &state,
        "2fa_backup_codes",
        &username,
        "success",
        "regenerated",
    );
    Json(json!({ "backup_codes": backup_codes })).into_response()
}

// ============================================================
// 元信息
// ============================================================

async fn healthz() -> &'static str {
    "ok"
}

async fn meta(State(state): State<AppState>) -> Response {
    let missing_deps = core::deps::missing();
    let yq_bad = core::deps::yq_flavor_ok();
    let ok = !core::deps::has_problems(&missing_deps, yq_bad);
    let missing: Vec<String> = missing_deps.iter().map(|d| d.cmd.to_string()).collect();
    Json(json!({
        "version": core::version_string(),
        "project": core::PROJECT_NAME,
        "paths": {
            "config": state.config,
            "data": state.data_dir.to_string_lossy(),
            "state": state.state_dir.to_string_lossy(),
            "log": state.log_path.to_string_lossy(),
            "history": state.state_dir.join("history.jsonl").to_string_lossy(),
        },
        "deps": { "ok": ok, "missing": missing },
        "public_url": state.public_url,
        "trust_proxy": state.trust_proxy,
    }))
    .into_response()
}

// ============================================================
// 卷
// ============================================================

#[derive(Serialize)]
struct VaultDto {
    id: u32,
    name: String,
    path: String,
    mount_point: String,
    mounted: bool,
    locked: bool,
    valid: bool,
}

fn vault_error(status: StatusCode, message: impl std::fmt::Display) -> Response {
    (status, Json(json!({ "error": message.to_string() }))).into_response()
}

#[allow(clippy::result_large_err)]
fn load_vaults(state: &AppState) -> Result<Vec<core::vaults::Vault>, Response> {
    core::vaults::load_vaults(&state.config)
        .map_err(|e| vault_error(StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn vaults(State(state): State<AppState>) -> Response {
    match load_vaults(&state) {
        Ok(list) => {
            let out: Vec<VaultDto> = list
                .into_iter()
                .map(|v| VaultDto {
                    id: v.id,
                    name: v.name,
                    path: v.path,
                    mount_point: v.mount_point,
                    mounted: v.mounted,
                    locked: v.locked,
                    valid: v.valid,
                })
                .collect();
            Json(json!({ "vaults": out })).into_response()
        }
        Err(resp) => resp,
    }
}

#[derive(Deserialize)]
struct MountRequest {
    #[serde(default)]
    passphrase: String,
    /// 可选的界面语言（zh-CN / en-US），用于本地化任务日志。
    #[serde(default)]
    lang: Option<String>,
}

#[derive(Deserialize)]
struct UmountRequest {
    #[serde(default)]
    force: bool,
    /// 可选的界面语言（zh-CN / en-US），用于本地化任务日志。
    #[serde(default)]
    lang: Option<String>,
}

/// 只透传 CLI 支持的两种语言代码；其他值一律忽略（CLI 会回落到系统语言）。
fn normalize_lang(lang: Option<&str>) -> Option<&'static str> {
    match lang {
        Some("zh-CN") => Some("zh-CN"),
        Some("en-US") => Some("en-US"),
        _ => None,
    }
}

#[allow(clippy::result_large_err)]
fn find_vault(state: &AppState, name: &str) -> Result<(), Response> {
    let list = load_vaults(state)?;
    if list.iter().any(|v| v.name == name) {
        Ok(())
    } else {
        Err(vault_error(
            StatusCode::NOT_FOUND,
            format!("vault not found: {name}"),
        ))
    }
}

async fn mount_vault(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(body): Json<MountRequest>,
) -> Response {
    if let Err(resp) = find_vault(&state, &name) {
        return resp;
    }
    let lang = normalize_lang(body.lang.as_deref());
    match spawn_job(&state, "mount", &name, Some(body.passphrase), false, lang) {
        Ok(id) => (StatusCode::ACCEPTED, Json(json!({ "id": id }))).into_response(),
        Err(e) => vault_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn umount_vault(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(body): Json<UmountRequest>,
) -> Response {
    if let Err(resp) = find_vault(&state, &name) {
        return resp;
    }
    let lang = normalize_lang(body.lang.as_deref());
    match spawn_job(&state, "umount", &name, None, body.force, lang) {
        Ok(id) => (StatusCode::ACCEPTED, Json(json!({ "id": id }))).into_response(),
        Err(e) => vault_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

// ============================================================
// 任务
// ============================================================

/// 启动后台任务；返回任务 id。
fn spawn_job(
    state: &AppState,
    kind: &str,
    target: &str,
    passphrase: Option<String>,
    force: bool,
    lang: Option<&str>,
) -> Result<u64, String> {
    let id = state.next_job_id.fetch_add(1, Ordering::SeqCst);
    let (tx, _rx) = broadcast::channel(256);

    let mut cmd = core::backend::cli_command(&core::paths::cli_path());
    // 全局参数必须位于子命令之前：gocryptfs-cli [-l <lang>] <command> ...
    if let Some(lang) = lang {
        cmd.arg("-l").arg(lang);
    }
    cmd.arg(kind).arg(target);
    if force {
        cmd.arg("--force");
    }
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        let msg = format!("failed to start {kind} {target}: {e}");
        let _ = tx.send(JobEvent::Error {
            message: msg.clone(),
        });
        msg
    })?;

    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let child = Arc::new(Mutex::new(Some(child)));

    let job = Arc::new(Job {
        id,
        kind: kind.to_string(),
        target: target.to_string(),
        state: Mutex::new(JobState {
            running: true,
            exit_code: None,
            cancelled: false,
        }),
        child: child.clone(),
        tx: tx.clone(),
    });
    state.jobs.lock().unwrap().insert(id, job.clone());

    // 写入 passphrase（进程可能已退出，忽略错误）
    if let Some(mut pipe) = stdin {
        let pass = passphrase.unwrap_or_default();
        std::thread::spawn(move || {
            let _ = pipe.write_all(pass.as_bytes());
            let _ = pipe.write_all(b"\n");
        });
    }

    // stdout：原样作为 log 广播
    let stdout_tx = tx.clone();
    let stdout_thread = stdout.map(|out| {
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let _ = stdout_tx.send(JobEvent::Log { line });
            }
        })
    });

    // stderr：协议行解析 + 非协议行作为 log
    let stderr_tx = tx.clone();
    let stderr_thread = stderr.map(|err| {
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                match core::protocol::parse_protocol_line(&line) {
                    Some(core::protocol::CliEvent::Progress { pct, done, total }) => {
                        let _ = stderr_tx.send(JobEvent::Progress { pct, done, total });
                    }
                    Some(core::protocol::CliEvent::Check { key, value }) => {
                        let _ = stderr_tx.send(JobEvent::Check { key, value });
                    }
                    // core 的解析器目前不识别 @@DONE@@，这里补上。
                    _ => {
                        if let Some(rest) = line.strip_prefix("@@DONE@@ ") {
                            let _ = stderr_tx.send(JobEvent::Log { line: line.clone() });
                            let _ = stderr_tx.send(JobEvent::Done {
                                code: rest.trim().parse::<i32>().unwrap_or(0),
                            });
                        } else {
                            let _ = stderr_tx.send(JobEvent::Log { line });
                        }
                    }
                }
            }
        })
    });

    // 等待进程结束（轮询 try_wait，便于 cancel 线程 kill）
    let wait_child = child;
    let wait_tx = tx;
    let wait_job = job;
    std::thread::spawn(move || {
        let status = loop {
            let mut guard = wait_child.lock().unwrap();
            let mut done = None;
            if let Some(c) = guard.as_mut() {
                match c.try_wait() {
                    Ok(Some(status)) => done = Some(Some(status)),
                    Ok(None) => {}
                    Err(_) => done = Some(None),
                }
            }
            drop(guard);
            if let Some(status) = done {
                break status;
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        wait_child.lock().unwrap().take();
        if let Some(t) = stdout_thread {
            let _ = t.join();
        }
        if let Some(t) = stderr_thread {
            let _ = t.join();
        }
        let code = status.as_ref().and_then(|s| s.code()).unwrap_or(-1);
        {
            let mut st = wait_job.state.lock().unwrap();
            st.running = false;
            st.exit_code = Some(code);
        }
        let _ = wait_tx.send(JobEvent::Done { code });
    });

    Ok(id)
}

async fn list_jobs(State(state): State<AppState>) -> Response {
    let jobs = state.jobs.lock().unwrap();
    let mut list: Vec<Value> = jobs
        .values()
        .map(|j| {
            let st = j.state.lock().unwrap();
            json!({
                "id": j.id,
                "kind": j.kind,
                "target": j.target,
                "state": if st.running { "running" } else if st.cancelled { "cancelled" } else { "finished" },
                "exit_code": st.exit_code,
            })
        })
        .collect();
    list.sort_by_key(|v| v.get("id").and_then(|i| i.as_u64()).unwrap_or(0));
    Json(json!({ "jobs": list })).into_response()
}

async fn job_events(State(state): State<AppState>, AxumPath(id): AxumPath<u64>) -> Response {
    let job = match state.jobs.lock().unwrap().get(&id).cloned() {
        Some(j) => j,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "job not found" })),
            )
                .into_response()
        }
    };

    let mut rx = job.tx.subscribe();
    let finished = job.finished();
    let (tx, out_rx) = tokio::sync::mpsc::channel::<JobEvent>(64);
    tokio::spawn(async move {
        if let Some(code) = finished {
            let _ = tx.send(JobEvent::Done { code }).await;
            return;
        }
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let last = matches!(ev, JobEvent::Done { .. } | JobEvent::Error { .. });
                    if tx.send(ev).await.is_err() {
                        return;
                    }
                    if last {
                        return;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    });

    let stream =
        ReceiverStream::new(out_rx).map(|ev| Ok::<Event, std::convert::Infallible>(ev.into_sse()));
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

async fn cancel_job(State(state): State<AppState>, AxumPath(id): AxumPath<u64>) -> Response {
    let job = match state.jobs.lock().unwrap().get(&id).cloned() {
        Some(j) => j,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "job not found" })),
            )
                .into_response()
        }
    };
    {
        let mut st = job.state.lock().unwrap();
        st.cancelled = true;
    }
    if let Some(child) = job.child.lock().unwrap().as_mut() {
        let _ = child.kill();
    }
    StatusCode::NO_CONTENT.into_response()
}

// ============================================================
// 日志
// ============================================================

#[derive(Deserialize)]
struct LogsQuery {
    limit: Option<usize>,
    src: Option<String>,
    result: Option<String>,
    since: Option<String>,
}

async fn logs(State(state): State<AppState>, Query(q): Query<LogsQuery>) -> Response {
    let limit = q.limit.unwrap_or(200).min(10_000);
    let content = std::fs::read_to_string(&state.log_path).unwrap_or_default();
    let mut entries: Vec<Value> = content
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|v| match &q.src {
            Some(src) => v.get("src").and_then(|x| x.as_str()) == Some(src.as_str()),
            None => true,
        })
        .filter(|v| match &q.result {
            Some(result) => v.get("result").and_then(|x| x.as_str()) == Some(result.as_str()),
            None => true,
        })
        .filter(|v| match &q.since {
            Some(since) => v
                .get("ts")
                .and_then(|x| x.as_str())
                .map(|t| t >= since.as_str())
                .unwrap_or(false),
            None => true,
        })
        .collect();
    if entries.len() > limit {
        entries.drain(0..entries.len() - limit);
    }
    Json(json!({ "entries": entries })).into_response()
}

// ============================================================
// 静态资源
// ============================================================

fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn serve_asset(path: &str) -> Option<Response> {
    let file = Assets::get(path)?;
    let value = HeaderValue::from_static(mime_for(path));
    Some(([(header::CONTENT_TYPE, value)], file.data.into_owned()).into_response())
}

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") || path == "api" {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response();
    }
    let asset = if path.is_empty() { "index.html" } else { path };
    if let Some(resp) = serve_asset(asset) {
        return resp;
    }
    // SPA 回退
    if let Some(resp) = serve_asset("index.html") {
        return resp;
    }
    (StatusCode::NOT_FOUND, "not found").into_response()
}

// ============================================================
// Router
// ============================================================

/// 构造可测试的 Router。
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/session", post(login).delete(logout))
        .route("/api/auth/status", get(auth_status))
        .route("/api/login-links", post(login_links))
        .route("/api/2fa/enroll", post(twofa_enroll))
        .route("/api/2fa/activate", post(twofa_activate))
        .route("/api/2fa/disable", post(twofa_disable))
        .route("/api/2fa/backup-codes", post(twofa_backup_codes))
        .route("/api/2fa/status", get(twofa_status))
        .route("/api/meta", get(meta))
        .route("/api/vaults", get(vaults))
        .route("/api/vaults/:name/mount", post(mount_vault))
        .route("/api/vaults/:name/umount", post(umount_vault))
        .route("/api/jobs", get(list_jobs))
        .route("/api/jobs/:id/events", get(job_events))
        .route("/api/jobs/:id/cancel", post(cancel_job))
        .route("/api/logs", get(logs))
        .fallback(static_handler)
        .layer(middleware::from_fn_with_state(state.clone(), auth_mw))
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}

// ============================================================
// CLI
// ============================================================

fn usage() {
    println!("gocryptfs-web {}", core::version_string());
    println!("Web UI for gocryptfs ({})", core::PROJECT_NAME);
    println!();
    println!("USAGE:");
    println!("    gocryptfs-web [OPTIONS]");
    println!("    gocryptfs-web token list");
    println!("    gocryptfs-web token add --label NAME [--expires 30d]");
    println!("    gocryptfs-web token revoke <label|id>");
    println!("    gocryptfs-web user add NAME");
    println!("    gocryptfs-web user passwd NAME");
    println!("    gocryptfs-web user delete NAME");
    println!("    gocryptfs-web user list");
    println!();
    println!("OPTIONS:");
    println!("    --listen ADDR       listen address (default {DEFAULT_LISTEN})");
    println!("    --public-url URL   public base URL used in QR/one-time links");
    println!("                       (e.g. https://example.com, behind a reverse proxy)");
    println!("    --trust-proxy      trust X-Forwarded-Proto/Host/For from a reverse proxy");
    println!(
        "    --token-file PATH   token file (default <config_dir>/{}/web.token)",
        core::APP_NAME
    );
    println!("    -c, --config PATH   config file (default: backend / XDG config)");
    println!("    -D, --data-dir DIR  data directory");
    println!("    -h, --help          print this help");
    println!("    -V, --version       print version");
    println!();
    println!("TOKEN SUBCOMMANDS:");
    println!("    list                list token ids/labels (never prints hashes)");
    println!("    add --label NAME    create a token and print it exactly once");
    println!("        [--expires 30d] optional expiry, e.g. 30d / 12h / 60m");
    println!("    revoke <label|id>   revoke by label or id");
    println!();
    println!("USER SUBCOMMANDS:");
    println!("    add NAME            add a user (password read from stdin)");
    println!("    passwd NAME         change password (password read from stdin)");
    println!("    delete NAME         delete a user");
    println!("    list                list users (never prints hashes/secrets)");
}

fn default_token_file() -> PathBuf {
    // 与配置目录同级（遵循 XDG_CONFIG_HOME）；不跟随 -c 的自定义文件名
    if let Some(dir) = dirs::config_dir() {
        return dir.join(core::APP_NAME).join("web.token");
    }
    PathBuf::from("/tmp").join(core::APP_NAME).join("web.token")
}

fn auth_file_for(token_path: &Path) -> PathBuf {
    match token_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join("web.auth"),
        _ => PathBuf::from("web.auth"),
    }
}

// ============================================================
// CLI 参数解析
// ============================================================

#[derive(Debug)]
enum TokenCommand {
    List,
    Add {
        label: String,
        expires: Option<String>,
    },
    Revoke {
        target: String,
    },
}

#[derive(Debug)]
enum UserCommand {
    Add(String),
    Passwd(String),
    Delete(String),
    List,
}

#[derive(Debug)]
enum Command {
    Serve,
    Token(TokenCommand),
    User(UserCommand),
}

#[derive(Debug)]
struct CliOptions {
    listen: String,
    public_url: Option<String>,
    trust_proxy: bool,
    token_file: Option<PathBuf>,
    config_arg: Option<PathBuf>,
    data_arg: Option<PathBuf>,
    command: Command,
}

enum Parsed {
    Help,
    Version,
    Run(CliOptions),
}

/// 规范化 `--public-url`：只允许 http(s) URL，去掉结尾 `/`。
fn normalize_public_url(value: &str) -> Result<String, String> {
    let v = value.trim().trim_end_matches('/');
    let rest = v.split_once("://").map(|(_, r)| r).unwrap_or("");
    let scheme_ok = v.starts_with("http://") || v.starts_with("https://");
    if !scheme_ok || rest.is_empty() || rest.contains('/') {
        return Err(format!(
            "--public-url must be a base http(s) URL like https://example.com, got {value:?}"
        ));
    }
    Ok(v.to_string())
}

fn next_value(args: &[String], i: &mut usize, name: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("{name} requires a value"))
}

fn parse_token_command(
    positionals: &[String],
    label: Option<String>,
    expires: Option<String>,
) -> Result<TokenCommand, String> {
    let sub = positionals
        .first()
        .ok_or_else(|| "token requires a subcommand: list|add|revoke".to_string())?;
    match sub.as_str() {
        "list" => {
            if positionals.len() != 1 {
                return Err("token list takes no arguments".to_string());
            }
            if label.is_some() || expires.is_some() {
                return Err("token list does not accept --label/--expires".to_string());
            }
            Ok(TokenCommand::List)
        }
        "add" => {
            if positionals.len() != 1 {
                return Err("token add takes no positional arguments".to_string());
            }
            let label = label.ok_or_else(|| "token add requires --label NAME".to_string())?;
            if label.trim().is_empty() {
                return Err("--label must not be empty".to_string());
            }
            Ok(TokenCommand::Add { label, expires })
        }
        "revoke" => {
            if expires.is_some() {
                return Err("token revoke does not accept --expires".to_string());
            }
            let target = match positionals.get(1) {
                Some(target) if positionals.len() == 2 => target.clone(),
                Some(_) => return Err("token revoke takes exactly one id/label".to_string()),
                None => label.ok_or_else(|| {
                    "token revoke requires <label|id> (or --label NAME)".to_string()
                })?,
            };
            Ok(TokenCommand::Revoke { target })
        }
        other => Err(format!("unknown token subcommand: {other}")),
    }
}

fn parse_user_command(
    positionals: &[String],
    label: Option<String>,
    expires: Option<String>,
) -> Result<UserCommand, String> {
    if label.is_some() || expires.is_some() {
        return Err("user commands do not accept --label/--expires".to_string());
    }
    let sub = positionals
        .first()
        .ok_or_else(|| "user requires a subcommand: add|passwd|delete|list".to_string())?;
    match sub.as_str() {
        "list" => {
            if positionals.len() != 1 {
                return Err("user list takes no arguments".to_string());
            }
            Ok(UserCommand::List)
        }
        "add" | "passwd" | "delete" => {
            if positionals.len() != 2 {
                return Err(format!("user {sub} requires exactly one NAME"));
            }
            let name = positionals[1].clone();
            if name.trim().is_empty() {
                return Err("user name must not be empty".to_string());
            }
            Ok(match sub.as_str() {
                "add" => UserCommand::Add(name),
                "passwd" => UserCommand::Passwd(name),
                _ => UserCommand::Delete(name),
            })
        }
        other => Err(format!("unknown user subcommand: {other}")),
    }
}

fn parse_args(args: &[String]) -> Result<Parsed, String> {
    let mut listen = DEFAULT_LISTEN.to_string();
    let mut public_url: Option<String> = None;
    let mut trust_proxy = false;
    let mut token_file: Option<PathBuf> = None;
    let mut config_arg: Option<PathBuf> = None;
    let mut data_arg: Option<PathBuf> = None;
    let mut label: Option<String> = None;
    let mut expires: Option<String> = None;
    let mut mode: Option<&str> = None;
    let mut positionals: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "-h" || arg == "--help" {
            return Ok(Parsed::Help);
        }
        if arg == "-V" || arg == "--version" {
            return Ok(Parsed::Version);
        }
        if let Some(value) = arg.strip_prefix("--listen=") {
            listen = value.to_string();
        } else if let Some(value) = arg.strip_prefix("--public-url=") {
            public_url = Some(normalize_public_url(value)?);
        } else if arg == "--trust-proxy" {
            trust_proxy = true;
        } else if let Some(value) = arg.strip_prefix("--token-file=") {
            token_file = Some(PathBuf::from(value));
        } else if let Some(value) = arg.strip_prefix("--config=") {
            config_arg = Some(PathBuf::from(value));
        } else if let Some(value) = arg.strip_prefix("--data-dir=") {
            data_arg = Some(PathBuf::from(value));
        } else if let Some(value) = arg.strip_prefix("--label=") {
            label = Some(value.to_string());
        } else if let Some(value) = arg.strip_prefix("--expires=") {
            expires = Some(value.to_string());
        } else {
            match arg {
                "--listen" => listen = next_value(args, &mut i, "--listen")?,
                "--public-url" => {
                    public_url = Some(normalize_public_url(&next_value(
                        args,
                        &mut i,
                        "--public-url",
                    )?)?)
                }
                "--trust-proxy" => trust_proxy = true,
                "--token-file" => {
                    token_file = Some(PathBuf::from(next_value(args, &mut i, "--token-file")?))
                }
                "-c" | "--config" => {
                    config_arg = Some(PathBuf::from(next_value(args, &mut i, "-c")?))
                }
                "-D" | "--data-dir" => {
                    data_arg = Some(PathBuf::from(next_value(args, &mut i, "-D")?))
                }
                "--label" => label = Some(next_value(args, &mut i, "--label")?),
                "--expires" => expires = Some(next_value(args, &mut i, "--expires")?),
                "token" | "user" if mode.is_none() => {
                    mode = Some(if arg == "token" { "token" } else { "user" });
                }
                other if other.starts_with('-') => {
                    return Err(format!("unknown option: {other}"));
                }
                other => {
                    if mode.is_none() {
                        return Err(format!("unexpected argument: {other}"));
                    }
                    positionals.push(other.to_string());
                }
            }
        }
        i += 1;
    }

    let command = match mode {
        None => {
            if label.is_some() || expires.is_some() {
                return Err("--label/--expires are only valid for token add".to_string());
            }
            Command::Serve
        }
        Some("token") => Command::Token(parse_token_command(&positionals, label, expires)?),
        Some("user") => Command::User(parse_user_command(&positionals, label, expires)?),
        Some(_) => unreachable!("mode is token/user"),
    };

    Ok(Parsed::Run(CliOptions {
        listen,
        public_url,
        trust_proxy,
        token_file,
        config_arg,
        data_arg,
        command,
    }))
}

// ============================================================
// CLI 子命令实现
// ============================================================

fn parse_expiry(spec: &str) -> Result<String, String> {
    let digits = spec
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(spec.len());
    if digits == 0 {
        return Err(format!("invalid --expires value: {spec:?}"));
    }
    let (num, unit) = spec.split_at(digits);
    let n: i64 = num
        .parse()
        .map_err(|_| format!("invalid --expires value: {spec:?}"))?;
    if n <= 0 {
        return Err("--expires must be positive".to_string());
    }
    let duration = match unit {
        "d" => chrono::Duration::days(n),
        "h" => chrono::Duration::hours(n),
        "m" => chrono::Duration::minutes(n),
        "s" | "" => chrono::Duration::seconds(n),
        _ => return Err(format!("invalid --expires unit {unit:?} (use d/h/m/s)")),
    };
    let when = chrono::Utc::now() + duration;
    Ok(when.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn read_stdin_password() -> Result<String, String> {
    if std::io::stdin().is_terminal() {
        eprint!("Password: ");
        let _ = std::io::stderr().flush();
    }
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| format!("failed to read password from stdin: {e}"))?;
    let password = line.trim_end_matches(&['\r', '\n'][..]).to_string();
    if password.is_empty() {
        return Err("password must not be empty".to_string());
    }
    Ok(password)
}

fn run_token_command(path: &Path, command: TokenCommand) -> Result<(), String> {
    match command {
        TokenCommand::List => {
            let store = auth::load_token_store_for_update(path)?;
            if store.tokens.is_empty() {
                println!("no tokens");
                return Ok(());
            }
            println!(
                "{:<8} {:<24} {:<26} EXPIRES_AT",
                "ID", "LABEL", "CREATED_AT"
            );
            for token in &store.tokens {
                println!(
                    "{:<8} {:<24} {:<26} {}",
                    token.id,
                    token.label,
                    token.created_at,
                    token.expires_at.as_deref().unwrap_or("never")
                );
            }
            Ok(())
        }
        TokenCommand::Add { label, expires } => {
            let mut store = auth::load_token_store_for_update(path)?;
            let expires_at = match expires {
                Some(spec) => Some(parse_expiry(&spec)?),
                None => None,
            };
            let (entry, token) = store.add_token(&label, expires_at)?;
            auth::save_token_store(path, &store)?;
            // 明文 token 只输出这一次。
            println!("{token}");
            eprintln!(
                "token added: id={} label={} expires_at={}",
                entry.id,
                entry.label,
                entry.expires_at.as_deref().unwrap_or("never")
            );
            Ok(())
        }
        TokenCommand::Revoke { target } => {
            let mut store = auth::load_token_store_for_update(path)?;
            let removed = store.revoke(&target)?;
            auth::save_token_store(path, &store)?;
            println!("revoked {removed} token(s)");
            Ok(())
        }
    }
}

fn run_user_command(token_path: &Path, command: UserCommand) -> Result<(), String> {
    let auth_path = auth_file_for(token_path);
    let mut store = auth::load_auth_store(&auth_path)?;
    match command {
        UserCommand::Add(name) => {
            let password = read_stdin_password()?;
            store.add_user(&name, &password)?;
            auth::save_auth_store(&auth_path, &store)?;
            println!("user added: {name}");
        }
        UserCommand::Passwd(name) => {
            let password = read_stdin_password()?;
            store.set_password(&name, &password)?;
            auth::save_auth_store(&auth_path, &store)?;
            println!("password updated: {name}");
        }
        UserCommand::Delete(name) => {
            if !store.remove_user(&name) {
                return Err(format!("user not found: {name}"));
            }
            auth::save_auth_store(&auth_path, &store)?;
            println!("user deleted: {name}");
        }
        UserCommand::List => {
            if store.users.is_empty() {
                println!("no users");
                return Ok(());
            }
            println!("{:<24} {:<8} CREATED_AT", "NAME", "2FA");
            for user in &store.users {
                println!(
                    "{:<24} {:<8} {}",
                    user.name,
                    if user.totp_enabled { "on" } else { "off" },
                    user.created_at
                );
            }
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

fn serve(
    listen: String,
    public_url: Option<String>,
    trust_proxy: bool,
    token_path: PathBuf,
    config_arg: Option<PathBuf>,
    data_arg: Option<PathBuf>,
) -> Result<(), String> {
    let addr: SocketAddr = match listen.parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: invalid --listen address {listen:?}: {e}");
            std::process::exit(2);
        }
    };

    let config = core::paths::resolve_config(config_arg);
    let data_overridden = data_arg.is_some();
    let data_dir = core::paths::resolve_data_dir(data_arg);
    let state_dir = core::paths::resolve_state_dir(&data_dir, data_overridden);
    let auth_path = auth_file_for(&token_path);

    let (token_store, new_token) = auth::load_token_store(&token_path)?;
    let users = match auth::load_auth_store(&auth_path) {
        Ok(store) => store,
        Err(e) => {
            eprintln!("warning: cannot load auth store: {e} (password auth disabled)");
            auth::AuthStore::default()
        }
    };

    let mut state =
        AppState::with_token_store(token_store, config, data_dir, data_overridden, state_dir);
    state.auth_path = auth_path;
    state.token_path = token_path.clone();
    state.users = Arc::new(Mutex::new(users));
    *state.auth_mtime.lock().unwrap() = file_mtime(&state.auth_path);
    *state.token_mtime.lock().unwrap() = file_mtime(&state.token_path);
    state.listen_url = format!("http://{addr}");
    state.public_url = public_url;
    state.trust_proxy = trust_proxy;

    println!("gocryptfs-web {}", core::version_string());
    println!("listen:     {}", state.listen_url);
    if let Some(public_url) = &state.public_url {
        println!("public url: {public_url}");
    }
    println!(
        "trust proxy: {}",
        if state.trust_proxy {
            "yes (X-Forwarded-* honored)"
        } else {
            "no"
        }
    );
    if let Some(token) = new_token {
        println!("token:      {token}");
    }
    let token_count = state.tokens.lock().unwrap().active_count();
    println!("tokens:     {token_count} active");
    println!("token file: {}", token_path.display());
    println!("auth file:  {}", state.auth_path.display());
    println!("config:     {}", state.config);
    println!("data dir:   {}", state.data_dir.display());
    println!("state dir:  {}", state.state_dir.display());
    println!("log:        {}", state.log_path.display());
    if token_count == 0 {
        eprintln!();
        eprintln!("!!! WARNING: no active tokens. Create one with:");
        eprintln!("!!!   gocryptfs-web token add --label default");
        eprintln!();
    }
    if !addr.ip().is_loopback() {
        eprintln!();
        eprintln!("!!! WARNING: listening on a NON-LOOPBACK address {addr}");
        eprintln!("!!! The token grants full control over all vaults; anyone who can");
        eprintln!("!!! reach this port and read the token can mount/unmount volumes.");
        eprintln!("!!! Prefer 127.0.0.1 and a reverse proxy with TLS + auth.");
        eprintln!();
    }

    // 一次性手机登录链接（QR）。
    let nonce = state.issue_login_nonce();
    let link = state.login_url(&nonce);
    println!();
    println!(
        "one-time login link (valid {}s):",
        auth::LOGIN_NONCE_TTL.as_secs()
    );
    println!("{link}");
    match auth::qr_ascii(&link) {
        Ok(qr) => println!("{qr}"),
        Err(e) => eprintln!("warning: cannot render login QR: {e}"),
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => return Err(format!("cannot start tokio runtime: {e}")),
    };

    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("cannot bind {addr}: {e}"))?;
        axum::serve(
            listener,
            app(state).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| format!("server error: {e}"))
    })
}

/// 入口：解析参数；子命令直接执行，否则启动 HTTP 服务。
pub fn run() {
    // 与 TUI 一致：优先使用内嵌后端（避免 PATH 里的旧版 gocryptfs-cli 影响路径/文案）
    let data_dir = core::paths::resolve_data_dir(None);
    if let Err(e) = core::backend::install_embedded(&data_dir, &core::version_line()) {
        eprintln!("[!] failed to install embedded backend: {e}");
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!();
            usage();
            std::process::exit(2);
        }
    };
    let opts = match parsed {
        Parsed::Help => {
            usage();
            return;
        }
        Parsed::Version => {
            println!("gocryptfs-web {}", core::version_string());
            return;
        }
        Parsed::Run(opts) => opts,
    };

    let CliOptions {
        listen,
        public_url,
        trust_proxy,
        token_file,
        config_arg,
        data_arg,
        command,
    } = opts;
    let token_path = token_file.unwrap_or_else(default_token_file);

    let result = match command {
        Command::Token(cmd) => run_token_command(&token_path, cmd),
        Command::User(cmd) => run_user_command(&token_path, cmd),
        Command::Serve => serve(
            listen,
            public_url,
            trust_proxy,
            token_path,
            config_arg,
            data_arg,
        ),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod lang_tests {
    use super::normalize_lang;

    #[test]
    fn normalize_lang_only_passes_supported_codes() {
        assert_eq!(normalize_lang(Some("zh-CN")), Some("zh-CN"));
        assert_eq!(normalize_lang(Some("en-US")), Some("en-US"));
        assert_eq!(normalize_lang(Some("zh")), None);
        assert_eq!(normalize_lang(Some("fr-FR")), None);
        assert_eq!(normalize_lang(None), None);
    }
}

#[cfg(test)]
mod proxy_tests {
    use super::*;
    use axum::http::header::{HeaderName, HeaderValue};

    fn state() -> AppState {
        AppState::new(
            "tok".to_string(),
            "/tmp/config.yaml".to_string(),
            PathBuf::from("/tmp/data"),
            true,
            PathBuf::from("/tmp/state"),
        )
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn public_url_wins_for_links_and_cookies() {
        let mut st = state();
        st.public_url = Some("https://example.com".to_string());
        let h = headers(&[("host", "127.0.0.1:30032")]);
        assert_eq!(request_base_url(&st, &h), "https://example.com");
        assert!(request_is_secure(&st, &h));
        assert!(session_cookie(&st, &h, "s", 10).contains("Secure"));
    }

    #[test]
    fn forwarded_proto_only_honored_when_trusted() {
        let h = headers(&[("host", "nas.lan"), ("x-forwarded-proto", "https")]);
        let plain = state();
        assert_eq!(request_base_url(&plain, &h), "http://nas.lan");
        assert!(!request_is_secure(&plain, &h));
        assert!(!session_cookie(&plain, &h, "s", 10).contains("Secure"));

        let mut trusted = state();
        trusted.trust_proxy = true;
        assert_eq!(request_base_url(&trusted, &h), "https://nas.lan");
        assert!(request_is_secure(&trusted, &h));
        assert!(session_cookie(&trusted, &h, "s", 10).contains("Secure"));
    }

    #[test]
    fn x_forwarded_for_only_honored_when_trusted() {
        let h = headers(&[("x-forwarded-for", "203.0.113.9, 10.0.0.1")]);
        assert_eq!(client_ip(&state(), &h, None), "unknown");
        let mut trusted = state();
        trusted.trust_proxy = true;
        assert_eq!(client_ip(&trusted, &h, None), "203.0.113.9");
    }

    #[test]
    fn origin_allowed_via_forwarded_host_or_public_url() {
        let h = headers(&[
            ("host", "127.0.0.1:30032"),
            ("x-forwarded-host", "example.com"),
        ]);
        assert!(!origin_allowed(&state(), "https://example.com", &h));
        let mut trusted = state();
        trusted.trust_proxy = true;
        assert!(origin_allowed(&trusted, "https://example.com", &h));

        let mut public = state();
        public.public_url = Some("https://example.com".to_string());
        let local = headers(&[("host", "127.0.0.1:30032")]);
        assert!(origin_allowed(&public, "https://example.com", &local));
        assert!(!origin_allowed(&public, "https://evil.example", &local));
    }

    #[test]
    fn public_url_normalization() {
        assert_eq!(
            normalize_public_url("https://example.com/").unwrap(),
            "https://example.com"
        );
        assert!(normalize_public_url("example.com").is_err());
        assert!(normalize_public_url("https://example.com/app").is_err());
        assert!(normalize_public_url("ftp://example.com").is_err());
        assert!(normalize_public_url("https://").is_err());
    }
}
