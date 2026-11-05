//! Web 后端测试：认证、元信息、日志、静态资源与任务（SSE）路径。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::auth;
use crate::{app, AppState};

const TOKEN: &str = "test-token-0123456789";

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "gocryptfs-web-test-{}-{}-{}",
        std::process::id(),
        tag,
        n
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn test_state(dir: &Path) -> AppState {
    AppState::new(
        TOKEN.to_string(),
        dir.join("config.yaml").to_string_lossy().to_string(),
        dir.to_path_buf(),
        true,
        dir.to_path_buf(),
    )
}

fn state_with_token_store(dir: &Path, store: auth::TokenStore) -> AppState {
    AppState::with_token_store(
        store,
        dir.join("config.yaml").to_string_lossy().to_string(),
        dir.to_path_buf(),
        true,
        dir.to_path_buf(),
    )
}

fn router(dir: &Path) -> Router {
    app(test_state(dir))
}

async fn send(app: &Router, req: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let res = app.clone().oneshot(req).await.expect("request failed");
    let status = res.status();
    let headers = res.headers().clone();
    let body = res
        .into_body()
        .collect()
        .await
        .expect("body collect failed")
        .to_bytes()
        .to_vec();
    (status, headers, body)
}

fn get(uri: &str, cookie: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some(c) = cookie {
        builder = builder.header(header::COOKIE, c);
    }
    builder.body(Body::empty()).unwrap()
}

fn post_json(uri: &str, body: &str, cookie: Option<&str>, csrf: bool) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(c) = cookie {
        builder = builder.header(header::COOKIE, c);
    }
    if csrf {
        builder = builder.header("x-gfs-csrf", "1");
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

fn set_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(|v| v.to_string())
}

async fn login_json(app: &Router, body: Value) -> (StatusCode, Option<String>) {
    let req = post_json("/api/session", &body.to_string(), None, false);
    let (status, headers, _) = send(app, req).await;
    (status, set_cookie(&headers))
}

async fn login(app: &Router, token: &str) -> (StatusCode, Option<String>) {
    login_json(app, json!({ "token": token })).await
}

fn fake_cli() -> PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let dir = temp_dir("cli");
        let path = dir.join("fake-gocryptfs-cli");
        let script = r#"#!/bin/sh
if [ "$1" = "-c" ]; then
  echo '{"status":"ok","data":{"vaults":[{"id":1,"name":"demo","path":"/tmp/demo","mount_point":"/tmp/demo-mnt","mounted":false,"locked":true}]}}'
  exit 0
fi
if [ "$1" = "mount" ]; then
  echo "@@PROGRESS@@ 50 1 2" >&2
  echo "@@CHECK@@ fs ok" >&2
  echo "mounting demo" >&2
  echo "@@DONE@@ mount" >&2
  exit 0
fi
if [ "$1" = "umount" ]; then
  echo "@@DONE@@ umount" >&2
  exit 0
fi
exit 0
"#;
        std::fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    })
    .clone()
}

fn use_fake_cli() {
    std::env::set_var("GOCRYPTFS_CLI", fake_cli());
}

