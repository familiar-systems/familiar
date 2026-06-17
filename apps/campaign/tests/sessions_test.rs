//! Sessions schema: the temporal record round-trips through branded types, and
//! the `ordinal` unique constraint is live. No HTTP/actor path yet (the
//! create-session flow and ordinal auto-assignment are later slices); this
//! exercises the migration + entity directly.

use chrono::Utc;
use familiar_systems_campaign::db;
use familiar_systems_campaign::entities::columns::SessionIdCol;
use familiar_systems_campaign::entities::sessions;
use familiar_systems_campaign::migrations::Migrator;
use familiar_systems_campaign_shared::id::SessionId;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use sea_orm_migration::MigratorTrait;

async fn setup() -> DatabaseConnection {
    db::register_sqlite_vec();
    let db = db::connect("sqlite::memory:").await.expect("connect");
    Migrator::up(&db, None).await.expect("migrate");
    db
}

#[tokio::test]
async fn session_round_trips_branded_id_and_ordinal() {
    let db = setup().await;
    let now = Utc::now();

    let id = SessionId::generate();
    sessions::ActiveModel {
        id: Set(id.clone().into()),
        ordinal: Set(1),
        created_at: Set(now),
        updated_at: Set(now),
        page_id: Set(None),
    }
    .insert(&db)
    .await
    .expect("insert session");

    let row = sessions::Entity::find_by_id(SessionIdCol::from(id.clone()))
        .one(&db)
        .await
        .expect("query session")
        .expect("session exists");

    // The let binding is the assertion: the column must come back branded.
    let id_back: SessionId = row.id.into();
    assert_eq!(id_back, id);
    assert_eq!(row.ordinal, 1);
}

#[tokio::test]
async fn duplicate_ordinal_is_rejected() {
    let db = setup().await;
    let now = Utc::now();

    sessions::ActiveModel {
        id: Set(SessionId::generate().into()),
        ordinal: Set(7),
        created_at: Set(now),
        updated_at: Set(now),
        page_id: Set(None),
    }
    .insert(&db)
    .await
    .expect("first session inserts");

    // A different session reusing an ordinal must fail: the curated session
    // number is unique within the campaign.
    let dup = sessions::ActiveModel {
        id: Set(SessionId::generate().into()),
        ordinal: Set(7),
        created_at: Set(now),
        updated_at: Set(now),
        page_id: Set(None),
    }
    .insert(&db)
    .await;

    assert!(
        dup.is_err(),
        "duplicate ordinal should violate the unique constraint"
    );
}

#[tokio::test]
async fn duplicate_page_id_is_rejected() {
    use familiar_systems_campaign::entities::columns::{PageIdCol, PageKindCol, StatusCol};
    use familiar_systems_campaign::entities::pages;
    use familiar_systems_campaign_shared::id::PageId;

    let db = setup().await;
    let now = Utc::now();

    // FK parent: the page both sessions would link to.
    let page_id = PageId::generate();
    pages::ActiveModel {
        id: Set(PageIdCol::from(page_id.clone())),
        name: Set("Untitled Session".into()),
        status: Set(StatusCol::GmOnly),
        kind: Set(PageKindCol::Session),
        template_id: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&db)
    .await
    .expect("seed page");

    sessions::ActiveModel {
        id: Set(SessionId::generate().into()),
        ordinal: Set(1),
        created_at: Set(now),
        updated_at: Set(now),
        page_id: Set(Some(PageIdCol::from(page_id.clone()))),
    }
    .insert(&db)
    .await
    .expect("first session links the page");

    // A second session pointing at the same page must violate the unique link:
    // one session per page, enforced at the DB layer.
    let dup = sessions::ActiveModel {
        id: Set(SessionId::generate().into()),
        ordinal: Set(2),
        created_at: Set(now),
        updated_at: Set(now),
        page_id: Set(Some(PageIdCol::from(page_id))),
    }
    .insert(&db)
    .await;

    assert!(
        dup.is_err(),
        "one session per page: a duplicate page_id must be rejected"
    );
}
