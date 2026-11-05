//! `gocryptfs-ui-web` — gocryptfs-tui 的 Web UI 后端。
//!
//! 由 `gocryptfs-web` 二进制调用。提供：
//!   * 基于 token + cookie 的会话认证（12h TTL）；
//!   * 卷 mount / umount 的后台任务与 SSE 事件流；
//!   * 元信息、任务列表、JSONL 日志查询；
//!   * 内嵌前端静态资源（SPA 回退）。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write as _};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

#[cfg(test)]
mod tests;

const SESSION_TTL: Duration = Duration::from_secs(12 * 60 * 60);
const SESSION_COOKIE: &str = "gfs_session";
const CSRF_HEADER: &str = "x-gfs-csrf";
const DEFAULT_LISTEN: &str = "127.0.0.1:8787";

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
    pub token: String,
    pub sessions: Arc<Mutex<HashMap<String, Instant>>>,
    pub config: String,
    pub data_dir: PathBuf,
    pub data_overridden: bool,
    pub state_dir: PathBuf,
    pub log_path: PathBuf,
    pub jobs: Arc<Mutex<HashMap<u64, Arc<Job>>>>,
    pub next_job_id: Arc<AtomicU64>,
}

impl AppState {
    pub fn new(
        token: String,
        config: String,
        data_dir: PathBuf,
        data_overridden: bool,
        state_dir: PathBuf,
    ) -> Self {
        let log_path = state_dir.join("app.log.jsonl");
        Self {
            token,
            sessions: Arc::new(Mutex::new(HashMap::new())),
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
                false
            }
            None => false,
        }
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

/// 常数时间比较（长度不同直接返回 false）。
fn token_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
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

async fn auth_mw(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    if !path.starts_with("/api") || path == "/api/session" {
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
            let host = req
                .headers()
                .get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_ascii_lowercase();
            if origin_host(origin).as_deref() != Some(host.as_str()) {
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
// 会话
// ============================================================

#[derive(Deserialize)]
struct LoginRequest {
    token: String,
}

async fn login(State(state): State<AppState>, Json(body): Json<LoginRequest>) -> Response {
    if !token_eq(&body.token, &state.token) {
        return unauthorized();
    }
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let session = URL_SAFE_NO_PAD.encode(raw);
    state
        .sessions
        .lock()
        .unwrap()
        .insert(session.clone(), Instant::now());
    let cookie = format!("{SESSION_COOKIE}={session}; HttpOnly; SameSite=Strict; Path=/");
    (StatusCode::NO_CONTENT, [(header::SET_COOKIE, cookie)]).into_response()
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(session) = cookie_value(&headers, SESSION_COOKIE) {
        state.sessions.lock().unwrap().remove(&session);
    }
    let cookie = format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0");
    (StatusCode::NO_CONTENT, [(header::SET_COOKIE, cookie)]).into_response()
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
}

#[derive(Deserialize)]
struct UmountRequest {
    #[serde(default)]
    force: bool,
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
    match spawn_job(&state, "mount", &name, Some(body.passphrase), false) {
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
    match spawn_job(&state, "umount", &name, None, body.force) {
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
) -> Result<u64, String> {
    let id = state.next_job_id.fetch_add(1, Ordering::SeqCst);
    let (tx, _rx) = broadcast::channel(256);

    let mut cmd = core::backend::cli_command(&core::paths::cli_path());
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
    println!();
    println!("OPTIONS:");
    println!("    --listen ADDR       listen address (default {DEFAULT_LISTEN})");
    println!("    --token-file PATH   token file (default <config_dir>/gocryptfs-tui/web.token)");
    println!("    -c, --config PATH   config file (default: backend / XDG config)");
    println!("    -D, --data-dir DIR  data directory");
    println!("    -h, --help          print this help");
    println!("    -V, --version       print version");
}

fn default_token_file() -> PathBuf {
    let config = core::paths::resolve_config(None);
    let dir = Path::new(&config)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    dir.join(core::APP_NAME).join("web.token")
}

fn load_or_create_token(path: &Path) -> Result<String, String> {
    if let Ok(content) = std::fs::read_to_string(path) {
        let token = content.trim().to_string();
        if !token.is_empty() {
            return Ok(token);
        }
    }
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let token = URL_SAFE_NO_PAD.encode(raw);
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
    file.write_all(token.as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(b"\n")
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(token)
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

/// 入口：解析参数，加载/生成 token，启动 HTTP 服务。
pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut listen = DEFAULT_LISTEN.to_string();
    let mut token_file: Option<PathBuf> = None;
    let mut config_arg: Option<PathBuf> = None;
    let mut data_arg: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].clone();
        let value = |i: &mut usize, name: &str| -> String {
            *i += 1;
            match args.get(*i) {
                Some(v) => v.clone(),
                None => {
                    eprintln!("error: {name} requires a value");
                    std::process::exit(2);
                }
            }
        };
        match arg.as_str() {
            "-h" | "--help" => {
                usage();
                return;
            }
            "-V" | "--version" => {
                println!("gocryptfs-web {}", core::version_string());
                return;
            }
            "--listen" => listen = value(&mut i, "--listen"),
            "--token-file" => token_file = Some(PathBuf::from(value(&mut i, "--token-file"))),
            "-c" | "--config" => config_arg = Some(PathBuf::from(value(&mut i, "-c"))),
            "-D" | "--data-dir" => data_arg = Some(PathBuf::from(value(&mut i, "-D"))),
            other => {
                eprintln!("error: unknown argument: {other}");
                usage();
                std::process::exit(2);
            }
        }
        i += 1;
    }

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
    let token_path = token_file.unwrap_or_else(default_token_file);

    let token = match load_or_create_token(&token_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: cannot load/create token: {e}");
            std::process::exit(1);
        }
    };

    let log_path = state_dir.join("app.log.jsonl");
    println!("gocryptfs-web {}", core::version_string());
    println!("listen:     http://{addr}");
    println!("token:      {token}");
    println!("token file: {}", token_path.display());
    println!("config:     {config}");
    println!("data dir:   {}", data_dir.display());
    println!("state dir:  {}", state_dir.display());
    println!("log:        {}", log_path.display());
    if !addr.ip().is_loopback() {
        eprintln!();
        eprintln!("!!! WARNING: listening on a NON-LOOPBACK address {addr}");
        eprintln!("!!! The token grants full control over all vaults; anyone who can");
        eprintln!("!!! reach this port and read the token can mount/unmount volumes.");
        eprintln!("!!! Prefer 127.0.0.1 and a reverse proxy with TLS + auth.");
        eprintln!();
    }

    let state = AppState::new(token, config, data_dir, data_overridden, state_dir);
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: cannot start tokio runtime: {e}");
            std::process::exit(1);
        }
    };

    let result = runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("cannot bind {addr}: {e}"))?;
        axum::serve(listener, app(state))
            .with_graceful_shutdown(shutdown_signal())
            .await
            .map_err(|e| format!("server error: {e}"))
    });

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