#[tokio::test]
async fn healthz_is_public() {
    let dir = temp_dir("healthz");
    let app = router(&dir);
    let (status, headers, body) = send(&app, get("/healthz", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, b"ok");
    // 安全响应头出现在所有响应上
    assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
    assert_eq!(headers.get("referrer-policy").unwrap(), "no-referrer");
    assert!(headers
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("frame-ancestors 'none'"));
}

#[tokio::test]
async fn api_requires_cookie() {
    let dir = temp_dir("auth");
    let app = router(&dir);
    let (status, _, _) = send(&app, get("/api/vaults", None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = send(&app, get("/api/meta", Some("gfs_session=bogus"))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_rejects_wrong_token() {
    let dir = temp_dir("login-bad");
    let app = router(&dir);
    let (status, cookie) = login(&app, "wrong").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(cookie.is_none());
}

#[tokio::test]
async fn login_then_meta() {
    let dir = temp_dir("login-ok");
    let app = router(&dir);
    let (status, cookie) = login(&app, TOKEN).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let cookie = cookie.expect("set-cookie header");
    assert!(cookie.starts_with("gfs_session="));

    let (status, _, body) = send(&app, get("/api/meta", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["project"], "gocryptfs-ui");
    assert!(!json["version"].as_str().unwrap().is_empty());
    assert!(json["paths"]["log"]
        .as_str()
        .unwrap()
        .ends_with("app.log.jsonl"));
    assert!(json["deps"].get("missing").is_some());
}

#[tokio::test]
async fn logout_invalidates_session() {
    let dir = temp_dir("logout");
    let app = router(&dir);
    let (_, cookie) = login(&app, TOKEN).await;
    let cookie = cookie.unwrap();
    let req = Request::builder()
        .method("DELETE")
        .uri("/api/session")
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    let (status, _, _) = send(&app, req).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = send(&app, get("/api/meta", Some(&cookie))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logs_are_filtered_and_limited() {
    let dir = temp_dir("logs");
    let state = test_state(&dir);
    let lines = [
        r#"{"ts":"2026-01-01T00:00:00Z","src":"tui","result":"success","action":"mount","target":"a"}"#,
        r#"{"ts":"2026-01-02T00:00:00Z","src":"cli","result":"failed","action":"mount","target":"b"}"#,
        r#"{"ts":"2026-01-03T00:00:00Z","src":"tui","result":"success","action":"umount","target":"a"}"#,
    ];
    std::fs::write(&state.log_path, lines.join("\n") + "\n").unwrap();
    let app = app(state);
    let (_, cookie) = login(&app, TOKEN).await;
    let cookie = cookie.unwrap();

    let (status, _, body) = send(&app, get("/api/logs?limit=2", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["entries"].as_array().unwrap().len(), 2);
    // 取最后 2 条
    assert_eq!(json["entries"][1]["target"], "a");

    let (_, _, body) = send(&app, get("/api/logs?src=tui", Some(&cookie))).await;
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["entries"].as_array().unwrap().len(), 2);

    let (_, _, body) = send(&app, get("/api/logs?src=cli&result=failed", Some(&cookie))).await;
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["entries"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn vaults_use_cli_and_require_csrf_for_writes() {
    use_fake_cli();
    let dir = temp_dir("vaults");
    let app = router(&dir);
    let (_, cookie) = login(&app, TOKEN).await;
    let cookie = cookie.unwrap();

    let (status, _, body) = send(&app, get("/api/vaults", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["vaults"][0]["name"], "demo");

    // 写操作缺少 CSRF 头 → 403
    let (status, _, _) = send(
        &app,
        post_json(
            "/api/vaults/demo/mount",
            "{\"passphrase\":\"x\"}",
            Some(&cookie),
            false,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn mount_unknown_vault_is_404() {
    use_fake_cli();
    let dir = temp_dir("mount-unknown");
    let app = router(&dir);
    let (_, cookie) = login(&app, TOKEN).await;
    let cookie = cookie.unwrap();

    let (status, _, _) = send(
        &app,
        post_json(
            "/api/vaults/nope/mount",
            "{\"passphrase\":\"x\"}",
            Some(&cookie),
            true,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn mount_job_streams_sse_until_done() {
    use_fake_cli();
    let dir = temp_dir("sse");
    let app = router(&dir);
    let (_, cookie) = login(&app, TOKEN).await;
    let cookie = cookie.unwrap();

    let (status, _, body) = send(
        &app,
        post_json(
            "/api/vaults/demo/mount",
            "{\"passphrase\":\"secret\"}",
            Some(&cookie),
            true,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let id = json["id"].as_u64().expect("job id");

    let (_, _, jobs) = send(&app, get("/api/jobs", Some(&cookie))).await;
    let jobs: serde_json::Value = serde_json::from_slice(&jobs).unwrap();
    assert_eq!(jobs["jobs"][0]["kind"], "mount");

    let req = get(&format!("/api/jobs/{id}/events"), Some(&cookie));
    let (status, headers, body) = tokio::time::timeout(Duration::from_secs(10), send(&app, req))
        .await
        .expect("SSE stream should close after job ends");
    assert_eq!(status, StatusCode::OK);
    assert!(headers
        .get(header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("text/event-stream"));
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("event: done"), "SSE body: {text}");
}

#[tokio::test]
async fn static_fallback_serves_index() {
    let dir = temp_dir("static");
    let app = router(&dir);
    let (status, headers, body) = send(&app, get("/", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers
        .get(header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .contains("html"));
    assert!(!body.is_empty(), "index.html should be served");

    // 未知非 /api 路径 → SPA index
    let (status, _, _) = send(&app, get("/some/spa/route", None)).await;
    assert_eq!(status, StatusCode::OK);
}

// ============================================================
// 认证扩展：legacy token / 一次性 nonce / 多 token / 密码 / 2FA / 限流
// ============================================================

#[tokio::test]
async fn legacy_token_file_is_migrated_and_login_works() {
    let dir = temp_dir("legacy-token");
    let token_path = dir.join("web.token");
    std::fs::write(&token_path, format!("{TOKEN}\n")).unwrap();

    let (store, new_token) = auth::load_token_store(&token_path).unwrap();
    assert!(new_token.is_none(), "legacy token must not be re-created");
    assert_eq!(store.tokens.len(), 1);

    let raw = std::fs::read_to_string(&token_path).unwrap();
    let json: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(json["tokens"][0]["label"], "default");
    assert_eq!(json["tokens"][0]["hash"], auth::sha256_hex(TOKEN));
    assert!(
        !raw.contains(TOKEN),
        "migrated file must not contain the plaintext token: {raw}"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&token_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "token file must be 0600");
    }

    let app = app(state_with_token_store(&dir, store));
    let (status, cookie) = login(&app, TOKEN).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(cookie.is_some());
}

#[tokio::test]
async fn login_link_nonce_is_single_use() {
    let dir = temp_dir("nonce");
    let app = router(&dir);
    let (_, cookie) = login(&app, TOKEN).await;
    let cookie = cookie.unwrap();

    let (status, _, body) = send(
        &app,
        post_json("/api/login-links", "{}", Some(&cookie), true),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["expires_in"], 120);
    assert!(json["svg"].as_str().unwrap().contains("<svg"));
    let url = json["url"].as_str().unwrap();
    assert!(url.contains("/#t="), "url: {url}");
    let nonce = url.split("#t=").nth(1).unwrap().to_string();

    let (status, cookie) = login_json(&app, json!({ "nonce": nonce })).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(cookie.is_some());

    // 同一个 nonce 第二次使用必须失败。
    let (status, _) = login_json(&app, json!({ "nonce": nonce })).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn multi_token_store_add_list_revoke() {
    let dir = temp_dir("multi-token");
    let token_path = dir.join("web.token");
    let (mut store, default_token) = auth::load_token_store(&token_path).unwrap();
    let default_token = default_token.expect("fresh store token");

    let (phone, phone_token) = store.add_token("phone", None).unwrap();
    let (laptop, laptop_token) = store.add_token("laptop", None).unwrap();
    assert_eq!(store.tokens.len(), 3);
    assert_eq!(store.active_count(), 3);
    assert_ne!(phone.id, laptop.id);
    assert!(store.verify(&phone_token));
    assert!(store.verify(&laptop_token));
    assert!(store.verify(&default_token));

    auth::save_token_store(&token_path, &store).unwrap();
    let (loaded, _) = auth::load_token_store(&token_path).unwrap();
    assert_eq!(loaded.tokens.len(), 3);

    let state = state_with_token_store(&dir, loaded);
    let app = app(state.clone());
    assert_eq!(login(&app, &phone_token).await.0, StatusCode::NO_CONTENT);
    assert_eq!(login(&app, &default_token).await.0, StatusCode::NO_CONTENT);

    // 按 label 撤销 phone。
    {
        let mut store = state.tokens.lock().unwrap();
        assert_eq!(store.revoke("phone").unwrap(), 1);
        auth::save_token_store(&token_path, &store).unwrap();
    }
    assert_eq!(login(&app, &phone_token).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(login(&app, &default_token).await.0, StatusCode::NO_CONTENT);

    // 按 id 撤销 laptop。
    {
        let mut store = state.tokens.lock().unwrap();
        assert_eq!(store.revoke(&laptop.id).unwrap(), 1);
        auth::save_token_store(&token_path, &store).unwrap();
    }
    assert_eq!(login(&app, &laptop_token).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(login(&app, &default_token).await.0, StatusCode::NO_CONTENT);

    // 过期 token 不再可用。
    let mut store = state.tokens.lock().unwrap();
    store.tokens[0].expires_at = Some("2000-01-01T00:00:00Z".to_string());
    assert!(!store.verify(&default_token));
}

#[tokio::test]
async fn auth_status_and_password_login() {
    let dir = temp_dir("password");
    let state = test_state(&dir);
    state
        .users
        .lock()
        .unwrap()
        .add_user("alice", "s3cret")
        .unwrap();
    let app = app(state);

    let (status, _, body) = send(&app, get("/api/auth/status", None)).await;
    assert_eq!(status, StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["token_auth"], true);
    assert_eq!(json["password_auth"], true);
    assert_eq!(json["2fa_available"], true);

    let (status, _, body) = send(
        &app,
        post_json(
            "/api/session",
            &json!({ "username": "alice", "password": "wrong" }).to_string(),
            None,
            false,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "invalid credentials");

    let (status, cookie) =
        login_json(&app, json!({ "username": "alice", "password": "s3cret" })).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let cookie = cookie.expect("session cookie");
    let (status, _, _) = send(&app, get("/api/meta", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn totp_enroll_activate_login_and_backup_code() {
    let dir = temp_dir("totp");
    let state = test_state(&dir);
    state.users.lock().unwrap().add_user("alice", "pw").unwrap();
    let app = app(state);

    // token 登录（系统只有一个用户时 2FA 操作自动定位到该用户）。
    let (_, cookie) = login(&app, TOKEN).await;
    let cookie = cookie.unwrap();

    let (status, _, body) = send(
        &app,
        post_json("/api/2fa/enroll", "{}", Some(&cookie), true),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).unwrap();
    let secret = json["secret"].as_str().unwrap().to_string();
    assert!(!secret.is_empty());
    let otpauth = json["otpauth_url"].as_str().unwrap();
    assert!(otpauth.starts_with("otpauth://totp/"), "{otpauth}");
    assert!(json["svg"].as_str().unwrap().contains("<svg"));

    let code = auth::current_totp(&secret).unwrap();
    let (status, _, body) = send(
        &app,
        post_json(
            "/api/2fa/activate",
            &json!({ "code": code }).to_string(),
            Some(&cookie),
            true,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).unwrap();
    let backup_codes = json["backup_codes"].as_array().unwrap().clone();
    assert_eq!(backup_codes.len(), 8);
    for code in &backup_codes {
        assert_eq!(code.as_str().unwrap().len(), 10);
    }

    // 已启用 2FA 但未提供 totp → totp_required。
    let (status, _, body) = send(
        &app,
        post_json(
            "/api/session",
            &json!({ "username": "alice", "password": "pw" }).to_string(),
            None,
            false,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "totp_required");

    // 正确的 TOTP 登录。
    let code = auth::current_totp(&secret).unwrap();
    let (status, cookie) = login_json(
        &app,
        json!({ "username": "alice", "password": "pw", "totp": code }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(cookie.is_some());

    // 备份码可以登录一次，且会被消费。
    let backup = backup_codes[0].as_str().unwrap().to_string();
    let (status, cookie) = login_json(
        &app,
        json!({ "username": "alice", "password": "pw", "totp": backup }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(cookie.is_some());
    let (status, _, body) = send(
        &app,
        post_json(
            "/api/session",
            &json!({ "username": "alice", "password": "pw", "totp": backup }).to_string(),
            None,
            false,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "invalid totp");
}

#[tokio::test]
async fn password_rate_limit_returns_429_after_five_failures() {
    let dir = temp_dir("rate-limit");
    let state = test_state(&dir);
    state
        .users
        .lock()
        .unwrap()
        .add_user("bob", "correct")
        .unwrap();
    let app = app(state);
    let wrong = json!({ "username": "bob", "password": "nope" }).to_string();

    for _ in 0..5 {
        let (status, _, _) = send(&app, post_json("/api/session", &wrong, None, false)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    let (status, _, body) = send(
        &app,
        post_json(
            "/api/session",
            &json!({ "username": "bob", "password": "nope" }).to_string(),
            None,
            false,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "too many attempts");

    // 锁定期间即使密码正确也拒绝。
    let (status, _) = login_json(&app, json!({ "username": "bob", "password": "correct" })).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}
