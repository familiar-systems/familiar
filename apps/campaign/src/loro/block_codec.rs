//! Opaque round-trip between Loro containers and JSON blobs.
//!
//! Each block in a Thing's content section is a top-level child of the
//! ProseMirror document's children LoroList. This module serializes each
//! block's Loro sub-tree to JSON (via `get_deep_value`) and reconstructs
//! it from that JSON on restore.
//!
//! ## Known limitation
//!
//! `get_deep_value` collapses `LoroText` to a plain string, dropping rich
//! text marks (bold, italic, etc.). A future session will replace this with
//! a mark-preserving codec that uses `LoroText::to_delta`.

use familiar_systems_campaign_shared::id::BlockId;
use familiar_systems_campaign_shared::loro::prosemirror::{
    ATTR_BLOCK_ID, ATTRIBUTES_KEY, CHILDREN_KEY, NODE_NAME_KEY,
};
use fs_id::Ulid;
use loro::{LoroList, LoroMap, LoroText, LoroValue, ValueOrContainer};

// ── Persist: Loro -> JSON blob ──────────────────────────────────────────────

/// Serialize a block's Loro sub-tree to an opaque JSON blob.
pub fn serialize_block(block_map: &LoroMap) -> Vec<u8> {
    let value: serde_json::Value = block_map.get_deep_value().into();
    serde_json::to_vec(&value).expect("LoroValue is always serializable")
}

// ── Seed: build a starter block ─────────────────────────────────────────────

/// Build the content blob for a seeded empty paragraph carrying a stable
/// `blockId`. A brand-new Thing seeds one of these so it opens as a
/// schema-valid, editable ProseMirror document (a `doc` with one `block+`
/// child) rather than an empty, uneditable `doc`. Embedding the block's ULID
/// in `attributes.blockId` keeps its identity stable from genesis through
/// edit and reload (see [`extract_blocks`]).
pub fn empty_paragraph_blob(block_id: &BlockId) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        NODE_NAME_KEY: "paragraph",
        ATTRIBUTES_KEY: { ATTR_BLOCK_ID: block_id.to_string() },
        CHILDREN_KEY: [],
    }))
    .expect("paragraph block is always serializable")
}

// ── Restore: JSON blob -> Loro ──────────────────────────────────────────────

/// Reconstruct a ProseMirror node from a JSON blob into a Loro children list.
///
/// The blob must be an object with `nodeName`, `attributes`, and `children`
/// keys (the format produced by `serialize_block`).
pub fn restore_block(parent_list: &LoroList, pos: usize, blob: &[u8]) {
    let json: serde_json::Value =
        serde_json::from_slice(blob).expect("block content must be valid JSON");
    restore_node(parent_list, pos, &json);
}

/// Recursively create a Loro node from a JSON value.
///
/// Convention: in a `children` array, a JSON string is a `LoroText`,
/// an object with `nodeName` is a nested `LoroMap`. This matches the
/// output of `get_deep_value` on a loro-prosemirror document.
fn restore_node(parent_list: &LoroList, pos: usize, json: &serde_json::Value) {
    let obj = json
        .as_object()
        .expect("ProseMirror node must be a JSON object");

    let node = parent_list
        .insert_container(pos, LoroMap::new())
        .expect("insert block node");

    if let Some(name) = obj.get(NODE_NAME_KEY).and_then(|v| v.as_str()) {
        node.insert(NODE_NAME_KEY, name).unwrap();
    }

    if let Some(attrs) = obj.get(ATTRIBUTES_KEY).and_then(|v| v.as_object()) {
        let attrs_map = node
            .insert_container(ATTRIBUTES_KEY, LoroMap::new())
            .unwrap();
        for (k, v) in attrs {
            attrs_map.insert(k.as_str(), json_to_loro_value(v)).unwrap();
        }
    }

    if let Some(children) = obj.get(CHILDREN_KEY).and_then(|v| v.as_array()) {
        let children_list = node
            .insert_container(CHILDREN_KEY, LoroList::new())
            .unwrap();
        restore_children(&children_list, children);
    }
}

/// Populate a children LoroList from a JSON array.
fn restore_children(list: &LoroList, children: &[serde_json::Value]) {
    for (i, child) in children.iter().enumerate() {
        if let Some(s) = child.as_str() {
            let text = list.insert_container(i, LoroText::new()).unwrap();
            text.insert(0, s).unwrap();
        } else if child.is_object() {
            restore_node(list, i, child);
        }
    }
}

