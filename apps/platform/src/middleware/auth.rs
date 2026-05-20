//! Platform auth extractor: validates Hanko JWT + upserts user row.
//!
//! Wraps the shared [`AuthenticatedUser`] extractor from `app-shared` with
//! the platform-specific user-table upsert. Handlers take [`PlatformUser`]
//! instead of the bare `AuthenticatedUser`.

use std::sync::Arc;

use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use chrono::Utc;
use familiar_systems_app_shared::auth::{AuthenticatedUser, HankoSessionValidator};
use sea_orm::{
    ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, SqlErr, sea_query::OnConflict,
};
use uuid::Uuid;

use crate::entities::users;
use crate::error::AppError;
use crate::state::AppState;

impl FromRef<AppState> for Arc<HankoSessionValidator> {
    fn from_ref(state: &AppState) -> Self {
        state.validator.clone()
    }
}

pub struct PlatformUser {
    pub id: Uuid,
    pub email: String,
}

impl<S> FromRequestParts<S> for PlatformUser
where
    AppState: FromRef<S>,
    Arc<HankoSessionValidator>: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = AuthenticatedUser::from_request_parts(parts, state)
            .await
            .map_err(|e| match e {
                familiar_systems_app_shared::auth::extractor::AuthRejection::MissingHeader => {
                    AppError::Unauthorized("missing authorization header".into())
                }
                familiar_systems_app_shared::auth::extractor::AuthRejection::MalformedHeader => {
                    AppError::Unauthorized("expected Bearer scheme".into())
                }
                familiar_systems_app_shared::auth::extractor::AuthRejection::ValidationFailed(
                    auth_err,
                ) => AppError::Auth(auth_err),
            })?;

        let app_state = AppState::from_ref(state);
        let now = Utc::now();

        let am = users::ActiveModel {
            id: Set(user.id),
            email: Set(user.email.clone()),
            created_at: Set(now),
            updated_at: Set(now),
        };
        let row = match users::Entity::insert(am)
            .on_conflict(
                OnConflict::column(users::Column::Id)
                    .update_columns([users::Column::Email, users::Column::UpdatedAt])
                    .to_owned(),
            )
            .exec_with_returning(&app_state.db)
            .await
        {
            Ok(row) => row,
            Err(e) => {
                if let Some(SqlErr::UniqueConstraintViolation(detail)) = e.sql_err()
                    && detail.contains("email")
                {
                    let other_id = users::Entity::find()
                        .filter(users::Column::Email.eq(&user.email))
                        .one(&app_state.db)
                        .await
                        .ok()
                        .flatten()
                        .map(|r| r.id);
                    tracing::error!(
                        incoming_user_id = %user.id,
                        colliding_user_id = ?other_id,
                        email = %user.email,
                        "email unique collision during upsert; local mirror likely stale vs. Hanko"
                    );
                    return Err(AppError::EmailConflict);
                }
                return Err(AppError::from(e));
            }
        };

        Ok(PlatformUser {
            id: row.id,
            email: row.email,
        })
    }
}
