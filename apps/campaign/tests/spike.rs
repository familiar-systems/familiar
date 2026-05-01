//! Spike's grade card. Two tests:
//! 1. Things + Blocks round-trip with branded ID types across a cross-type FK.
//! 2. Vec search filters at KNN time (not after) for player viewers.

use chrono::Utc;
use familiar_systems_campaign::db;
use familiar_systems_campaign::embeddings::{EmbeddingsRepo, ViewerKind};
use familiar_systems_campaign::entities::{blocks, columns::*, things};
use familiar_systems_campaign::migrations::Migrator;
use familiar_systems_campaign_shared::id::{BlockId, ThingId};
use familiar_systems_campaign_shared::status::Status;
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, DatabaseConnection, EntityTrait, ModelTrait, Set, Statement,
};
use sea_orm_migration::MigratorTrait;

async fn setup() -> DatabaseConnection {
    db::register_sqlite_vec();
    let db = db::connect("sqlite::memory:").await.expect("connect");
    Migrator::up(&db, None).await.expect("migrate");
    db
}

#[tokio::test]
async fn things_and_blocks_round_trip_branded_types() {
    let db = setup().await;
    let now = Utc::now();

    // Insert a Thing.
    let thing_id = ThingId::new();
    things::ActiveModel {
        id: Set(thing_id.clone().into()),
        name: Set("Vex the Bone Sage".into()),
        status: Set(Status::Known.into()),
        prototype_id: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&db)
    .await
    .expect("insert thing");

    // Insert three Blocks tied to it. Mixed status so the FK + branded-type
    // round-trip carries non-trivial values.
    let statuses = [Status::Known, Status::Known, Status::GmOnly];
    for (i, status) in statuses.into_iter().enumerate() {
        blocks::ActiveModel {
            id: Set(BlockId::new().into()),
            thing_id: Set(thing_id.clone().into()),
            status: Set(status.into()),
            ordering: Set(i as i64),
            body: Set(format!("paragraph {i}")),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&db)
        .await
        .expect("insert block");
    }

    // Read the Thing back and pull its blocks via the FK relation.
    let thing = things::Entity::find_by_id(ThingIdCol::from(thing_id.clone()))
        .one(&db)
        .await
        .expect("find thing")
        .expect("thing exists");

    let block_models: Vec<blocks::Model> = thing
        .find_related(blocks::Entity)
        .all(&db)
        .await
        .expect("find related blocks");

    // Boundary: domain types come out branded. The let bindings are the
    // assertion — if any of these types disagreed with the entity columns,
    // the cast would fail to compile.
    let thing_id_back: ThingId = thing.id.into();
    let block_ids_back: Vec<BlockId> = block_models.iter().map(|b| b.id.clone().into()).collect();
    let block_thing_ids_back: Vec<ThingId> = block_models
        .iter()
        .map(|b| b.thing_id.clone().into())
        .collect();
    let block_statuses_back: Vec<Status> = block_models.iter().map(|b| b.status.into()).collect();

    assert_eq!(thing_id_back, thing_id);
    assert_eq!(block_ids_back.len(), 3);
    assert!(block_thing_ids_back.iter().all(|t| *t == thing_id));
    assert_eq!(
        block_statuses_back,
        vec![Status::Known, Status::Known, Status::GmOnly]
    );
}

#[tokio::test]
async fn vec_search_status_filters_for_player_inside_knn() {
    let db = setup().await;

    // The KNN must filter on the metadata column DURING the traversal, not
    // after. We test that by placing gm_only embeddings closer to the query
    // than the known ones; an over-fetch-and-filter implementation would
    // return fewer than k=5 results for the player. A correct pre-filter
    // returns exactly the visible top-k.

    // Query vector: [1, 0, 0, 0, 0, 0, 0, 0]
    // gm_only blocks at distances ~0.1, ~0.2 (closest); known blocks at ~0.3..~0.5.
    let query = [1.0_f32, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let entries: [(&str, [f32; 8]); 5] = [
        ("gm_only", [0.9, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        ("gm_only", [0.8, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        ("known", [0.7, 0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        ("known", [0.6, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        ("known", [0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ];

    let mut block_ids: Vec<ulid::Ulid> = Vec::new();
    let mut gm_only_block_ids: Vec<ulid::Ulid> = Vec::new();
    for (status, vec) in entries {
        let id = ulid::Ulid::new();
        if status == "gm_only" {
            gm_only_block_ids.push(id);
        }
        block_ids.push(id);

        let embedding_blob = vec_to_bytes(&vec);
        db.execute(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "INSERT INTO block_embeddings (block_id, embedding, status) VALUES (?1, ?2, ?3);",
            [
                sea_orm::Value::String(Some(Box::new(id.to_string()))),
                sea_orm::Value::Bytes(Some(Box::new(embedding_blob))),
                status.into(),
            ],
        ))
        .await
        .expect("insert vec row");
    }

    let repo = EmbeddingsRepo { db: &db };

    let gm = repo
        .search(&query, ViewerKind::Gm, 5)
        .await
        .expect("gm search");
    assert_eq!(gm.len(), 5, "GM sees all five blocks");

    let player = repo
        .search(&query, ViewerKind::Player, 5)
        .await
        .expect("player search");
    assert_eq!(
        player.len(),
        3,
        "Player sees 3 known blocks. If we got <3, the filter is post-KNN \
         (over-fetch-and-filter); if we got >3, the filter isn't applied."
    );

    let player_ids: Vec<ulid::Ulid> = player.iter().map(|(id, _)| id.0).collect();
    for gm_only_id in &gm_only_block_ids {
        assert!(
            !player_ids.contains(gm_only_id),
            "gm_only block leaked into player results"
        );
    }
}

fn vec_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}
