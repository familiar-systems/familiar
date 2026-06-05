//! ProseMirror/Loro convention constants.
//!
//! `loro-prosemirror` maps ProseMirror nodes to Loro containers using these keys.
//! The Rust side (LoroPageDoc) must use the same constants to read and write
//! ProseMirror-compatible LoroDoc trees.
//!
//! If `loro-prosemirror` changes these conventions, the Rust side must change too.
//! The roundtrip integration test catches drift.

/// Top-level key in the LoroDoc for the ProseMirror document root.
pub const ROOT_DOC_KEY: &str = "doc";

/// Key in each node's LoroMap that stores the ProseMirror node type name.
pub const NODE_NAME_KEY: &str = "nodeName";

/// Key in each node's LoroMap for the attributes sub-map.
pub const ATTRIBUTES_KEY: &str = "attributes";

/// Key in each node's LoroMap for the children list.
pub const CHILDREN_KEY: &str = "children";

/// Attribute key carrying a block's stable identity (a ULID). Assigned by the
/// editor's unique-id extension on the client (or server-side at genesis); the
/// server reads it back from a node's `attributes` and uses it as the durable
/// `blocks.id`, so block identity survives the delete+reinsert persist cycle.
/// Must match the `attributeName` configured on the TipTap UniqueID extension
/// in `packages/editor/`.
pub const ATTR_BLOCK_ID: &str = "blockId";

// -- Custom familiar.systems node type names --
// These must match the TipTap node specs in `packages/editor/`.

/// Inline mention node. Attrs: `pageId` (PageId), `label` (display name cache).
pub const NODE_MENTION: &str = "mention";

/// Suggestion mark applied to a block range.
/// Attrs: `suggestionId`, `conversationId`, `proposedContent`.
pub const NODE_SUGGESTION_MARK: &str = "suggestionMark";
