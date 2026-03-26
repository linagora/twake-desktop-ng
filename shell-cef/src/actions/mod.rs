pub mod navigate;
pub mod oidc;

use std::time::Instant;

use crate::protocol::{ErrorCode, Request, Response};

pub async fn dispatch(req: Request, start: Instant) -> Response {
    match req.action.as_str() {
        "navigate" => navigate::execute(req.params, start).await,
        "auth.oidc_start" => oidc::execute(req.params, start).await,
        _ => Response::error(
            ErrorCode::UnknownAction,
            format!("Unknown action: {}", req.action),
            start,
        ),
    }
}
