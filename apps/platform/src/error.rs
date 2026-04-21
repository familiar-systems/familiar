use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use familiar_systems_app_shared::auth::AuthError;
use serde::Serialize;

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("not found")]
    NotFound,
    #[error("internal: {0}")]
    Internal(String),
    /// Email UNIQUE violation during upsert. Arises when our local mirror
    /// is stale vs. Hanko's current state (e.g. user B changed email in
    /// Hanko but hasn't logged in since; user A now claims that address at
    /// Hanko and logs in). Surfaces as 409; the auth middleware logs both
    /// ids so the stale row can be reconciled out-of-band.
    #[error("email conflict")]
    EmailConflict,
    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
    #[error(transparent)]
    Auth(#[from] AuthError),
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

// Error-body policy: clients see a stable, generic message per variant; the
// full error (including sea-orm detail or reqwest URLs) goes to the log as a
// structured `error_kind` + `error_detail` event. This keeps constraint
// names, SQL fragments, and the Hanko tenant URL out of HTTP responses.
// `Unauthorized(m)` is the sole exception: its message is echoed because
// the variant is only constructed with caller-facing, PII-free strings
// (missing header, wrong scheme).
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, public_msg, kind) = match &self {
            AppError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m.clone(), "Unauthorized"),
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found".into(), "NotFound"),
            AppError::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal error".into(),
                "Internal",
            ),
            AppError::EmailConflict => (
                StatusCode::CONFLICT,
                "email already registered to another account; contact support".into(),
                "EmailConflict",
            ),
            AppError::Db(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal error".into(),
                "Db",
            ),
            // Auth sub-variants get their own `kind` so log aggregation can
            // answer questions like "how many Hanko timeouts per minute" with
            // a first-class filter instead of regex over `error_detail`.
            // Status choices follow the semantics we want to surface to the
            // SPA: 401 for "your token is the problem" (MissingHeader /
            // SessionRejected → redirect to login), 504/502 for "upstream
            // dep is the problem" (retry with backoff).
            AppError::Auth(e) => match e {
                AuthError::MissingHeader => (
                    StatusCode::UNAUTHORIZED,
                    "unauthorized".into(),
                    "AuthMissingHeader",
                ),
                AuthError::SessionRejected(_) => (
                    StatusCode::UNAUTHORIZED,
                    "unauthorized".into(),
                    "AuthRejected",
                ),
                AuthError::UpstreamTimeout => (
                    StatusCode::GATEWAY_TIMEOUT,
                    "authentication service timed out".into(),
                    "AuthUpstreamTimeout",
                ),
                AuthError::UpstreamError(_) => (
                    StatusCode::BAD_GATEWAY,
                    "authentication service unavailable".into(),
                    "AuthUpstreamError",
                ),
            },
        };
        // Inherits request_id / user_id / session_id from the enclosing
        // request span (see routes::make_request_span + middleware::auth).
        tracing::error!(error_kind = kind, error_detail = %self, "request failed");
        (status, Json(ErrorBody { error: public_msg })).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[test]
    fn unauthorized_maps_to_401() {
        let r = AppError::Unauthorized("nope".into()).into_response();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn auth_missing_header_maps_to_401() {
        let r = AppError::Auth(AuthError::MissingHeader).into_response();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn auth_session_rejected_maps_to_401() {
        let r = AppError::Auth(AuthError::SessionRejected("bad token".into())).into_response();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn auth_upstream_timeout_maps_to_504() {
        let r = AppError::Auth(AuthError::UpstreamTimeout).into_response();
        assert_eq!(r.status(), StatusCode::GATEWAY_TIMEOUT);
    }

    #[test]
    fn auth_upstream_error_maps_to_502() {
        let r =
            AppError::Auth(AuthError::UpstreamError("connection refused".into())).into_response();
        assert_eq!(r.status(), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn not_found_maps_to_404() {
        let r = AppError::NotFound.into_response();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn internal_maps_to_500() {
        let r = AppError::Internal("boom".into()).into_response();
        assert_eq!(r.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
