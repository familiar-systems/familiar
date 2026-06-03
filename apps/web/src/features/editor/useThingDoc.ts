// Subscribes one editor to a Thing's CRDT-synced Loro document. The socket and
// the room lifecycle live in LoroClientManager (one socket per campaign); this
// hook is a read-only subscription: it acquires/releases the room on mount/
// unmount and reads a referentially-stable snapshot via useSyncExternalStore.
// StrictMode double-mount and page-swap teardown are handled in the manager, not
// here.

import { contentContainerId } from "@familiar-systems/editor";
import type { ThingId } from "@familiar-systems/types-campaign";
import type { ContainerID, LoroDoc as LoroDocType } from "loro-crdt";
import { useCallback, useEffect, useMemo, useSyncExternalStore } from "react";

import { useLoroManager } from "./LoroManagerProvider";

export type ThingDocState =
  | { status: "connecting" }
  | { status: "synced"; doc: LoroDocType; containerId: ContainerID }
  // Socket dropped while the doc is open: keep editing (edits buffer locally) and
  // let the editor show a reconnecting indicator rather than tearing down.
  | { status: "reconnecting"; doc: LoroDocType; containerId: ContainerID }
  | { status: "error"; message: string };

export function useThingDoc(thingId: ThingId): ThingDocState {
  const manager = useLoroManager();

  // Ref-counted acquire/release. The manager debounces the leave, so a quick
  // unmount->remount (StrictMode, or fast back-and-forth) reuses the room.
  useEffect(() => {
    manager.acquireThing(thingId);
    return () => manager.releaseThing(thingId);
  }, [manager, thingId]);

  // Stable callbacks per (manager, thingId) for useSyncExternalStore.
  const subscribe = useCallback(
    (listener: () => void) => manager.subscribeThingDoc(thingId, listener),
    [manager, thingId],
  );
  const getSnapshot = useCallback(() => manager.getThingState(thingId), [manager, thingId]);
  const snapshot = useSyncExternalStore(subscribe, getSnapshot);

  // Derive the editor-facing state here (not in getSnapshot, which must return a
  // stable reference). The room's view is the LoroDoc; containerId is computed
  // per snapshot. The doc reference is stable across a reconnect, so the editor
  // (keyed by doc) is not recreated when the socket blips.
  return useMemo<ThingDocState>(() => {
    switch (snapshot.status) {
      case "joining":
        return { status: "connecting" };
      case "joined":
        return {
          status: "synced",
          doc: snapshot.view,
          containerId: contentContainerId(snapshot.view),
        };
      case "reconnecting":
        return {
          status: "reconnecting",
          doc: snapshot.view,
          containerId: contentContainerId(snapshot.view),
        };
      case "error":
        return { status: "error", message: snapshot.message };
    }
  }, [snapshot]);
}
