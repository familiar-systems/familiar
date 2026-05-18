use crate::{clients::campaign_internal::CampaignInternalClient, config::Config};
use familiar_systems_app_shared::auth::HankoSessionValidator;
use sea_orm::DatabaseConnection;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    // `DatabaseConnection` is internally `Arc<...>` per sea-orm, so cloning is
    // cheap and sharing across handlers is free. Left unwrapped to avoid a
    // redundant outer Arc; the Arc on `validator` and `config` exists because
    // those types are not internally shared.
    pub db: DatabaseConnection,
    pub validator: Arc<HankoSessionValidator>,
    pub config: Arc<Config>,
    /// Client for `POST /internal/campaign/init` and (later) the metadata
    /// mirror endpoint. Holds an internal `Arc` already; cloning is cheap.
    pub campaign_internal: CampaignInternalClient,
}
