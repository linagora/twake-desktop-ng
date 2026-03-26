use std::time::Instant;

use serde_json::json;

use crate::protocol::{ErrorCode, Response};

pub async fn navigate_to(url: &str, _timeout_ms: u64) -> Result<String, String> {
    let url_string = url.to_string();
    Err("CEF browser integration pending".to_string())
}

pub async fn execute(url: &str, timeout_ms: u64, start: Instant) -> Response {
    match navigate_to(url, timeout_ms).await {
        Ok(body) => Response::ok(
            json!({
                "url": url,
                "final_url": url,
                "title": "",
                "status_code": 200,
                "body": body,
            }),
            start,
        ),
        Err(e) => Response::error(ErrorCode::NavigateFailed, e, start),
    }
}
