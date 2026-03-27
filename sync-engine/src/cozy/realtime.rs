use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn, error, debug};

/// Events emitted by the Cozy realtime WebSocket.
#[derive(Debug, Clone)]
pub enum RealtimeEvent {
    Created { id: String, path: Option<String>, is_dir: bool },
    Updated { id: String, path: Option<String>, is_dir: bool },
    Deleted { id: String, path: Option<String>, is_dir: bool },
}

#[derive(Serialize)]
struct WsCommand {
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct WsMessage {
    event: Option<String>,
    payload: Option<WsPayload>,
}

#[derive(Deserialize, Debug)]
struct WsPayload {
    id: Option<String>,
    #[serde(rename = "type")]
    doc_type: Option<String>,
    doc: Option<WsDoc>,
}

#[derive(Deserialize, Debug)]
struct WsDoc {
    #[serde(rename = "_id")]
    id: Option<String>,
    path: Option<String>,
    #[serde(rename = "type")]
    file_type: Option<String>,
}

/// Connect to Cozy realtime WebSocket and yield file events.
pub async fn realtime_connect(
    base_url: &str,
    token: &str,
    cookie: Option<&str>,
    mut on_event: impl FnMut(RealtimeEvent) + Send,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws_url = base_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");
    let url = format!("{}/realtime/", ws_url.trim_end_matches('/'));

    info!("Realtime: connecting to {}", url);

    // Build request with required WebSocket headers
    use tokio_tungstenite::tungstenite::handshake::client::generate_key;
    let mut request = tokio_tungstenite::tungstenite::http::Request::builder()
        .uri(&url)
        .header("Host", url.replace("wss://", "").replace("ws://", "").split('/').next().unwrap_or(""))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", generate_key())
        .header("Sec-WebSocket-Protocol", "io.cozy.websocket")
        .header("Authorization", format!("Bearer {}", token));

    if let Some(cookie_str) = cookie {
        request = request.header("Cookie", cookie_str);
    }

    let request = request.body(())?;
    let (mut ws, _resp) = connect_async(request).await?;

    info!("Realtime: connected, authenticating...");

    // AUTH
    let auth_cmd = WsCommand {
        method: "AUTH".to_string(),
        payload: Some(serde_json::Value::String(token.to_string())),
    };
    ws.send(Message::Text(serde_json::to_string(&auth_cmd)?)).await?;

    // SUBSCRIBE to io.cozy.files
    let sub_cmd = WsCommand {
        method: "SUBSCRIBE".to_string(),
        payload: Some(serde_json::json!({"type": "io.cozy.files"})),
    };
    ws.send(Message::Text(serde_json::to_string(&sub_cmd)?)).await?;

    info!("Realtime: subscribed to io.cozy.files");

    while let Some(msg) = ws.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                debug!("Realtime: raw message: {}", text);
                match serde_json::from_str::<WsMessage>(&text) {
                    Ok(ws_msg) => {
                        if let Some(event) = parse_event(&ws_msg) {
                            info!("Realtime: {:?}", event);
                            on_event(event);
                        }
                    }
                    Err(e) => {
                        debug!("Realtime: could not parse message: {} ({})", text, e);
                    }
                }
            }
            Ok(Message::Ping(data)) => {
                let _ = ws.send(Message::Pong(data)).await;
            }
            Ok(Message::Close(_)) => {
                info!("Realtime: server closed connection");
                break;
            }
            Err(e) => {
                error!("Realtime: WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    Ok(())
}

fn parse_event(msg: &WsMessage) -> Option<RealtimeEvent> {
    let event_type = msg.event.as_deref()?;
    let payload = msg.payload.as_ref()?;
    let doc = payload.doc.as_ref();

    let id = doc.and_then(|d| d.id.clone())
        .or_else(|| payload.id.clone())?;
    let path = doc.and_then(|d| d.path.clone());
    let is_dir = doc.and_then(|d| d.file_type.as_deref()) == Some("directory");

    match event_type {
        "CREATED" => Some(RealtimeEvent::Created { id, path, is_dir }),
        "UPDATED" => Some(RealtimeEvent::Updated { id, path, is_dir }),
        "DELETED" => Some(RealtimeEvent::Deleted { id, path, is_dir }),
        _ => None,
    }
}
