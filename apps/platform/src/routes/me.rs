use crate::middleware::auth::AuthenticatedUser;
use axum::Json;
use familiar_systems_app_shared::{auth::MeResponse, id::UserId};

#[utoipa::path(
    get,
    path = "/me",
    tag = "auth",
    responses(
        (status = OK, description = "Authenticated user", body = MeResponse),
        (status = UNAUTHORIZED, description = "Authentication required"),
    ),
    security(("bearerAuth" = [])),
)]
pub async fn me(user: AuthenticatedUser) -> Json<MeResponse> {
    Json(MeResponse {
        id: UserId(user.id),
        email: user.email,
    })
}
