use crate::{entities::users, error::AppError, state::AppState};
use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use chrono::Utc;
use sea_orm::{
    ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, SqlErr, sea_query::OnConflict,
};
use uuid::Uuid;

/// Authenticated user, proven via Hanko and persisted in `users`.
///
/// `id` is the Hanko subject (= the `users` primary key); see
/// `apps/platform/src/entities/users.rs`.
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub id: Uuid,
    pub email: String,
}

impl<S> FromRequestParts<S> for AuthenticatedUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .ok_or(AppError::Unauthorized(
                "missing authorization header".into(),
            ))?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or(AppError::Unauthorized("expected Bearer scheme".into()))?;

        let claims = app_state.validator.validate(token).await?;
        let now = Utc::now();
        let id = claims.subject;

        // TODO add rate limiting cached by user ID
        // Currently, every login writes a row to the database.
        // This is wildly inefficient.

        // Upsert by id (= Hanko subject). ON CONFLICT (id) handles the normal
        // repeat-login case; a UNIQUE(email) violation on either the INSERT
        // or the UPDATE path surfaces as AppError::EmailConflict → 409.
        // exec_with_returning uses SQLite's RETURNING clause to get the row
        // back in one round-trip.
        let am = users::ActiveModel {
            id: Set(id),
            email: Set(claims.email.clone()),
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
                // FIXME we probably want a detail.contains that's less dialect-fragile
                {
                    // Look up the other row to give the log event both
                    // parties' ids. Best-effort; if the lookup itself
                    // errors, we still emit the event with what we have.
                    let other_id = users::Entity::find()
                        .filter(users::Column::Email.eq(&claims.email))
                        .one(&app_state.db)
                        .await
                        .ok()
                        .flatten()
                        .map(|r| r.id);
                    tracing::error!(
                        incoming_user_id = %id,
                        colliding_user_id = ?other_id,
                        email = %claims.email,
                        "email unique collision during upsert; local mirror likely stale vs. Hanko"
                    );
                    return Err(AppError::EmailConflict);
                }
                return Err(AppError::from(e));
            }
        };

        // Populate the wide-event correlation fields declared as Empty on the
        // request span (see routes::make_request_span). These IDs are
        // pseudonymous and logged under legitimate interest (application
        // reliability); email is *not* recorded here and must not be logged
        // on the success path.
        let span = tracing::Span::current();
        span.record("user_id", tracing::field::display(row.id));
        span.record("session_id", tracing::field::display(&claims.session_id));

        Ok(AuthenticatedUser {
            id: row.id,
            email: row.email,
        })
    }
}
