use axum::http::StatusCode;

#[utoipa::path(
    get,
    path = "/health",
    tag = "system",
    responses((status = OK, description = "Service is healthy")),
)]
pub async fn health() -> StatusCode {
    StatusCode::OK
}
