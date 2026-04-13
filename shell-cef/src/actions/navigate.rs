use std::time::Instant;

use serde::Deserialize;
use serde_json::json;
use tracing::info;

use crate::browser::CefController;
use crate::protocol::{ErrorCode, Response};

#[derive(Debug, Deserialize)]
struct NavigateParams {
    url: Option<String>,
    #[serde(default = "default_wait_until")]
    #[allow(dead_code)]
    wait_until: String,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
}

fn default_wait_until() -> String {
    "load".into()
}

fn default_timeout_ms() -> u64 {
    30_000
}

/// Extract the text content of the first `<title>` tag from raw HTML.
fn extract_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let after_tag = lower[start..].find('>')?;
    let content_start = start + after_tag + 1;
    let end = lower[content_start..].find("</title>")?;
    let title = &html[content_start..content_start + end];
    Some(title.trim().to_string())
}

pub async fn execute(params: serde_json::Value, start: Instant) -> Response {
    let parsed: NavigateParams = match serde_json::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return Response::error(
                ErrorCode::InvalidParams,
                format!("Invalid navigate params: {e}"),
                start,
            );
        }
    };

    let url = match parsed.url {
        Some(ref u) if !u.is_empty() => u.clone(),
        _ => {
            return Response::error(
                ErrorCode::InvalidParams,
                "Missing required parameter: url",
                start,
            );
        }
    };

    info!(url = %url, timeout_ms = parsed.timeout_ms, "navigate via CEF");

    let cef_url = url.clone();
    let result = CefController::navigate(cef_url).await;
    match result {
        Ok(body) => {
            let title = extract_title(&body).unwrap_or_default();
            Response::ok(
                json!({
                    "url": url,
                    "final_url": url,
                    "title": title,
                    "status_code": 200,
                    "body": body,
                }),
                start,
            )
        }
        Err(e) => Response::error(
            ErrorCode::NavigateFailed,
            format!("CEF navigation failed: {e}"),
            start,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_title() {
        assert_eq!(
            extract_title("<html><head><title>Hello World</title></head></html>"),
            Some("Hello World".into())
        );
        assert_eq!(
            extract_title("<TITLE>Upper Case</TITLE>"),
            Some("Upper Case".into())
        );
        assert_eq!(
            extract_title("<title>  Trimmed  </title>"),
            Some("Trimmed".into())
        );
        assert_eq!(extract_title("<html>No title here</html>"), None);
    }
}
