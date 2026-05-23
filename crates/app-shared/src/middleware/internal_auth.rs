//! Bearer-token middleware for `/internal/*` routes.
//!
//! Layer 3 of the internal-API defense stack. Layers 1 and 2 (Ingress
//! discipline + NetworkPolicy) are enforced at the cluster boundary; this
//! middleware is the in-process backstop that catches Ingress drift,
//! namespace misconfiguration, and contributors who forget to scope a new
//! Ingress. See the bearer-rotation contract in
//! `infra/CLAUDE.md`.
//!
//! Constant-time compare via [`subtle::ConstantTimeEq`] avoids a timing
//! oracle on the bearer string. Both primary and secondary are accepted so
//! that rotation has a window where both old and new tokens work.

use axum::{
    extract::{Request, State},
    http::{StatusCode, header::AUTHORIZATION},
    middleware::Next,
    response::Response,
};
use subtle::ConstantTimeEq;

/// The bearer tokens to validate against. Each app wires this to its own
/// `AppState` via `FromRef`.
#[derive(Clone)]
pub struct InternalBearerConfig {
    pub primary: String,
    pub secondary: Option<String>,
}

/// Extracts the bearer from `Authorization`, constant-time-compares against
/// the configured primary (and optional secondary) bearers, and rejects
/// 401 on absent/mismatched. The request body is untouched.
pub async fn require_internal_bearer(
    State(config): State<InternalBearerConfig>,
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

    let primary_match = constant_time_eq(token, &config.primary);
    let secondary_match = config
        .secondary
        .as_deref()
        .map(|s| constant_time_eq(token, s))
        .unwrap_or(false);

    if primary_match || secondary_match {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// `subtle::ConstantTimeEq` for byte slices, returning a regular bool. The
/// `unwrap_u8 == 1` step is the documented way to convert `Choice` back to
/// `bool` once the constant-time comparison is done.
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
    fn constant_time_eq_returns_false_on_different_length() {
        assert!(!constant_time_eq("hello", "hello!"));
    }

    #[test]
    fn constant_time_eq_returns_false_on_same_length_different_content() {
        assert!(!constant_time_eq("hello", "world"));
    }

    #[test]
    fn constant_time_eq_returns_false_on_empty_vs_nonempty() {
        assert!(!constant_time_eq("", "x"));
    }
}
