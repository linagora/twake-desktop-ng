use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Helper: start the server on a temp socket and return (socket path, shutdown sender).
async fn start_server() -> (std::path::PathBuf, tokio::sync::watch::Sender<bool>) {
    let dir = tempfile::tempdir().expect("tmpdir");
    let sock_path = dir.path().join("test.sock");

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let path = sock_path.clone();
    tokio::spawn(async move {
        shell_cef::server::run(&path, shutdown_rx)
            .await
            .expect("server::run");
    });

    // Wait briefly for the listener to bind.
    for _ in 0..50 {
        if sock_path.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(sock_path.exists(), "Socket was not created in time");

    // Leak the tempdir so it survives until the test finishes.
    // The shutdown sender drop will clean up the socket file.
    std::mem::forget(dir);

    (sock_path, shutdown_tx)
}

/// Send a raw string to the server socket and read the newline-terminated response.
async fn send_raw(sock_path: &std::path::Path, payload: &str) -> String {
    let stream = UnixStream::connect(sock_path)
        .await
        .expect("connect to socket");

    let (reader, mut writer) = stream.into_split();

    writer.write_all(payload.as_bytes()).await.expect("write");
    writer.write_all(b"\n").await.expect("write newline");
    writer.shutdown().await.expect("shutdown write");

    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();
    buf_reader.read_line(&mut line).await.expect("read response");

    line
}

/// Send a JSON value and parse the response.
async fn send_json(sock_path: &std::path::Path, request: &Value) -> Value {
    let raw = send_raw(sock_path, &request.to_string()).await;
    serde_json::from_str(&raw).expect("parse response JSON")
}

#[tokio::test]
async fn test_unknown_action() {
    let (sock, _shutdown) = start_server().await;

    let resp = send_json(&sock, &json!({"action": "unknown"})).await;

    assert_eq!(resp["status"], "error");
    assert_eq!(resp["error"]["code"], "UNKNOWN_ACTION");
    assert!(resp["error"]["message"].as_str().unwrap().contains("unknown"));
    assert!(resp["meta"]["duration_ms"].is_u64());
    assert!(resp["meta"]["timestamp"].is_string());
}

#[tokio::test]
async fn test_invalid_json() {
    let (sock, _shutdown) = start_server().await;

    let resp_raw = send_raw(&sock, "this is not json{{{").await;
    let resp: Value = serde_json::from_str(&resp_raw).expect("parse response");

    assert_eq!(resp["status"], "error");
    assert_eq!(resp["error"]["code"], "INVALID_JSON");
}

#[tokio::test]
async fn test_missing_url_param() {
    let (sock, _shutdown) = start_server().await;

    let resp = send_json(&sock, &json!({"action": "navigate", "params": {}})).await;

    assert_eq!(resp["status"], "error");
    assert_eq!(resp["error"]["code"], "INVALID_PARAMS");
}

#[tokio::test]
async fn test_navigate_invalid_url() {
    let (sock, _shutdown) = start_server().await;

    let resp = send_json(
        &sock,
        &json!({
            "action": "navigate",
            "params": {"url": "http://256.256.256.256:1/nope", "timeout_ms": 2000}
        }),
    )
    .await;

    assert_eq!(resp["status"], "error");
    let code = resp["error"]["code"].as_str().unwrap();
    assert!(
        code == "NAVIGATE_FAILED" || code == "NAVIGATE_TIMEOUT",
        "Expected NAVIGATE_FAILED or NAVIGATE_TIMEOUT, got {code}"
    );
}

#[tokio::test]
async fn test_navigate_empty_url() {
    let (sock, _shutdown) = start_server().await;

    let resp = send_json(
        &sock,
        &json!({"action": "navigate", "params": {"url": ""}}),
    )
    .await;

    assert_eq!(resp["status"], "error");
    assert_eq!(resp["error"]["code"], "INVALID_PARAMS");
}

#[tokio::test]
async fn test_navigate_success() {
    let (sock, _shutdown) = start_server().await;

    let resp = send_json(
        &sock,
        &json!({
            "action": "navigate",
            "params": {"url": "https://example.com", "timeout_ms": 10000}
        }),
    )
    .await;

    if resp["status"] == "ok" {
        assert_eq!(resp["data"]["url"], "https://example.com");
        assert!(resp["data"]["final_url"].is_string());
        assert!(resp["data"]["title"].is_string());
        assert_eq!(resp["data"]["status_code"], 200);
        assert!(resp["data"]["body"].is_string());
        assert!(resp["data"]["body"].as_str().unwrap().contains("<!doctype html>"));
    } else {
        assert_eq!(resp["error"]["code"], "NAVIGATE_FAILED");
    }
}

#[tokio::test]
async fn test_navigate_with_query_params() {
    let (sock, _shutdown) = start_server().await;

    let resp = send_json(
        &sock,
        &json!({
            "action": "navigate",
            "params": {"url": "https://httpbin.org/anything?foo=bar&num=42", "timeout_ms": 10000}
        }),
    )
    .await;

    assert_eq!(resp["status"], "ok");
    assert!(resp["data"]["body"].as_str().unwrap().contains("foo=bar"));
}

#[tokio::test]
async fn test_shutdown_signal() {
    let (sock, shutdown_tx) = start_server().await;

    let resp = send_json(&sock, &json!({"action": "navigate", "params": {"url": "https://example.com"}})).await;
    
    if resp["status"] != "ok" {
        assert!(resp["error"]["code"].is_string());
    }

    let _ = shutdown_tx.send(true);

    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(!sock.exists(), "Socket should be removed after shutdown");
}
