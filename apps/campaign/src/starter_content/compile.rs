//! Template markdown -> block rows for page genesis.
//!
//! A one-way parse (see `docs/plans/2026-06-30-ai-serialization-and-editing-model.md`,
//! "Template import"): markdown in, freshly-minted block rows out, with no prior
//! document to reconcile against. This is *not* the agent write path (`f⁻¹`),
//! which is a stateful diff; the two share only a parser direction.
//!
//! v0 grammar (see `docs/plans/2026-06-29-templates.md`):
//! - Visibility spans `<player_visible>` / `<gm_only>` run-length-encode per-block
//!   status; the fail-closed default outside any span is `gm_only`.
//! - Blocks are ATX headings (`#`..`###`, levels 1-3) and plain paragraphs; rich
//!   marks are dropped (the block codec is plain-text today).
//! - Sections are positional: prose before the first heading is the `preamble`
//!   index card; the first heading onward is the freeform `body`.

use familiar_systems_campaign_shared::id::BlockId;
use familiar_systems_campaign_shared::loro::page::Section;
use familiar_systems_campaign_shared::loro::prosemirror::{
    ATTR_BLOCK_ID, ATTRIBUTES_KEY, CHILDREN_KEY, NODE_NAME_KEY,
};
use familiar_systems_campaign_shared::status::Status;

use crate::domain::page::NewBlock;
use crate::starter_content::template::split_frontmatter;

/// Visibility span tags. Total labeling is the template convention, so every
/// block sits inside one; anything outside a span keeps the fail-closed default.
const TAG_PLAYER_OPEN: &str = "<player_visible>";
const TAG_PLAYER_CLOSE: &str = "</player_visible>";
const TAG_GM_OPEN: &str = "<gm_only>";
const TAG_GM_CLOSE: &str = "</gm_only>";

/// The largest heading level the ProseMirror schema allows (`h1`..`h3`).
const MAX_HEADING_LEVEL: usize = 3;

/// A template compiled to the values page genesis persists: its display name and
/// its block rows (each carrying its section, per-block visibility, and the
/// block-codec blob with a freshly minted, stable `blockId`).
#[derive(Debug)]
pub struct CompiledTemplate {
    pub name: String,
    pub blocks: Vec<NewBlock>,
}

/// Compile one locale's template markdown into genesis block rows.
///
/// Errors only on a malformed frontmatter fence (the body grammar is total:
/// any line is a heading, a paragraph line, a span tag, or blank).
pub fn compile_template(markdown: &str) -> Result<CompiledTemplate, String> {
    let (frontmatter, body) = split_frontmatter(markdown)?;
    let parsed = parse_body(body);

    // Positional section split: everything up to the first heading is the
    // preamble index card; the first heading onward is the body.
    let first_heading = parsed
        .iter()
        .position(|b| matches!(b.node, Node::Heading { .. }));

    let mut preamble_ordering = 0i64;
    let mut body_ordering = 0i64;
    let blocks = parsed
        .into_iter()
        .enumerate()
        .map(|(i, b)| {
            let section = match first_heading {
                Some(h) if i >= h => Section::Body,
                _ => Section::Preamble,
            };
            let ordering = match section {
                Section::Preamble => &mut preamble_ordering,
                _ => &mut body_ordering,
            };
            let id = BlockId::generate();
            let block = NewBlock {
                content: b.node.to_blob(&id),
                id,
                section,
                ordering: *ordering,
                status: b.status,
            };
            *ordering += 1;
            block
        })
        .collect();

    Ok(CompiledTemplate {
        name: frontmatter.name,
        blocks,
    })
}

/// A parsed body block plus the visibility it inherits from its enclosing span.
struct ParsedBlock {
    status: Status,
    node: Node,
}

enum Node {
    Heading { level: usize, text: String },
    Paragraph { text: String },
}

impl Node {
    /// Serialize to the block-codec blob shape
    /// (`{ nodeName, attributes, children }`), embedding the stable `blockId`.
    fn to_blob(&self, id: &BlockId) -> Vec<u8> {
        let json = match self {
            Node::Heading { level, text } => serde_json::json!({
                NODE_NAME_KEY: "heading",
                ATTRIBUTES_KEY: { ATTR_BLOCK_ID: id.to_string(), "level": level },
                CHILDREN_KEY: [text],
            }),
            Node::Paragraph { text } => serde_json::json!({
                NODE_NAME_KEY: "paragraph",
                ATTRIBUTES_KEY: { ATTR_BLOCK_ID: id.to_string() },
                CHILDREN_KEY: [text],
            }),
        };
        serde_json::to_vec(&json).expect("template block json is always serializable")
    }
}

/// Walk the body line by line, tracking the current span's visibility and
/// coalescing consecutive prose lines into paragraphs. Span tags and headings
/// each flush any open paragraph first, so a paragraph never straddles a status
/// change or a heading boundary.
fn parse_body(body: &str) -> Vec<ParsedBlock> {
    let mut status = Status::GmOnly; // fail-closed default outside any span
    let mut blocks: Vec<ParsedBlock> = Vec::new();
    let mut paragraph = String::new();

    for raw_line in body.lines() {
        let line = raw_line.trim();
        match line {
            TAG_PLAYER_OPEN => {
                flush_paragraph(&mut blocks, &mut paragraph, status);
                status = Status::Known;
            }
            TAG_GM_OPEN => {
                flush_paragraph(&mut blocks, &mut paragraph, status);
                status = Status::GmOnly;
            }
            TAG_PLAYER_CLOSE | TAG_GM_CLOSE => {
                flush_paragraph(&mut blocks, &mut paragraph, status);
                status = Status::GmOnly;
            }
            "" => flush_paragraph(&mut blocks, &mut paragraph, status),
            _ => {
                if let Some((level, text)) = parse_heading(line) {
                    flush_paragraph(&mut blocks, &mut paragraph, status);
                    blocks.push(ParsedBlock {
                        status,
                        node: Node::Heading {
                            level,
                            text: text.to_string(),
                        },
                    });
                } else {
                    if !paragraph.is_empty() {
                        paragraph.push(' ');
                    }
                    paragraph.push_str(line);
                }
            }
        }
    }
    flush_paragraph(&mut blocks, &mut paragraph, status);
    blocks
}

