//! Bearer-token middleware for `/internal/campaign/*` routes.
//!
//! Mirror of `apps/platform/src/middleware/internal_auth.rs`. See that file
//! for the rationale (layer 3 backstop; cluster-level Ingress + NetworkPolicy
//! are the primary controls).

use crate::state::AppState;
use axum::{
    extract::{Request, State},
    http::{StatusCode, header::AUTHORIZATION},
    middleware::Next,
    response::Response,
};
use subtle::ConstantTimeEq;

pub async fn require_internal_bearer(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let header = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let token = header
        .strip_prefix("Bearer ")
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let primary_match = constant_time_eq(token, &state.config.internal_bearer_primary);
    let secondary_match = state
        .config
        .internal_bearer_secondary
        .as_deref()
        .map(|s| constant_time_eq(token, s))
        .unwrap_or(false);

    if primary_match || secondary_match {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).unwrap_u8() == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_returns_true_on_match() {
        assert!(constant_time_eq("hello", "hello"));
    }

    #[test]
    fn constant_time_eq_returns_false_on_mismatch() {
        assert!(!constant_time_eq("hello", "world"));
        assert!(!constant_time_eq("hello", "hello!"));
    }
}
