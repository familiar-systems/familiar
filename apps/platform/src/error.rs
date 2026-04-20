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
// structured `error_kind` + `error_detail` event. This closes two leaks:
// - `AppError::Db(_)` no longer echoes constraint/column names or SQL;
// - `AppError::Auth(_)` no longer surfaces reqwest error strings that can
//   include the Hanko tenant URL.
// `Unauthorized(m)` intentionally echoes `m` because that variant is
// constructed only with caller-facing, PII-free messages (missing header,
// wrong scheme) — it's a small API affordance, not a leak.
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
            AppError::Db(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal error".into(),
                "Db",
            ),
            AppError::Auth(_) => (StatusCode::UNAUTHORIZED, "unauthorized".into(), "Auth"),
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
    fn auth_error_maps_to_401() {
        let r = AppError::Auth(AuthError::MissingHeader).into_response();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
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
