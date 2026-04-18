use crate::config::Config;
use familiar_systems_app_shared::auth::HankoSessionValidator;
use sea_orm::DatabaseConnection;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub validator: Arc<HankoSessionValidator>,
    pub config: Arc<Config>,
}
