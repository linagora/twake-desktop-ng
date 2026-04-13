use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Request
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct Request {
    pub action: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Response
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct Response {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
    pub meta: Meta,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct Meta {
    pub duration_ms: u64,
    pub timestamp: String,
}

impl Meta {
    pub fn from_start(start: Instant) -> Self {
        Self {
            duration_ms: start.elapsed().as_millis() as u64,
            timestamp: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        }
    }
}

// ---------------------------------------------------------------------------
// Error codes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub enum ErrorCode {
    UnknownAction,
    InvalidParams,
    InvalidJson,
    NavigateTimeout,
    NavigateFailed,
    InternalError,
}

impl Serialize for ErrorCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnknownAction => "UNKNOWN_ACTION",
            Self::InvalidParams => "INVALID_PARAMS",
            Self::InvalidJson => "INVALID_JSON",
            Self::NavigateTimeout => "NAVIGATE_TIMEOUT",
            Self::NavigateFailed => "NAVIGATE_FAILED",
            Self::InternalError => "INTERNAL_ERROR",
        }
    }
}

// ---------------------------------------------------------------------------
// Response builders
// ---------------------------------------------------------------------------

impl Response {
    pub fn ok(data: serde_json::Value, start: Instant) -> Self {
        Self {
            status: "ok",
            data: Some(data),
            error: None,
            meta: Meta::from_start(start),
        }
    }

    pub fn error(code: ErrorCode, message: impl Into<String>, start: Instant) -> Self {
        Self {
            status: "error",
            data: None,
            error: Some(ErrorBody {
                code,
                message: message.into(),
            }),
            meta: Meta::from_start(start),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::time::Instant;

    #[test]
    fn test_response_ok_serialization() {
        let start = Instant::now();
        let resp = Response::ok(json!({"url": "https://example.com"}), start);
        let json = serde_json::to_string(&resp).unwrap();
        
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "ok");
        assert_eq!(parsed["data"]["url"], "https://example.com");
        assert!(parsed["meta"]["duration_ms"].is_u64());
        assert!(parsed["meta"]["timestamp"].is_string());
    }

    #[test]
    fn test_response_error_serialization() {
        let start = Instant::now();
        let resp = Response::error(ErrorCode::NavigateTimeout, "Timeout occurred", start);
        let json = serde_json::to_string(&resp).unwrap();
        
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "error");
        assert_eq!(parsed["error"]["code"], "NAVIGATE_TIMEOUT");
        assert_eq!(parsed["error"]["message"], "Timeout occurred");
    }

    #[test]
    fn test_error_code_serialization() {
        let codes = vec![
            (ErrorCode::UnknownAction, "UNKNOWN_ACTION"),
            (ErrorCode::InvalidParams, "INVALID_PARAMS"),
            (ErrorCode::InvalidJson, "INVALID_JSON"),
            (ErrorCode::NavigateTimeout, "NAVIGATE_TIMEOUT"),
            (ErrorCode::NavigateFailed, "NAVIGATE_FAILED"),
            (ErrorCode::InternalError, "INTERNAL_ERROR"),
        ];
        
        for (code, expected) in codes {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
        }
    }

    #[test]
    fn test_request_deserialization() {
        let json = r#"{"action": "navigate", "params": {"url": "https://example.com"}}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        
        assert_eq!(req.action, "navigate");
        assert_eq!(req.params["url"], "https://example.com");
    }

    #[test]
    fn test_request_without_params() {
        let json = r#"{"action": "ping"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        
        assert_eq!(req.action, "ping");
        assert_eq!(req.params, serde_json::Value::Null);
    }
}
