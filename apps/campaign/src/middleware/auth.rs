//! Re-exports the shared [`AuthenticatedUser`] extractor and wires it to
//! this binary's [`AppState`] via `FromRef`.

use std::sync::Arc;

use axum::extract::FromRef;
use familiar_systems_app_shared::auth::HankoSessionValidator;

use crate::state::AppState;

pub use familiar_systems_app_shared::auth::AuthenticatedUser;

impl FromRef<AppState> for Arc<HankoSessionValidator> {
    fn from_ref(state: &AppState) -> Self {
        state.validator.clone()
    }
}
