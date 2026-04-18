use crate::middleware::auth::AuthenticatedUser;
use axum::Json;
use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize)]
pub struct MeResponse {
    pub id: Uuid,
    pub hanko_sub: String,
    pub email: Option<String>,
}

pub async fn me(user: AuthenticatedUser) -> Json<MeResponse> {
    Json(MeResponse {
        id: user.id,
        hanko_sub: user.hanko_sub,
        email: user.email,
    })
}
