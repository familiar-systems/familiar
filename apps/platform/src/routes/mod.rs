mod health;
mod me;

use crate::state::AppState;
use axum::{routing::get, Router};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health::health))
        .route("/me", get(me::me))
}
