use crate::{
    clients::platform_internal::PlatformInternalClient, config::Config,
    starter_content::catalog::RawCatalog,
};
use std::sync::Arc;

/// Campaign-tier process state. Cloned per-handler invocation; everything
/// inside is `Arc`-shared so this is cheap.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    /// Locale-unresolved catalog, parsed once at startup.
    pub catalog: Arc<RawCatalog>,
    /// Bearer-attached client for platform `/internal/platform/*` callbacks.
    pub platform_internal: PlatformInternalClient,
}
