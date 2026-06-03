//! DB assertion for the full-stack e2e smoke test (step 10).
//!
//! After the harness gracefully shuts the campaign server down (so its room
//! actors have flushed their CRDT snapshots to SQLite), it runs this against
//! the campaign's `.db` file to prove the edits actually persisted:
//!
//!   - at least 2 Things exist (the seeded "Campaign Base Camp" home page plus
//!     the "Test page" the spec created), and
//!   - each Thing has at least 2 `section = 'content'` blocks (the spec types
//!     two paragraphs into each).
//!
//! It deliberately reuses the campaign crate's own `db` helpers and sea-orm
//! entities rather than a separate SQLite reader: same driver/WAL semantics the
//! server writes with, and a `things`/`blocks` schema change becomes a compile
//! error here instead of a silently-passing assertion.
//!
//! Usage: `cargo run -p familiar-systems-campaign --example smoke_assert_db -- <path-to.db>`
//! Exits 0 if both invariants hold, non-zero (with a FAIL line) otherwise.

use std::path::PathBuf;
use std::process::exit;

use familiar_systems_campaign::db::{connect_readonly, register_sqlite_vec};
use familiar_systems_campaign::entities::{blocks, things};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

const MIN_THINGS: u64 = 2;
const MIN_CONTENT_BLOCKS_PER_THING: u64 = 2;

#[tokio::main]
async fn main() {
    let path: PathBuf = match std::env::args().nth(1) {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("FAIL: usage: smoke_assert_db <path-to-campaign.db>");
            exit(2);
        }
    };
    if !path.exists() {
        eprintln!("FAIL: campaign db not found at {}", path.display());
        exit(2);
    }

    // vec0 is an auto-extension; register it before opening so connecting to a
    // schema that contains the vec0 virtual table never trips on a missing
    // module (mirrors the server's startup).
    register_sqlite_vec();
    let db = match connect_readonly(&path).await {
        Ok(db) => db,
        Err(e) => {
            eprintln!("FAIL: could not open {}: {e}", path.display());
            exit(2);
        }
    };

    let mut failures: Vec<String> = Vec::new();

    let all_things = things::Entity::find()
        .all(&db)
        .await
        .unwrap_or_else(|e| fatal(&format!("query things failed: {e}")));

    let thing_count = all_things.len() as u64;
    if thing_count < MIN_THINGS {
        failures.push(format!(
            "expected >= {MIN_THINGS} things, found {thing_count}"
        ));
    }

    for t in &all_things {
        let content_blocks = blocks::Entity::find()
            .filter(blocks::Column::ThingId.eq(t.id.clone()))
            .filter(blocks::Column::Section.eq("content"))
            .count(&db)
            .await
            .unwrap_or_else(|e| fatal(&format!("count blocks for {:?} failed: {e}", t.id)));

        println!(
            "thing {:?} ({}): {content_blocks} content blocks",
            t.id, t.name
        );
        if content_blocks < MIN_CONTENT_BLOCKS_PER_THING {
            failures.push(format!(
                "thing {:?} ({}) has {content_blocks} content blocks, expected >= {MIN_CONTENT_BLOCKS_PER_THING}",
                t.id, t.name
            ));
        }
    }

    if failures.is_empty() {
        println!(
            "OK: {thing_count} things, each with >= {MIN_CONTENT_BLOCKS_PER_THING} content blocks"
        );
        exit(0);
    }
    for f in &failures {
        eprintln!("FAIL: {f}");
    }
    exit(1);
}

fn fatal(msg: &str) -> ! {
    eprintln!("FAIL: {msg}");
    exit(2);
}
