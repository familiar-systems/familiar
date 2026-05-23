//! Thing CRUD wire types (FE-visible).

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utoipa::ToSchema;

use crate::id::ThingId;

#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/things/")]
pub struct CreateThingRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/things/")]
pub struct CreateThingResponse {
    pub id: ThingId,
    pub name: String,
}
