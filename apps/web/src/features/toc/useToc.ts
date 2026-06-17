// Read-only subscription to the campaign's table of contents, plus the hook that
// pins the ToC room. The room is ref-counted in LoroClientManager like any other;
// the campaign layout calls useTocRoom() to hold the single long-lived acquire,
// and useToc() is a pure subscription that maps the manager's room snapshot into
// the sidebar's vocabulary.

import { useEffect, useMemo, useSyncExternalStore } from "react";

import type { PageId } from "@familiar-systems/types-campaign";

import { useLoroManager } from "../editor/LoroManagerProvider";
import type { RoomError } from "../editor/loro-manager";
import { pagePrefix } from "./pageDisplayName";
import { findTocPageEntry, type TocTreeNode } from "./toc-doc";

export type TocSnapshot =
  | { status: "loading" }
  | { status: "ready"; tree: TocTreeNode[] }
  // Socket dropped while the tree is open: keep showing the last-known tree with
  // an indicator rather than collapsing back to the loading state.
  | { status: "reconnecting"; tree: TocTreeNode[] }
  | { status: "error"; error: RoomError };

/**
 * Pin the campaign's ToC room for the lifetime of the calling component. Mounted
 * once at the campaign layout so the tree is available to every reader (the
 * sidebar today, a breadcrumb tomorrow) without each reader churning the join.
 */
export function useTocRoom(): void {
  const manager = useLoroManager();
  useEffect(() => {
    manager.acquireToc();
    return () => manager.releaseToc();
  }, [manager]);
}

export function useToc(): TocSnapshot {
  const manager = useLoroManager();
  // subscribeToc / getTocSnapshot are stable bound fields on the manager instance
  // (itself stable for the campaign mount), so they can be passed directly.
  const snapshot = useSyncExternalStore(manager.subscribeToc, manager.getTocSnapshot);
  return useMemo<TocSnapshot>(() => {
    switch (snapshot.status) {
      case "joining":
        return { status: "loading" };
      case "joined":
        return { status: "ready", tree: snapshot.view };
      case "reconnecting":
        return { status: "reconnecting", tree: snapshot.view };
      case "error":
        return { status: "error", error: snapshot.error };
    }
  }, [snapshot]);
}

/**
 * The non-editable display prefix for a page (e.g. "Session 3:", "Template:"),
 * or null for an entity / before the page appears in the synced ToC. Reads the
 * immutable kind/ordinal off the ToC entry; the editable name stays sourced from
 * the live page doc, so the header composes prefix + live title.
 */
export function usePagePrefix(pageId: PageId): string | null {
  const snapshot = useToc();
  return useMemo(() => {
    if (snapshot.status !== "ready" && snapshot.status !== "reconnecting") return null;
    const entry = findTocPageEntry(snapshot.tree, pageId);
    return entry === null ? null : pagePrefix(entry.pageKind);
  }, [snapshot, pageId]);
}
