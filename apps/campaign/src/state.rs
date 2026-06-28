use crate::{
    actors::registry::{CampaignRegistry, CampaignTable},
    clients::platform_internal::PlatformInternalClient,
    config::Config,
    starter_content::catalog::RawCatalog,
};
use familiar_systems_app_shared::auth::HankoSessionValidator;
use kameo::actor::ActorRef;
use std::sync::Arc;

/// Campaign-tier process state. Cloned per-handler invocation; everything
/// inside is `Arc`-shared or a cheap `ActorRef` clone.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    /// Hanko JWT validator, shared across all handlers.
    pub validator: Arc<HankoSessionValidator>,
    /// Locale-unresolved catalog, parsed once at startup.
    pub catalog: Arc<RawCatalog>,
    /// Bearer-attached client for platform `/internal/platform/*` callbacks.
    pub platform_internal: PlatformInternalClient,
    /// Handle to the process-lifetime `CampaignRegistry`. Handlers ask it to
    /// initiate a checkout (`EnsureCampaign`/`CreateCampaign`).
    pub registry: ActorRef<CampaignRegistry>,
    /// Lock-free routing-table snapshot, written only by the registry actor.
    /// Handlers read it directly to resolve a campaign to its `CampaignState`.
    pub table: CampaignTable,
}
