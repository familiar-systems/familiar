//! Reading a Page's metadata from its Loro document.
//!
//! A Page's LoroDoc has a `meta` root map holding `title` and `status` as
//! LWW strings (mirrors `CONTAINER_META`/`KEY_TITLE` in
//! `crates/campaign-shared/src/loro/page.rs`). Editing the title is out of
//! scope for now; this only reads it for display.

import type { LoroDoc } from "loro-crdt";

/** Root Loro map holding a Page's metadata. */
export const META_CONTAINER = "meta";
/** Key under `meta` holding the Page's title (an LWW string). */
export const META_TITLE_KEY = "title";

/** Read a Page's title from its synced LoroDoc; `""` if unset. */
export function readPageTitle(doc: LoroDoc): string {
  const value = doc.getMap(META_CONTAINER).get(META_TITLE_KEY);
  return typeof value === "string" ? value : "";
}
