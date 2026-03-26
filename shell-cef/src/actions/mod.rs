pub mod navigate;

use std::time::Instant;

use crate::protocol::{ErrorCode, Request, Response};

/// Dispatch a parsed request to the appropriate action handler.
pub async fn dispatch(req: Request, start: Instant) -> Response {
    match req.action.as_str() {
        "navigate" => navigate::execute(req.params, start).await,
        _ => Response::error(
            ErrorCode::UnknownAction,
            format!("Unknown action: {}", req.action),
            start,
        ),
    }
}
