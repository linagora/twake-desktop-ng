use std::path::Path;
use std::time::Instant;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tracing::{debug, error, info, warn};

use crate::actions;
use crate::protocol::{ErrorCode, Response};

/// Remove a stale socket file if it exists and is not actively listening.
fn cleanup_stale_socket(path: &Path) {
    if path.exists() {
        // Try connecting to see if someone is listening.
        match std::os::unix::net::UnixStream::connect(path) {
            Ok(_) => {
                // Another process is using this socket — we must not remove it.
                warn!(?path, "Socket is in use by another process");
            }
            Err(_) => {
                info!(?path, "Removing stale socket");
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// Set socket file permissions to 0600 (owner read/write only).
fn set_socket_permissions(path: &Path) -> std::io::Result<()> {
    use nix::sys::stat::{fchmodat, FchmodatFlags, Mode};
    fchmodat(None, path, Mode::S_IRUSR | Mode::S_IWUSR, FchmodatFlags::FollowSymlink)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
}

/// Run the Unix socket server.
///
/// Accepts one connection at a time (one-shot mode per connection), reads a
/// single newline-terminated JSON request, dispatches it, and writes back the
/// JSON response followed by a newline.
pub async fn run(sock_path: &Path, shutdown: tokio::sync::watch::Receiver<bool>) -> std::io::Result<()> {
    cleanup_stale_socket(sock_path);

    let listener = UnixListener::bind(sock_path)?;
    set_socket_permissions(sock_path)?;

    info!(?sock_path, "Listening");

    let mut shutdown = shutdown;

    loop {
        tokio::select! {
            biased;
            _ = shutdown.changed() => {
                info!("Shutdown signal received");
                break;
            }
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, _addr)) => {
                        // Spawn a task so we can continue accepting while processing.
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream).await {
                                error!(error = %e, "Connection handler error");
                            }
                        });
                    }
                    Err(e) => {
                        error!(error = %e, "Accept failed");
                    }
                }
            }
        }
    }

    // Cleanup socket on shutdown.
    info!(?sock_path, "Removing socket");
    let _ = std::fs::remove_file(sock_path);

    Ok(())
}

async fn handle_connection(stream: tokio::net::UnixStream) -> std::io::Result<()> {
    let start = Instant::now();

    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    buf_reader.read_line(&mut line).await?;
    debug!(raw = %line.trim_end(), "Received request");

    let response = match serde_json::from_str::<crate::protocol::Request>(&line) {
        Ok(req) => actions::dispatch(req, start).await,
        Err(e) => Response::error(
            ErrorCode::InvalidJson,
            format!("Invalid JSON: {e}"),
            start,
        ),
    };

    let mut out = serde_json::to_vec(&response).unwrap_or_else(|e| {
        // Last resort — should never happen.
        format!(
            r#"{{"status":"error","error":{{"code":"INTERNAL_ERROR","message":"Serialization failed: {e}"}},"meta":{{"duration_ms":0,"timestamp":""}}}}"#,
        )
        .into_bytes()
    });
    out.push(b'\n');
    writer.write_all(&out).await?;
    writer.shutdown().await?;

    debug!(duration_ms = start.elapsed().as_millis() as u64, "Connection handled");

    Ok(())
}
