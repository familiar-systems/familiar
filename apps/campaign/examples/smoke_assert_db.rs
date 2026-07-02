//! DB assertion for the full-stack e2e smoke test.
//!
//! After the harness gracefully shuts the campaign server down (so its room
//! actors have flushed their CRDT snapshots to SQLite), it runs this against
//! the campaign's `.db` file to prove the edits actually persisted:
//!
//!   - at least 2 Pages exist (the seeded "Campaign Base Camp" home page plus
//!     the second page the spec created), and
//!   - each Page has at least 2 `section = 'body'` blocks (the spec types two
//!     paragraphs into each page's body section), and
//!   - one Page is named "The Sunken Bastion": the spec renames "Test page" via
//!     the in-editor title, so this proves an in-editor rename reached
//!     `pages.name` (the server-authoritative `meta.title` -> `name_sync` flush), and
//!   - exactly one live relationship row exists (neither superseded nor retconned)
//!     carrying the predicate pair the spec created through the modal, proving the
//!     create flow reached SQLite (the RelationshipGraph writes synchronously, so
//!     this needs no flush), and
//!   - that live row reads is_secret = true: the spec creates it public then conceals
//!     it (a knowledge PATCH), proving the mutable knowledge axis persisted, and
//!   - the picked system's template bundle seeded: the spec chooses Daggerheart,
//!     whose bundle must land as `template`-kind pages ("NPC", "Player Character")
//!     nested under a "Templates" ToC folder, with a `<player_visible>` block's
//!     visibility surviving genesis - the wizard-completion seeding path end to end.
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
use familiar_systems_campaign::entities::{blocks, pages, relationships, toc_entries};
use familiar_systems_campaign_shared::id::PageId;
use familiar_systems_campaign_shared::page_kind::PageKind;
use familiar_systems_campaign_shared::status::Status;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

