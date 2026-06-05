//! Regression guard for the loro-prosemirror content-wipe bug.
//!
//! `LoroSyncPlugin` defers its Loro->PM init to a `setTimeout(0)` inside the
//! plugin's `view()`. Until that fires, its PM<->Loro node mapping is empty. Any
//! `docChanged` transaction in that window reaches `updateLoroToPmState` with the
//! empty mapping, which (lacking a mapping to reconcile against) deletes the
//! populated Loro content and replaces it with the near-empty PM state -- then
//! `subscribeLocalUpdates` broadcasts that wipe to the server. React StrictMode's
//! editor create->destroy->recreate and A->B->A navigation both widen this
//! window. See loro-dev/loro-prosemirror#77.
//!
//! The fix is a vendored pnpm patch (`patches/loro-prosemirror@0.4.3.patch`,
//! pinned in `pnpm-workspace.yaml`) that adds an `initialized` flag and guards
//! the write behind it. We can only run the *patched* dependency, so this test
//! is a guard, not a before/after: it asserts the patched behaviour (server
//! content survives a pre-init edit). If a version bump ever drops the patch,
//! the wipe returns and this test fails loudly -- which is the point.
//!
//! Pure ProseMirror `EditorState` + the plugin's public API: no `EditorView`
//! (so the `setTimeout` init never fires and the plugin stays pre-init), no
//! jsdom, no server.

import { getSchema } from "@tiptap/core";
import { EditorState } from "@tiptap/pm/state";
import { LoroDoc } from "loro-crdt";
import { type LoroDocType, LoroSyncPlugin, updateLoroToPmState } from "loro-prosemirror";
import { describe, expect, it } from "vitest";

import { contentContainerId } from "./loro-extension";
import { NODE_DOC, NODE_EXTENSIONS, NODE_PARAGRAPH } from "./schema";

const SEED_TEXT = "The dragon Ashardalon slumbers beneath the Sunless Citadel.";
const RACE_TEXT = "PREMATURE_PRE_INIT_EDIT";

describe("loro-prosemirror content-wipe guard", () => {
  it("keeps server content when a transaction lands before the sync plugin initialises", () => {
    const schema = getSchema(NODE_EXTENSIONS);

    // A LoroDoc already populated from the server -- the state `usePageDoc`
    // reaches via `waitForReachingServerVersion()` before mounting the editor.
    // We seed it by writing a content-bearing PM state straight into the same
    // "content" container the app binds to (the inverse of what the plugin does
    // at runtime).
    const loroDoc = new LoroDoc();
    const containerId = contentContainerId(loroDoc);
    const seededDoc = schema.node(NODE_DOC, null, [
      schema.node(NODE_PARAGRAPH, null, [schema.text(SEED_TEXT)]),
    ]);
    updateLoroToPmState(
      loroDoc as LoroDocType,
      new Map(),
      EditorState.create({ schema, doc: seededDoc }),
      containerId,
    );
    loroDoc.commit();
    expect(JSON.stringify(loroDoc.toJSON())).toContain(SEED_TEXT); // seed actually took

    // Mount the sync plugin over an empty PM doc, but create NO EditorView. The
    // plugin's `setTimeout(0)` init lives in view(); with no view it never runs,
    // so the node mapping stays empty and `initialized` stays false -- the exact
    // pre-init window the bug needs.
    const plugin = LoroSyncPlugin({ doc: loroDoc as LoroDocType, containerId });
    const emptyDoc = schema.node(NODE_DOC, null, [schema.node(NODE_PARAGRAPH, null, [])]);
    const state = EditorState.create({ schema, doc: emptyDoc, plugins: [plugin] });

    // A docChanged transaction in that window. ProseMirror's own appendTransaction
    // loop tags it doc-changed and runs the plugin's apply() -- the path that,
    // unpatched, overwrites the populated Loro doc with this near-empty PM state.
    const editTr = state.tr.insertText(RACE_TEXT, 1);
    const stateAfterEdit = state.apply(editTr);

    // Guard against a vacuous pass: prove this edit really does reach the
    // doc-changed path, so the test would actually catch a dropped patch rather
    // than passing because nothing happened.
    expect(plugin.spec.appendTransaction?.([editTr], state, stateAfterEdit) ?? null).not.toBeNull();

    // The patch's `initialized` guard must have skipped the write: server content
    // survives and the pre-init edit was never pushed into Loro. Drop the patch
    // and these invert (SEED_TEXT gone, RACE_TEXT written).
    const afterRace = JSON.stringify(loroDoc.toJSON());
    expect(afterRace).toContain(SEED_TEXT);
    expect(afterRace).not.toContain(RACE_TEXT);
  });
});
