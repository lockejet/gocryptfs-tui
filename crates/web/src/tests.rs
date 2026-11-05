//! Web 后端测试：认证、元信息、日志、静态资源与任务（SSE）路径。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;

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

async fn login(app: &Router, token: &str) -> (StatusCode, Option<String>) {
    let req = post_json(
        "/api/session",
        &format!("{{\"token\":\"{token}\"}}"),
        None,
        false,
    );
    let (status, headers, _) = send(app, req).await;
    let cookie = headers
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(|v| v.to_string());
    (status, cookie)
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
