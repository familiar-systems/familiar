//! ProseMirror/Loro convention constants.
//!
//! `loro-prosemirror` maps ProseMirror nodes to Loro containers using these keys.
//! The Rust side (LoroThingDoc) must use the same constants to read and write
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

// -- Custom familiar.systems node type names --
// These must match the TipTap node specs in `packages/editor/`.

/// Inline mention node. Attrs: `thingId` (ThingId), `label` (display name cache).
pub const NODE_MENTION: &str = "mention";

/// Suggestion mark applied to a block range.
/// Attrs: `suggestionId`, `conversationId`, `proposedContent`.
pub const NODE_SUGGESTION_MARK: &str = "suggestionMark";
