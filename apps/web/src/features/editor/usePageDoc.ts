// Subscribes one editor to a Page's CRDT-synced Loro document. The socket and
// the room lifecycle live in LoroClientManager (one socket per campaign); this
// hook is a read-only subscription: it acquires/releases the room on mount/
// unmount and reads a referentially-stable snapshot via useSyncExternalStore.
// StrictMode double-mount and page-swap teardown are handled in the manager, not
// here.

import type { PageId } from "@familiar-systems/types-campaign";
import type { LoroDoc as LoroDocType } from "loro-crdt";
import { useCallback, useEffect, useMemo, useSyncExternalStore } from "react";

import { useLoroManager } from "./LoroManagerProvider";
import type { RoomError } from "./loro-manager";

export type PageDocState =
  | { status: "connecting" }
  // The page has multiple section containers (preamble, body); the consumer
  // derives each section's `containerId` from the doc, so the doc is all we
  // expose. The doc reference is stable across a reconnect.
  | { status: "synced"; doc: LoroDocType }
  // Socket dropped while the doc is open: keep editing (edits buffer locally) and
  // let the editor show a reconnecting indicator rather than tearing down.
  | { status: "reconnecting"; doc: LoroDocType }
  | { status: "error"; error: RoomError };

export function usePageDoc(pageId: PageId): PageDocState {
  const manager = useLoroManager();

  // Ref-counted acquire/release. The manager debounces the leave, so a quick
  // unmount->remount (StrictMode, or fast back-and-forth) reuses the room.
  useEffect(() => {
    manager.acquirePage(pageId);
    return () => manager.releasePage(pageId);
  }, [manager, pageId]);

  // Stable callbacks per (manager, pageId) for useSyncExternalStore.
  const subscribe = useCallback(
    (listener: () => void) => manager.subscribePageDoc(pageId, listener),
    [manager, pageId],
  );
  const getSnapshot = useCallback(() => manager.getPageState(pageId), [manager, pageId]);
  const snapshot = useSyncExternalStore(subscribe, getSnapshot);

  // Derive the editor-facing state here (not in getSnapshot, which must return a
  // stable reference). The room's view is the LoroDoc; the doc reference is
  // stable across a reconnect, so the editors (keyed by doc) are not recreated
  // when the socket blips.
  return useMemo<PageDocState>(() => {
    switch (snapshot.status) {
      case "joining":
        return { status: "connecting" };
      case "joined":
        return { status: "synced", doc: snapshot.view };
      case "reconnecting":
        return { status: "reconnecting", doc: snapshot.view };
      case "error":
        return { status: "error", error: snapshot.error };
    }
  }, [snapshot]);
}
