use crate::{
    actors::registry::CampaignRegistry, clients::platform_internal::PlatformInternalClient,
    config::Config, starter_content::catalog::RawCatalog,
};
use kameo::actor::ActorRef;
use std::sync::Arc;

/// Campaign-tier process state. Cloned per-handler invocation; everything
/// inside is `Arc`-shared or a cheap `ActorRef` clone.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    /// Locale-unresolved catalog, parsed once at startup.
    pub catalog: Arc<RawCatalog>,
    /// Bearer-attached client for platform `/internal/platform/*` callbacks.
    pub platform_internal: PlatformInternalClient,
    /// Handle to the process-lifetime `CampaignRegistry`. HTTP handlers
    /// ask the registry to ensure or look up per-campaign supervisors.
    pub registry: ActorRef<CampaignRegistry>,
}
