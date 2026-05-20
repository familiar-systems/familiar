//! Axum extractor for Hanko JWT authentication.
//!
//! Validates the `Authorization: Bearer` header via [`HankoSessionValidator`]
//! and returns an [`AuthenticatedUser`]. Both platform and campaign binaries
//! use this extractor; the platform wraps it with a user-table upsert.
//!
//! The `FromRequestParts` impl is generic over any state `S` that can
//! provide `Arc<HankoSessionValidator>` via `FromRef`. Each binary
//! implements `FromRef` for its own `AppState`.

use std::sync::Arc;

use axum::{
    extract::{FromRef, FromRequestParts},
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use uuid::Uuid;

use super::{AuthError, HankoSessionValidator};

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub id: Uuid,
    pub email: String,
}

pub enum AuthRejection {
    MissingHeader,
    MalformedHeader,
    ValidationFailed(AuthError),
}

impl AuthRejection {
    pub fn reason(&self) -> String {
        match self {
            AuthRejection::MissingHeader => "missing authorization header".into(),
            AuthRejection::MalformedHeader => "expected Bearer scheme".into(),
            AuthRejection::ValidationFailed(e) => e.to_string(),
        }
    }
}

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        tracing::debug!(reason = %self.reason(), "auth rejected");
        StatusCode::UNAUTHORIZED.into_response()
    }
}

impl<S> FromRequestParts<S> for AuthenticatedUser
where
    Arc<HankoSessionValidator>: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AuthRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let validator = Arc::<HankoSessionValidator>::from_ref(state);
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .ok_or(AuthRejection::MissingHeader)?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or(AuthRejection::MalformedHeader)?;

        let claims = validator
            .validate(token)
            .await
            .map_err(AuthRejection::ValidationFailed)?;

        let span = tracing::Span::current();
        span.record("user_id", tracing::field::display(claims.subject));

        Ok(AuthenticatedUser {
            id: claims.subject,
            email: claims.email,
        })
    }
}
