//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! API key authentication middleware (§5.A.2).
//!
//! Checks the `Authorization: Bearer <key>` header on every request to
//! `/api/v1/`.  Returns 401 if the header is absent or the
//! key does not match the configured value.

use crate::error::ApiError;
use crate::state::AppState;
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};

pub async fn require_api_key(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let provided = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));

    match provided {
        Some(key) if key == state.api_key() => next.run(request).await,
        _ => ApiError::Unauthorized.into_response(),
    }
}
