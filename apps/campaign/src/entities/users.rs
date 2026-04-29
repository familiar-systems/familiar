use familiar_systems_campaign_shared::id::ThingId;
use sea_orm::DeriveEntityModel;
use sea_orm::DeriveValueType;

#[derive(DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: ThingId,
    pub name: String,
}
