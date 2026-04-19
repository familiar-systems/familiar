use crate::middleware::auth::AuthenticatedUser;
use axum::Json;
use familiar_systems_app_shared::{auth::MeResponse, id::UserId};

pub async fn me(user: AuthenticatedUser) -> Json<MeResponse> {
    Json(MeResponse {
        id: UserId(user.id),
        hanko_sub: user.hanko_sub,
        email: user.email,
    })
}
