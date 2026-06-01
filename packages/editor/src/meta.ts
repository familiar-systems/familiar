//! Reading a Thing's metadata from its Loro document.
//!
//! A Thing's LoroDoc has a `meta` root map holding `title` and `status` as
//! LWW strings (mirrors `CONTAINER_META`/`KEY_TITLE` in
//! `crates/campaign-shared/src/loro/thing.rs`). Editing the title is out of
//! scope for now; this only reads it for display.

import type { LoroDoc } from "loro-crdt";

/** Root Loro map holding a Thing's metadata. */
export const META_CONTAINER = "meta";
/** Key under `meta` holding the Thing's title (an LWW string). */
export const META_TITLE_KEY = "title";

/** Read a Thing's title from its synced LoroDoc; `""` if unset. */
export function readThingTitle(doc: LoroDoc): string {
  const value = doc.getMap(META_CONTAINER).get(META_TITLE_KEY);
  return typeof value === "string" ? value : "";
}
