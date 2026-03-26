use std::time::Instant;

use serde::Deserialize;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tracing::info;

use crate::protocol::{ErrorCode, Response};

#[derive(Debug, Deserialize)]
struct OidcParams {
    issuer: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    #[serde(default)]
    pkce: bool,
    #[serde(default)]
    scopes: Vec<String>,
}

async fn handle_oidc_callback(
    listener: TcpListener,
    sender: oneshot::Sender<String>,
) -> std::io::Result<()> {
    let (mut stream, addr) = listener.accept().await?;
    info!(?addr, "OIDC callback received");

    let mut buf = String::new();
    let mut reader = BufReader::new(&mut stream);
    reader.read_line(&mut buf).await?;

    let response = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h1>Authentication successful!</h1><p>You can close this window.</p></body></html>";
    stream.write_all(response).await?;

    if let Some(query_start) = buf.find("?code=") {
        let query = &buf[query_start + 1..];
        if let Some(code_start) = query.find("code=") {
            let code_start = code_start + 5;
            let code_end = query[code_start..].find('&').map(|p| code_start + p).unwrap_or(query.len());
            let code = &query[code_start..code_end];
            let _ = sender.send(code.to_string());
        }
    }

    Ok(())
}

fn generate_pkce() -> (String, String) {
    use rand::Rng;

    let mut rng = rand::thread_rng();
    let verifier: String = (0..128)
        .map(|_| {
            let idx = rng.gen_range(0..BASE64URL_CHARS.len());
            BASE64URL_CHARS[idx] as char
        })
        .collect();

    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    verifier.as_bytes().hash(&mut hasher);
    let hash = hasher.finish();

    let challenge = encode_base64url(hash.to_le_bytes());
    (verifier, challenge)
}

const BASE64URL_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

fn encode_base64url(bytes: impl AsRef<[u8]>) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(bytes)
}

pub async fn execute(params: serde_json::Value, start: Instant) -> Response {
    let parsed: OidcParams = match serde_json::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return Response::error(
                ErrorCode::InvalidParams,
                format!("Invalid OIDC params: {e}"),
                start,
            );
        }
    };

    let issuer = match parsed.issuer {
        Some(i) if !i.is_empty() => i,
        _ => {
            return Response::error(
                ErrorCode::InvalidParams,
                "Missing required parameter: issuer",
                start,
            );
        }
    };

    let client_id = match parsed.client_id {
        Some(c) if !c.is_empty() => c,
        _ => {
            return Response::error(
                ErrorCode::InvalidParams,
                "Missing required parameter: client_id",
                start,
            );
        }
    };

    let redirect_uri = parsed.redirect_uri.unwrap_or_else(|| "http://localhost:5000/callback".to_string());

    let scopes = if parsed.scopes.is_empty() {
        vec!["openid".to_string(), "profile".to_string(), "email".to_string()]
    } else {
        parsed.scopes.clone()
    };

    let (code_verifier, code_challenge) = if parsed.pkce {
        generate_pkce()
    } else {
        (String::new(), String::new())
    };

    info!(
        issuer = %issuer,
        client_id = %client_id,
        redirect_uri = %redirect_uri,
        pkce = parsed.pkce,
        "Starting OIDC flow"
    );

    let (callback_tx, callback_rx) = oneshot::channel();

    let listener = match TcpListener::bind("127.0.0.1:5000").await {
        Ok(l) => l,
        Err(e) => {
            return Response::error(
                ErrorCode::InternalError,
                format!("Failed to bind callback server: {e}"),
                start,
            );
        }
    };

    let callback_server = tokio::spawn(async move {
        let _ = handle_oidc_callback(listener, callback_tx).await;
    });

    let auth_endpoint = format!("{}/authorize", issuer.trim_end_matches('/'));
    let scopes_str = scopes.join(" ");

    let mut auth_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}",
        auth_endpoint,
        urlencoding::encode(&client_id),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(&scopes_str)
    );

    if !code_challenge.is_empty() {
        auth_url.push_str(&format!("&code_challenge={}&code_challenge_method=S256", urlencoding::encode(&code_challenge)));
    }

    if let Err(e) = open::that(&auth_url) {
        return Response::error(
            ErrorCode::InternalError,
            format!("Failed to open browser: {e}"),
            start,
        );
    }

    info!(url = %auth_url, "Browser opened for authentication");

    let code = match callback_rx.await {
        Ok(c) => c,
        Err(_) => {
            return Response::error(
                ErrorCode::InternalError,
                "Callback server dropped without receiving code",
                start,
            );
        }
    };

    let token_endpoint = format!("{}/token", issuer.trim_end_matches('/'));

    let mut token_params = vec![
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("client_id", client_id.as_str()),
    ];

    if !code_verifier.is_empty() {
        token_params.push(("code_verifier", code_verifier.as_str()));
    }

    let client = reqwest::Client::new();

    let token_response = client
        .post(&token_endpoint)
        .form(&token_params)
        .send()
        .await;

    let token_response = match token_response {
        Ok(r) => r,
        Err(e) => {
            return Response::error(
                ErrorCode::InternalError,
                format!("Token exchange failed: {e}"),
                start,
            );
        }
    };

    if !token_response.status().is_success() {
        let status = token_response.status();
        let body = token_response.text().await.unwrap_or_default();
        return Response::error(
            ErrorCode::InternalError,
            format!("Token exchange failed: {} - {}", status, body),
            start,
        );
    }

    let tokens: serde_json::Value = match token_response.json().await {
        Ok(t) => t,
        Err(e) => {
            return Response::error(
                ErrorCode::InternalError,
                format!("Failed to parse token response: {e}"),
                start,
            );
        }
    };

    callback_server.abort();

    Response::ok(
        json!({
            "access_token": tokens.get("access_token").unwrap_or(&serde_json::Value::Null),
            "refresh_token": tokens.get("refresh_token").unwrap_or(&serde_json::Value::Null),
            "id_token": tokens.get("id_token").unwrap_or(&serde_json::Value::Null),
            "token_type": tokens.get("token_type").unwrap_or(&serde_json::Value::Null),
            "expires_in": tokens.get("expires_in").unwrap_or(&serde_json::Value::Null),
        }),
        start,
    )
}
