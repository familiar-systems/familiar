//! DB assertion for the full-stack e2e smoke test.
//!
//! After the harness gracefully shuts the campaign server down (so its room
//! actors have flushed their CRDT snapshots to SQLite), it runs this against
//! the campaign's `.db` file to prove the edits actually persisted:
//!
//!   - at least 2 Pages exist (the seeded "Campaign Base Camp" home page plus
//!     the second page the spec created), and
//!   - each Page has at least 2 `section = 'content'` blocks (the spec types
//!     two paragraphs into each), and
//!   - one Page is named "The Sunken Bastion": the spec renames "Test page" via
//!     the in-editor title, so this proves an in-editor rename reached
//!     `pages.name` (the server-authoritative `meta.title` -> `name_sync` flush).
//!
//! It deliberately reuses the campaign crate's own `db` helpers and sea-orm
//! entities rather than a separate SQLite reader: same driver/WAL semantics the
//! server writes with, and a `pages`/`blocks` schema change becomes a compile
//! error here instead of a silently-passing assertion.
//!
//! Usage: `cargo run -p familiar-systems-campaign --example smoke_assert_db -- <path-to.db>`
//! Exits 0 if all invariants hold, non-zero (with a FAIL line) otherwise.

use std::path::PathBuf;
use std::process::exit;

use familiar_systems_campaign::db::{connect_readonly, register_sqlite_vec};
use familiar_systems_campaign::entities::{blocks, pages};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

const MIN_PAGES: u64 = 2;
const MIN_CONTENT_BLOCKS_PER_PAGE: u64 = 2;
/// The spec renames "Test page" to this via the in-editor title; it must persist
/// to `pages.name`.
const RENAMED_PAGE: &str = "The Sunken Bastion";

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

    let all_pages = pages::Entity::find()
        .all(&db)
        .await
        .unwrap_or_else(|e| fatal(&format!("query pages failed: {e}")));

    let page_count = all_pages.len() as u64;
    if page_count < MIN_PAGES {
        failures.push(format!("expected >= {MIN_PAGES} pages, found {page_count}"));
    }

    if !all_pages.iter().any(|p| p.name == RENAMED_PAGE) {
        let names: Vec<&str> = all_pages.iter().map(|p| p.name.as_str()).collect();
        failures.push(format!(
            "expected a page renamed to {RENAMED_PAGE:?} (in-editor title edit must persist to pages.name), found names {names:?}"
        ));
    }

    for t in &all_pages {
        let content_blocks = blocks::Entity::find()
            .filter(blocks::Column::PageId.eq(t.id.clone()))
            .filter(blocks::Column::Section.eq("content"))
            .count(&db)
            .await
            .unwrap_or_else(|e| fatal(&format!("count blocks for {:?} failed: {e}", t.id)));

        println!(
            "page {:?} ({}): {content_blocks} content blocks",
            t.id, t.name
        );
        if content_blocks < MIN_CONTENT_BLOCKS_PER_PAGE {
            failures.push(format!(
                "page {:?} ({}) has {content_blocks} content blocks, expected >= {MIN_CONTENT_BLOCKS_PER_PAGE}",
                t.id, t.name
            ));
        }
    }

    if failures.is_empty() {
        println!(
            "OK: {page_count} pages, each with >= {MIN_CONTENT_BLOCKS_PER_PAGE} content blocks, one renamed to {RENAMED_PAGE:?}"
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
