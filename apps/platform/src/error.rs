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

// TODO(hardening): internal error Display content leaks into HTTP response
// bodies. `AppError::Db(e)` formats `sea_orm::DbErr` directly (exposes
// constraint names, column names, SQL fragments); `AppError::Auth(_)` via
// `self.to_string()` can surface `AuthError::RequestFailed(reqwest::Error)`,
// which may include the Hanko tenant URL. Fix: log the full error at
// `tracing::error!` and return a generic string to clients. Must land
// before prod traffic arrives.
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            AppError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m.clone()),
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found".into()),
            AppError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m.clone()),
            AppError::Db(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {e}")),
            AppError::Auth(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
        };
        (status, Json(ErrorBody { error: msg })).into_response()
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
