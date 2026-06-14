//! Binds a TipTap editor to a synced Loro document.
//!
//! `LoroSyncPlugin` keeps the ProseMirror state and a Loro container in sync;
//! `LoroUndoPlugin` provides CRDT-aware undo/redo (which is why the schema
//! omits TipTap's History extension). The transport that actually syncs the
//! `LoroDoc` over a WebSocket lives in the app, not here -- this package stays
//! transport-free.

import { Extension } from "@tiptap/core";
import type { ContainerID, LoroDoc, UndoManager } from "loro-crdt";
import { LoroSyncPlugin, LoroUndoPlugin, type LoroDocType } from "loro-prosemirror";

/**
 * Names of the root Loro maps holding each section's ProseMirror document,
 * matching the server's section constants in `campaign-shared`
 * (`loro::page::{CONTAINER_PREAMBLE, CONTAINER_BODY}`). These strings are a
 * cross-language contract; `section-contract.test.ts` guards them against drift.
 */
export const PREAMBLE_CONTAINER = "preamble";
export const BODY_CONTAINER = "body";

/**
 * The `ContainerID` loro-prosemirror should bind to for the preamble section.
 * Each section stores its ProseMirror tree under its own root map (not
 * loro-prosemirror's default "doc" map), so the sync plugin must be told which
 * container to use.
 */
export function preambleContainerId(doc: LoroDoc): ContainerID {
  return doc.getMap(PREAMBLE_CONTAINER).id;
}

/** The `ContainerID` loro-prosemirror should bind to for the body section. */
export function bodyContainerId(doc: LoroDoc): ContainerID {
  return doc.getMap(BODY_CONTAINER).id;
}

export interface LoroExtensionOptions {
  doc: LoroDoc;
  containerId: ContainerID;
  /**
   * Optional shared undo manager. Multiple section editors over one doc pass the
   * SAME instance so undo is unified at the page level (one Ctrl-Z stack across
   * preamble + body). Omitted, loro-prosemirror constructs a per-editor manager,
   * which is correct only for a single-editor page.
   */
  undoManager?: UndoManager;
}

/**
 * Configure with the synced doc and a section `containerId`, e.g.
 * `LoroExtension.configure({ doc, containerId: bodyContainerId(doc), undoManager })`.
 */
export const LoroExtension = Extension.create<LoroExtensionOptions>({
  name: "loroSync",

  addProseMirrorPlugins() {
    const { doc, containerId, undoManager } = this.options;
    return [
      // loro-prosemirror types its doc as a `LoroDoc<{ doc; data }>`
      // (`LoroDocType`), assuming the PM tree lives under a root map named
      // "doc". This app stores each section under its own root map and selects
      // it via `containerId`, so the doc we hold is a generic `LoroDoc`. The two
      // views describe the same runtime object and differ only in a compile-time
      // phantom key type; the plugin reads the container named by `containerId`
      // at runtime. Bridge the type here, once.
      LoroSyncPlugin({ doc: doc as LoroDocType, containerId }),
      // Pass the shared manager when given (unified page-level undo); otherwise
      // loro-prosemirror defaults to `new UndoManager(doc)`. Spread conditionally
      // so the optional key is omitted rather than set to `undefined`
      // (exactOptionalPropertyTypes).
      LoroUndoPlugin({ doc, ...(undoManager ? { undoManager } : {}) }),
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
