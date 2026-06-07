//! Reading and writing a Page's metadata on its Loro document.
//!
//! A Page's LoroDoc has a `meta` root map holding `title` and `status` as
//! LWW strings (mirrors `CONTAINER_META`/`KEY_TITLE` in
//! `crates/campaign-shared/src/loro/page.rs`). The title is the single source of
//! truth for a Page's name: the client edits only `meta.title` here, the room's
//! LoroAdaptor sends the commit, and the server (PageActor) mirrors it to
//! `pages.name` and pushes the change to the ToC. We never write the ToC title
//! from the client.

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

/**
 * Write a Page's title to its synced LoroDoc and commit so the LoroAdaptor
 * broadcasts the change. The title is an LWW string (concurrent edits pick a
 * winner, not interleave), matching the server's `meta.title` map semantics.
 */
export function writePageTitle(doc: LoroDoc, title: string): void {
  doc.getMap(META_CONTAINER).set(META_TITLE_KEY, title);
  doc.commit();
}
