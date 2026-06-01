//! Binds a TipTap editor to a synced Loro document.
//!
//! `LoroSyncPlugin` keeps the ProseMirror state and a Loro container in sync;
//! `LoroUndoPlugin` provides CRDT-aware undo/redo (which is why the schema
//! omits TipTap's History extension). The transport that actually syncs the
//! `LoroDoc` over a WebSocket lives in the app, not here -- this package stays
//! transport-free.

import { Extension } from "@tiptap/core";
import type { ContainerID, LoroDoc } from "loro-crdt";
import { LoroSyncPlugin, LoroUndoPlugin, type LoroDocType } from "loro-prosemirror";

/**
 * Name of the root Loro map holding the ProseMirror document, matching the
 * server's `CONTAINER_CONTENT` ("content") in `campaign-shared`.
 */
export const CONTENT_CONTAINER = "content";

/**
 * The `ContainerID` loro-prosemirror should bind to: this app stores the
 * ProseMirror tree under the "content" root map, not loro-prosemirror's default
 * "doc" map, so the sync plugin must be told which container to use.
 */
export function contentContainerId(doc: LoroDoc): ContainerID {
  return doc.getMap(CONTENT_CONTAINER).id;
}

export interface LoroExtensionOptions {
  doc: LoroDoc;
  containerId: ContainerID;
}

/**
 * Configure with the synced doc and its content `containerId`:
 * `LoroExtension.configure({ doc, containerId: contentContainerId(doc) })`.
 */
export const LoroExtension = Extension.create<LoroExtensionOptions>({
  name: "loroSync",

  addProseMirrorPlugins() {
    const { doc, containerId } = this.options;
    return [
      // loro-prosemirror types its doc as a `LoroDoc<{ doc; data }>`
      // (`LoroDocType`), assuming the PM tree lives under a root map named
      // "doc". This app stores it under "content" and selects it via
      // `containerId`, so the doc we hold is a generic `LoroDoc`. The two views
      // describe the same runtime object and differ only in a compile-time
      // phantom key type; the plugin reads the container named by `containerId`
      // at runtime. Bridge the type here, once.
      LoroSyncPlugin({ doc: doc as LoroDocType, containerId }),
      LoroUndoPlugin({ doc }),
      // KNOWN BUG -- read before wiring multiplayer cursor presence here.
      // Do NOT add `LoroCursorPlugin` / `LoroEphemeralCursorPlugin` until the
      // loro-prosemirror cursor-mapping staleness bug is fixed. loro-prosemirror
      // maps ProseMirror nodes to Loro containers by object identity, and PM
      // nodes are immutable, so the mapping goes stale on edits that mint a new
      // node object (notably the blockId stamp on Enter). Today that surfaces
      // only as a suppressed `console.error("Cannot find the loroNode")` from the
      // undo plugin's selection snapshot (see patches/loro-prosemirror@0.4.3.patch
      // and the note in pnpm-workspace.yaml). The presence plugins run the SAME
      // conversion, where the bug instead corrupts remote cursors -- they jump to
      // the end of the document on concurrent edits (loro-dev/loro-prosemirror#78:
      // https://github.com/loro-dev/loro-prosemirror/issues/78).
      // Wiring presence MUST come with a real fix (resolve position<->container by
      // tree path, not object identity) AND removal of that patch suppression.
    ];
  },
});
