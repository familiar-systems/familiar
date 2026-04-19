use crate::{entities::users, error::AppError, state::AppState};
use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use chrono::Utc;
use sea_orm::{ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, sea_query::OnConflict};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub id: Uuid,
    pub hanko_sub: String,
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
        let email = claims
            .email
            .as_ref()
            .map(|e| e.address.clone())
            .ok_or_else(|| AppError::Unauthorized("hanko claims missing email".into()))?;
        let now = Utc::now();

        let am = users::ActiveModel {
            id: Set(Uuid::now_v7()),
            hanko_sub: Set(claims.subject.clone()),
            email: Set(email.clone()),
            created_at: Set(now),
            updated_at: Set(now),
        };
        users::Entity::insert(am)
            .on_conflict(
                OnConflict::column(users::Column::HankoSub)
                    .update_columns([users::Column::Email, users::Column::UpdatedAt])
                    .to_owned(),
            )
            .exec(&app_state.db)
            .await?;

        let row = users::Entity::find()
            .filter(users::Column::HankoSub.eq(&claims.subject))
            .one(&app_state.db)
            .await?
            .ok_or_else(|| AppError::Internal("upsert did not land".into()))?;

        Ok(AuthenticatedUser {
            id: row.id,
            hanko_sub: row.hanko_sub,
            email: row.email,
        })
    }
}
