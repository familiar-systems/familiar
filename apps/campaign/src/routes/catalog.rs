//! `GET /catalog/systems`: locale-resolved game systems and bundle templates.
//!
//! Honors `?locale=<bcp-47>` (precedence) then `Accept-Language` (first tag
//! only; no q-value parsing in v0). FE always passes an explicit
//! `?locale=` so the header path is a fallback for non-FE callers like
//! curl-from-a-tool.

use crate::{starter_content::catalog::Catalog, state::AppState};
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use familiar_systems_campaign_shared::onboarding::catalog::CatalogResponse;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CatalogQuery {
    locale: Option<String>,
}

pub async fn list_systems(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<CatalogQuery>,
) -> Json<CatalogResponse> {
    let locale = q
        .locale
        .or_else(|| {
            headers
                .get(axum::http::header::ACCEPT_LANGUAGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.split(',').next())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "en".to_string());

    let cat = Catalog::from_raw(&state.catalog, &locale);
    Json(CatalogResponse {
        systems: cat.systems,
        byo: cat.byo,
    })
}
