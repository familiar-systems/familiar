//! Wire-type conventions enforced as a `cargo test`.
//!
//! Internally-tagged enums (`#[serde(tag = "type")]` with no `content`)
//! emit a `oneOf` with inline anonymous objects in utoipa 5.x — not
//! `$ref` + `discriminator.mapping` — which downstream `openapi-typescript`
//! and any other codegen consume as a stringly-typed union. Adjacent
//! tagging (`#[serde(tag, content)]`) emits clean `oneOf` references that
//! survive both pipelines. We commit to adjacent tagging now, before any
//! tagged enum lands inside the OpenAPI spec, so we never have to retrofit.
//!
//! Scope: only types that derive `ToSchema` (i.e. types utoipa actually
//! sees). Wire types that go through ts-rs / Loro alone keep their existing
//! shape — bringing them under utoipa later means choosing the tagging
//! style at that time.
//!
//! Drop this test once `juhaku/utoipa#1456` (and its corresponding fix in
//! the discriminator-renderer) ships and we've validated our spec round-
//! trips through openapi-typescript without regressions.

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/
    p.pop(); // workspace root
    p
}

fn collect_rs(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// One attribute cluster + the type definition it adorns.
struct TypeBlock {
    attributes: String,
    line: usize,
}

/// Group every attribute cluster with the `struct`/`enum` it adorns.
///
/// Multi-line attributes are collected by tracking parenthesis and bracket
/// depth, so a `#[serde(\n    tag = "x",\n    content = "y",\n)]` is one
/// logical unit even though it spans four lines.
fn parse_blocks(src: &str) -> Vec<TypeBlock> {
    let mut blocks = Vec::new();
    let mut attrs = String::new();
    let mut started_at: Option<usize> = None;
    let mut paren = 0i32;
    let mut bracket = 0i32;

    for (lineno, line) in src.lines().enumerate() {
        // Continuation of a multi-line attribute that opened on a prior line.
        if paren > 0 || bracket > 0 {
            attrs.push_str(line);
            attrs.push('\n');
            paren += line.matches('(').count() as i32 - line.matches(')').count() as i32;
            bracket += line.matches('[').count() as i32 - line.matches(']').count() as i32;
            continue;
        }

        let trimmed = line.trim_start();
        if trimmed.starts_with("#[") {
            if started_at.is_none() {
                started_at = Some(lineno + 1);
            }
            attrs.push_str(line);
            attrs.push('\n');
            paren = line.matches('(').count() as i32 - line.matches(')').count() as i32;
            bracket = line.matches('[').count() as i32 - line.matches(']').count() as i32;
            continue;
        }

        if trimmed.starts_with("//") || trimmed.is_empty() {
            // Doc comment or blank line between attributes — keep accumulating.
            continue;
        }

        let is_type_def = ["pub struct ", "struct ", "pub enum ", "enum "]
            .iter()
            .any(|kw| trimmed.starts_with(kw))
            || trimmed.starts_with("pub(crate) struct ")
            || trimmed.starts_with("pub(crate) enum ");

        if is_type_def && started_at.is_some() {
            blocks.push(TypeBlock {
                attributes: attrs.clone(),
                line: started_at.unwrap(),
            });
        }

        attrs.clear();
        started_at = None;
    }

    blocks
}

#[test]
fn to_schema_types_use_adjacent_tagging_only() {
    let ws = workspace_root();
    let mut files = Vec::new();
    for crate_src in [
        "crates/app-shared/src",
        "crates/campaign-shared/src",
        "apps/platform/src",
        "apps/campaign/src",
    ] {
        collect_rs(&ws.join(crate_src), &mut files);
    }

    let mut violations = Vec::new();
    for path in &files {
        let src = fs::read_to_string(path).unwrap_or_default();
        for block in parse_blocks(&src) {
            // The rule only governs types that flow through utoipa. Types
            // visible only to ts-rs / Loro pick their own tagging style.
            if !block.attributes.contains("ToSchema") {
                continue;
            }
            let has_tag = block.attributes.contains("tag =") || block.attributes.contains("tag=");
            let has_content =
                block.attributes.contains("content =") || block.attributes.contains("content=");
            let has_untagged = block.attributes.contains("untagged");
            if has_tag && !has_content && !has_untagged {
                let rel = path.strip_prefix(&ws).unwrap_or(path);
                violations.push(format!("{}:{}", rel.display(), block.line));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Internally-tagged enums break utoipa's discriminator emission. \
         Use `#[serde(tag = \"...\", content = \"...\")]` (adjacent tagging) \
         instead. Violations:\n  {}",
        violations.join("\n  ")
    );
}