/// Emit the accumulated paragraph (if any) as a block and reset the buffer.
fn flush_paragraph(blocks: &mut Vec<ParsedBlock>, paragraph: &mut String, status: Status) {
    let text = paragraph.trim();
    if !text.is_empty() {
        blocks.push(ParsedBlock {
            status,
            node: Node::Paragraph {
                text: text.to_string(),
            },
        });
    }
    paragraph.clear();
}

/// An ATX heading is 1-6 leading `#` followed by a space; the level is clamped
/// to the schema's `h1`..`h3`. A `#` with no following space (a hashtag like
/// `#NPC`) is not a heading - it stays paragraph text.
fn parse_heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    if hashes == 0 {
        return None;
    }
    let text = line[hashes..].strip_prefix(' ')?.trim();
    Some((hashes.min(MAX_HEADING_LEVEL), text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::starter_content::catalog::Catalog;

    fn blob_json(block: &NewBlock) -> serde_json::Value {
        serde_json::from_slice(&block.content).unwrap()
    }

    #[test]
    fn splits_preamble_at_first_heading_with_per_block_status() {
        let md = "\
---
name: NPC
description: d
icon: contact
---

<player_visible>
Who is this person?
</player_visible>

<gm_only>
A secret.
</gm_only>

<player_visible>
# Appearance

What do they look like?
</player_visible>
";
        let compiled = compile_template(md).unwrap();
        assert_eq!(compiled.name, "NPC");

        // Preamble: the two prose blocks before `# Appearance`, each keeping its
        // own span's visibility.
        let preamble: Vec<_> = compiled
            .blocks
            .iter()
            .filter(|b| b.section == Section::Preamble)
            .collect();
        assert_eq!(preamble.len(), 2);
        assert_eq!(preamble[0].status, Status::Known);
        assert_eq!(preamble[0].ordering, 0);
        assert_eq!(preamble[1].status, Status::GmOnly);
        assert_eq!(preamble[1].ordering, 1);

        // Body: heading + paragraph, ordering restarts at 0 within the section.
        let body: Vec<_> = compiled
            .blocks
            .iter()
            .filter(|b| b.section == Section::Body)
            .collect();
        assert_eq!(body.len(), 2);
        assert_eq!(body[0].ordering, 0);
        assert_eq!(blob_json(body[0])[NODE_NAME_KEY], "heading");
        assert_eq!(blob_json(body[0])[ATTRIBUTES_KEY]["level"], 1);
        assert_eq!(blob_json(body[0])[CHILDREN_KEY][0], "Appearance");
        assert_eq!(body[0].status, Status::Known);
        assert_eq!(blob_json(body[1])[NODE_NAME_KEY], "paragraph");
        assert_eq!(
            blob_json(body[1])[CHILDREN_KEY][0],
            "What do they look like?"
        );
    }

    #[test]
    fn blocks_carry_a_stable_block_id() {
        let md = "---\nname: N\ndescription: d\nicon: i\n---\n\n<gm_only>\nHello.\n</gm_only>\n";
        let compiled = compile_template(md).unwrap();
        let block = &compiled.blocks[0];
        // The persisted id matches the blob's `attributes.blockId`.
        assert_eq!(
            blob_json(block)[ATTRIBUTES_KEY][ATTR_BLOCK_ID],
            block.id.to_string()
        );
    }

    #[test]
    fn heading_level_clamps_and_hashtag_is_not_a_heading() {
        let md = "\
---
name: N
description: d
icon: i
---

<gm_only>
##### Deep

#NotAHeading is a hashtag.
</gm_only>
";
        let compiled = compile_template(md).unwrap();
        // `#####` clamps to h3; `#NotAHeading` (no space) stays paragraph text.
        assert_eq!(blob_json(&compiled.blocks[0])[NODE_NAME_KEY], "heading");
        assert_eq!(blob_json(&compiled.blocks[0])[ATTRIBUTES_KEY]["level"], 3);
        assert_eq!(blob_json(&compiled.blocks[1])[NODE_NAME_KEY], "paragraph");
        assert_eq!(
            blob_json(&compiled.blocks[1])[CHILDREN_KEY][0],
            "#NotAHeading is a hashtag."
        );
    }

    #[test]
    fn every_bundled_template_compiles() {
        // Guards that real content parses: a malformed body would otherwise only
        // surface as a silently-skipped seed at campaign creation.
        let raw = Catalog::load_from_embedded().unwrap();
        for slug in [
            "common/npc",
            "common/player-character",
            "blades-in-the-dark/crew",
        ] {
            let md = raw
                .template_markdown(slug, "en")
                .unwrap_or_else(|| panic!("{slug} markdown present"));
            let compiled =
                compile_template(md).unwrap_or_else(|e| panic!("{slug} should compile: {e}"));
            assert!(
                compiled.blocks.iter().any(|b| b.section == Section::Body),
                "{slug} should produce body blocks"
            );
        }
    }
}