const MIN_PAGES: u64 = 2;
const MIN_BODY_BLOCKS_PER_PAGE: u64 = 2;
/// The spec renames "Test page" to this via the in-editor title; it must persist
/// to `pages.name`.
const RENAMED_PAGE: &str = "The Sunken Bastion";
/// The predicate pair the spec types into the create-relationship modal. The row
/// is stored canonically (page_a = smaller PageId, predicates assigned to match),
/// so assert on the unordered pair, never which slot holds which.
const REL_PREDICATES: [&str; 2] = ["is a resident of", "is the home of"];
/// Daggerheart's template bundle (the system the spec picks). Wizard completion
/// must seed one `template`-kind page per slug, nested under a folder.
const EXPECTED_TEMPLATES: [&str; 2] = ["NPC", "Player Character"];
const TEMPLATES_FOLDER: &str = "Templates";

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
        let body_blocks = blocks::Entity::find()
            .filter(blocks::Column::PageId.eq(t.id.clone()))
            .filter(blocks::Column::Section.eq("body"))
            .count(&db)
            .await
            .unwrap_or_else(|e| fatal(&format!("count blocks for {:?} failed: {e}", t.id)));

        println!("page {:?} ({}): {body_blocks} body blocks", t.id, t.name);
        if body_blocks < MIN_BODY_BLOCKS_PER_PAGE {
            failures.push(format!(
                "page {:?} ({}) has {body_blocks} body blocks, expected >= {MIN_BODY_BLOCKS_PER_PAGE}",
                t.id, t.name
            ));
        }
    }

    let rels = relationships::Entity::find()
        .all(&db)
        .await
        .unwrap_or_else(|e| fatal(&format!("query relationships failed: {e}")));
    // Live = factually current: neither superseded nor retconned. The spec creates one.
    let live: Vec<&relationships::Model> = rels
        .iter()
        .filter(|r| r.superseded_session_id.is_none() && r.retcon_session_id.is_none())
        .collect();
    println!("{} relationships ({} live)", rels.len(), live.len());
    if live.len() != 1 {
        failures.push(format!(
            "expected exactly 1 live relationship, found {}",
            live.len()
        ));
    }
    let has_expected_pair = live.iter().any(|r| {
        let pair = [r.predicate_a_to_b.as_str(), r.predicate_b_to_a.as_str()];
        REL_PREDICATES.iter().all(|p| pair.contains(p))
    });
    if !has_expected_pair {
        let pairs: Vec<(&str, &str)> = live
            .iter()
            .map(|r| (r.predicate_a_to_b.as_str(), r.predicate_b_to_a.as_str()))
            .collect();
        failures.push(format!(
            "expected a live relationship with predicates {REL_PREDICATES:?} (either slot order), found {pairs:?}"
        ));
    }
    // The spec creates the relationship public, then conceals it (a knowledge PATCH).
    // The live row must read is_secret = true, proving the mutable knowledge axis
    // reached SQLite.
    let is_secret = live.iter().any(|r| r.is_secret);
    if !is_secret {
        failures.push("expected the live relationship concealed (is_secret = true)".to_string());
    }

    // --- Template bundle seeding (the spec picks Daggerheart). ---
    let template_pages: Vec<&pages::Model> = all_pages
        .iter()
        .filter(|p| PageKind::from(p.kind) == PageKind::Template)
        .collect();
    let template_names: Vec<&str> = template_pages.iter().map(|p| p.name.as_str()).collect();
    println!(
        "{} template pages: {template_names:?}",
        template_pages.len()
    );
    for expected in EXPECTED_TEMPLATES {
        if !template_names.contains(&expected) {
            failures.push(format!(
                "expected a seeded template page {expected:?} (Daggerheart bundle), found {template_names:?}"
            ));
        }
    }

    // Each seeded template carries its compiled scaffold, and per-block
    // visibility survives genesis: at least one player-visible block from a
    // `<player_visible>` span. A template is never opened in the spec, so its
    // PageActor never flushes -- the genesis status is the persisted status.
    let mut any_player_visible = false;
    for t in &template_pages {
        let template_blocks = blocks::Entity::find()
            .filter(blocks::Column::PageId.eq(t.id.clone()))
            .all(&db)
            .await
            .unwrap_or_else(|e| fatal(&format!("query blocks for {:?} failed: {e}", t.name)));
        if template_blocks.is_empty() {
            failures.push(format!("template page {:?} has no blocks", t.name));
        }
        if template_blocks
            .iter()
            .any(|b| Status::from(b.status) == Status::Known)
        {
            any_player_visible = true;
        }
    }
    if !template_pages.is_empty() && !any_player_visible {
        failures.push(
            "expected >= 1 player-visible template block (a <player_visible> span must persist through genesis)".to_string(),
        );
    }

    // The bundle lands in a "Templates" ToC folder that parents the template pages.
    let toc = toc_entries::Entity::find()
        .all(&db)
        .await
        .unwrap_or_else(|e| fatal(&format!("query toc_entries failed: {e}")));
    match toc
        .iter()
        .find(|r| r.folder_title.as_deref() == Some(TEMPLATES_FOLDER))
    {
        None => failures.push(format!(
            "expected a {TEMPLATES_FOLDER:?} ToC folder for the seeded bundle"
        )),
        Some(folder) => {
            let template_ids: std::collections::HashSet<PageId> = template_pages
                .iter()
                .map(|p| PageId::from(p.id.clone()))
                .collect();
            let nested = toc
                .iter()
                .filter(|r| {
                    r.parent_id.as_ref() == Some(&folder.id)
                        && r.page_id
                            .clone()
                            .map(PageId::from)
                            .is_some_and(|pid| template_ids.contains(&pid))
                })
                .count();
            if nested < EXPECTED_TEMPLATES.len() {
                failures.push(format!(
                    "expected {} template pages nested under {TEMPLATES_FOLDER:?}, found {nested}",
                    EXPECTED_TEMPLATES.len()
                ));
            }
        }
    }

    if failures.is_empty() {
        println!(
            "OK: {page_count} pages, each with >= {MIN_BODY_BLOCKS_PER_PAGE} body blocks, one renamed to {RENAMED_PAGE:?}, one live concealed relationship with {REL_PREDICATES:?}, and the {EXPECTED_TEMPLATES:?} template bundle seeded under {TEMPLATES_FOLDER:?}"
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
