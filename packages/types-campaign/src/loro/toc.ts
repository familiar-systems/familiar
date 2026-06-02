//! Table-of-Contents Loro schema: the container name, node-metadata keys, and
//! kind values the client reads/writes on the `"toc"` LoroTree.
//!
//! These mirror the Rust constants in
//! `crates/campaign-shared/src/loro/toc.rs` (`CONTAINER_TOC`, `KEY_*`,
//! `KIND_*`, `MAX_DEPTH`). The `TocEntry`/`TocEntryKind` *types* are ts-rs
//! generated; these *string constants* are not, so they are hand-mirrored here
//! the same way `packages/editor/src/meta.ts` mirrors the Thing meta keys.
//!
//! FIXME: hand-mirroring is a drift hazard. These constants are the wire schema
//! shared with the server and MUST be generated/exported from the Rust source
//! of truth (`crates/campaign-shared/src/loro/toc.rs`) rather than retyped here.
//! ts-rs exports types but not `const &str` values; wiring a codegen step (or a
//! ts-rs-exported "constants" struct) is the real fix. Until then, any change to
//! the Rust constants must be made here by hand.

import type { TocEntryKind } from "../generated/document";

/** Top-level LoroTree container for the table of contents. */
export const TOC_CONTAINER = "toc";

// Node-metadata LoroMap keys (one map per tree node).
export const TOC_KEY_KIND = "kind";
export const TOC_KEY_TITLE = "title";
export const TOC_KEY_THING_ID = "thingId";
export const TOC_KEY_VISIBILITY = "visibility";
export const TOC_KEY_CONVERSATION_ID = "conversationId";

// Kind discriminant values. `satisfies TocEntryKind` pins them to the generated
// union, so a rename on the Rust/ts-rs side fails to compile here.
export const TOC_KIND_FOLDER = "folder" satisfies TocEntryKind;
export const TOC_KIND_THING = "thing" satisfies TocEntryKind;
export const TOC_KIND_SUGGESTION = "suggestion" satisfies TocEntryKind;

/**
 * Maximum nesting depth, counting the root level as depth 1:
 * ```
 * - One         (depth 1)
 *   - Two       (depth 2)
 *     - Three   (depth 3)
 * ```
 * Client-enforced only for now (cooperative-within-campaign threat model); the
 * server does not yet reject over-deep moves.
 */
export const TOC_MAX_DEPTH = 3;