/// Convert a JSON value to a LoroValue for map insertion.
fn json_to_loro_value(v: &serde_json::Value) -> LoroValue {
    match v {
        serde_json::Value::Null => LoroValue::Null,
        serde_json::Value::Bool(b) => LoroValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                LoroValue::I64(i)
            } else {
                LoroValue::Double(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => LoroValue::String(s.as_str().into()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            // Nested arrays/objects in attributes are rare in ProseMirror.
            // Store as a JSON string for now; a future codec can handle this.
            LoroValue::String(v.to_string().into())
        }
    }
}

// ── Extract: walk content and produce block descriptors ─────────────────────

/// A block extracted from the LoroDoc content section, ready for persistence.
pub struct ExtractedBlock {
    /// Stable identity parsed from the node's `attributes.blockId` (a ULID
    /// assigned by the editor's unique-id extension, or server-side at
    /// genesis). `None` when the attribute is absent or malformed; the persist
    /// path then mints a fresh id as a fallback (and the id will churn).
    pub id: Option<BlockId>,
    pub content: Vec<u8>,
    pub ordering: i64,
}

/// Walk the content section's children and serialize each top-level node.
pub fn extract_blocks(content_map: &LoroMap) -> Vec<ExtractedBlock> {
    let children = match content_map.get(CHILDREN_KEY) {
        Some(ValueOrContainer::Container(loro::Container::List(list))) => list,
        _ => return Vec::new(),
    };

    let mut blocks = Vec::new();
    let len = children.len();
    for i in 0..len {
        if let Some(ValueOrContainer::Container(loro::Container::Map(block_map))) = children.get(i)
        {
            blocks.push(ExtractedBlock {
                id: read_block_id(&block_map),
                content: serialize_block(&block_map),
                ordering: i as i64,
            });
        }
    }
    blocks
}

/// Read a block node's stable `blockId` (a ULID) from its `attributes` map.
/// Returns `None` if the attribute is missing, the wrong type, or not a valid
/// ULID -- callers treat that as "mint a fresh id".
fn read_block_id(block_map: &LoroMap) -> Option<BlockId> {
    let attrs = match block_map.get(ATTRIBUTES_KEY)? {
        ValueOrContainer::Container(loro::Container::Map(m)) => m,
        _ => return None,
    };
    let raw = match attrs.get(ATTR_BLOCK_ID)? {
        ValueOrContainer::Value(LoroValue::String(s)) => s.to_string(),
        _ => return None,
    };
    Ulid::from_string(&raw).ok().map(BlockId::from)
}

/// Initialize a content LoroMap as a ProseMirror document root and
/// populate it with blocks restored from their JSON blobs.
///
/// The blobs must be in the correct order (by `ordering`).
pub fn restore_content(content_map: &LoroMap, blobs: &[Vec<u8>]) {
    content_map.insert(NODE_NAME_KEY, "doc").unwrap();
    content_map
        .insert_container(ATTRIBUTES_KEY, LoroMap::new())
        .unwrap();
    let children = content_map
        .insert_container(CHILDREN_KEY, LoroList::new())
        .unwrap();
    for (i, blob) in blobs.iter().enumerate() {
        restore_block(&children, i, blob);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loro::LoroDoc;
    use serde_json::json;

    fn setup_doc_with_heading() -> (LoroDoc, LoroMap) {
        let doc = LoroDoc::new();
        let content = doc.get_map("content");
        content.insert(NODE_NAME_KEY, "doc").unwrap();
        content
            .insert_container(ATTRIBUTES_KEY, LoroMap::new())
            .unwrap();
        let children = content
            .insert_container(CHILDREN_KEY, LoroList::new())
            .unwrap();

        let heading = children.insert_container(0, LoroMap::new()).unwrap();
        heading.insert(NODE_NAME_KEY, "heading").unwrap();
        let attrs = heading
            .insert_container(ATTRIBUTES_KEY, LoroMap::new())
            .unwrap();
        attrs.insert("level", 1i64).unwrap();
        let h_children = heading
            .insert_container(CHILDREN_KEY, LoroList::new())
            .unwrap();
        let text = h_children.insert_container(0, LoroText::new()).unwrap();
        text.insert(0, "Hello World").unwrap();

        (doc, content)
    }

    #[test]
    fn heading_round_trip() {
        let (_doc, content) = setup_doc_with_heading();
        let blocks = extract_blocks(&content);
        assert_eq!(blocks.len(), 1);

        let doc2 = LoroDoc::new();
        let content2 = doc2.get_map("content");
        restore_content(&content2, &[blocks[0].content.clone()]);

        assert_eq!(
            content.get_deep_value(),
            content2.get_deep_value(),
            "content should survive round-trip"
        );
    }

    #[test]
    fn paragraph_round_trip() {
        let doc = LoroDoc::new();
        let content = doc.get_map("content");
        content.insert(NODE_NAME_KEY, "doc").unwrap();
        content
            .insert_container(ATTRIBUTES_KEY, LoroMap::new())
            .unwrap();
        let children = content
            .insert_container(CHILDREN_KEY, LoroList::new())
            .unwrap();

        let para = children.insert_container(0, LoroMap::new()).unwrap();
        para.insert(NODE_NAME_KEY, "paragraph").unwrap();
        para.insert_container(ATTRIBUTES_KEY, LoroMap::new())
            .unwrap();
        let p_children = para
            .insert_container(CHILDREN_KEY, LoroList::new())
            .unwrap();
        let text = p_children.insert_container(0, LoroText::new()).unwrap();
        text.insert(0, "Some body text here.").unwrap();

        let blocks = extract_blocks(&content);
        assert_eq!(blocks.len(), 1);

        let doc2 = LoroDoc::new();
        let content2 = doc2.get_map("content");
        restore_content(&content2, &[blocks[0].content.clone()]);

        assert_eq!(content.get_deep_value(), content2.get_deep_value());
    }

    #[test]
    fn multiple_blocks_round_trip() {
        let (_doc, content) = setup_doc_with_heading();

        // Add a second block (paragraph)
        let children = match content.get(CHILDREN_KEY) {
            Some(ValueOrContainer::Container(loro::Container::List(l))) => l,
            _ => panic!("children not found"),
        };
        let para = children.insert_container(1, LoroMap::new()).unwrap();
        para.insert(NODE_NAME_KEY, "paragraph").unwrap();
        para.insert_container(ATTRIBUTES_KEY, LoroMap::new())
            .unwrap();
        let p_children = para
            .insert_container(CHILDREN_KEY, LoroList::new())
            .unwrap();
        let text = p_children.insert_container(0, LoroText::new()).unwrap();
        text.insert(0, "A paragraph after the heading.").unwrap();

        let blocks = extract_blocks(&content);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].ordering, 0);
        assert_eq!(blocks[1].ordering, 1);

        let doc2 = LoroDoc::new();
        let content2 = doc2.get_map("content");
        let blobs: Vec<Vec<u8>> = blocks.into_iter().map(|b| b.content).collect();
        restore_content(&content2, &blobs);

        assert_eq!(content.get_deep_value(), content2.get_deep_value());
    }

    #[test]
    fn empty_content_round_trip() {
        let doc = LoroDoc::new();
        let content = doc.get_map("content");
        content.insert(NODE_NAME_KEY, "doc").unwrap();
        content
            .insert_container(ATTRIBUTES_KEY, LoroMap::new())
            .unwrap();
        content
            .insert_container(CHILDREN_KEY, LoroList::new())
            .unwrap();

        let blocks = extract_blocks(&content);
        assert!(blocks.is_empty());

        let doc2 = LoroDoc::new();
        let content2 = doc2.get_map("content");
        restore_content(&content2, &[]);

        assert_eq!(content.get_deep_value(), content2.get_deep_value());
    }

    #[test]
    fn attributes_preserved() {
        let blob = serde_json::to_vec(&json!({
            NODE_NAME_KEY: "heading",
            ATTRIBUTES_KEY: {
                "level": 2,
                "blockId": "01ABC",
                "collapsed": true,
            },
            CHILDREN_KEY: ["Test heading"]
        }))
        .unwrap();

        let doc = LoroDoc::new();
        let content = doc.get_map("content");
        restore_content(&content, &[blob]);

        let deep: serde_json::Value = content.get_deep_value().into();
        let block = &deep[CHILDREN_KEY][0];
        assert_eq!(block[ATTRIBUTES_KEY]["level"], 2);
        assert_eq!(block[ATTRIBUTES_KEY]["blockId"], "01ABC");
        assert_eq!(block[ATTRIBUTES_KEY]["collapsed"], true);
    }

    #[test]
    fn seed_blob_extracts_with_stable_block_id() {
        // The genesis seed embeds the block's ULID in `attributes.blockId`;
        // `extract_blocks` must recover that exact id (not mint a new one).
        let block_id = BlockId::generate();
        let doc = LoroDoc::new();
        let content = doc.get_map("content");
        restore_content(&content, &[empty_paragraph_blob(&block_id)]);

        let blocks = extract_blocks(&content);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].id, Some(block_id));
        assert_eq!(blocks[0].ordering, 0);
    }

    #[test]
    fn extract_block_id_is_none_when_attribute_absent() {
        // A heading carries only `level`; with no `blockId` the persist path
        // falls back to minting a fresh id.
        let (_doc, content) = setup_doc_with_heading();
        let blocks = extract_blocks(&content);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].id, None);
    }
}
