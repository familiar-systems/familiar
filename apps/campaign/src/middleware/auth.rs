//! Re-exports the shared [`AuthenticatedUser`] extractor and wires it to
//! this binary's [`AppState`] via `FromRef`.

use std::sync::Arc;

use axum::extract::FromRef;
use familiar_systems_app_shared::auth::HankoSessionValidator;
use familiar_systems_app_shared::middleware::internal_auth::InternalBearerConfig;

use crate::state::AppState;

pub use familiar_systems_app_shared::auth::AuthenticatedUser;

impl FromRef<AppState> for Arc<HankoSessionValidator> {
    fn from_ref(state: &AppState) -> Self {
        state.validator.clone()
    }
}

impl FromRef<AppState> for InternalBearerConfig {
    fn from_ref(state: &AppState) -> Self {
        InternalBearerConfig {
            primary: state.config.internal_bearer_primary.clone(),
            secondary: state.config.internal_bearer_secondary.clone(),
        }
    }
}
