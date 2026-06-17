import {
  TOC_CONTAINER,
  TOC_KEY_KIND,
  TOC_KEY_ORDINAL,
  TOC_KEY_PAGE_ID,
  TOC_KEY_PAGE_KIND,
  TOC_KEY_TITLE,
  TOC_KEY_VISIBILITY,
  TOC_KIND_FOLDER,
  TOC_KIND_PAGE,
} from "@familiar-systems/types-campaign";
import { LoroDoc } from "loro-crdt";
import type { TreeID } from "loro-crdt";
import { describe, expect, it } from "vitest";

import { getTocTree, moveTocNode, readTocTree } from "./toc-doc";

// Canonical 26-char ULIDs (accepted by pageIdSchema).
const PAGE_A = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const PAGE_B = "01BX5ZZKBKACTAV9WEVGEMMVRZ";

function addFolder(doc: LoroDoc, parent: TreeID | undefined, title: string): TreeID {
  const node = getTocTree(doc).createNode(parent);
  node.data.set(TOC_KEY_KIND, TOC_KIND_FOLDER);
  node.data.set(TOC_KEY_TITLE, title);
  node.data.set(TOC_KEY_VISIBILITY, "known");
  return node.id;
}

function addPage(
  doc: LoroDoc,
  parent: TreeID | undefined,
  title: string,
  pageId: string,
  opts?: { pageKind?: string; ordinal?: number },
): TreeID {
  const node = getTocTree(doc).createNode(parent);
  node.data.set(TOC_KEY_KIND, TOC_KIND_PAGE);
  node.data.set(TOC_KEY_TITLE, title);
  node.data.set(TOC_KEY_PAGE_ID, pageId);
  node.data.set(TOC_KEY_VISIBILITY, "gmOnly");
  // Pages always carry a pageKind now (strict decode); default to entity.
  node.data.set(TOC_KEY_PAGE_KIND, opts?.pageKind ?? "entity");
  if (opts?.ordinal !== undefined) node.data.set(TOC_KEY_ORDINAL, opts.ordinal);
  return node.id;
}

describe("readTocTree", () => {
  it("decodes folders and pages into a nested structure", () => {
    const doc = new LoroDoc();
    const actId = addFolder(doc, undefined, "Act I");
    addPage(doc, actId, "The Iron Citadel", PAGE_A);
    addPage(doc, undefined, "Korgath", PAGE_B);
    doc.commit();

    const [act, korgath] = readTocTree(doc);

    expect(act?.entry).toEqual({
      kind: "folder",
      title: "Act I",
      visibility: "known",
      suggestions: [],
    });
    expect(act?.children).toHaveLength(1);
    expect(act?.children[0]?.entry).toMatchObject({
      kind: "page",
      title: "The Iron Citadel",
      pageId: PAGE_A,
    });
    expect(korgath?.entry).toMatchObject({ kind: "page", title: "Korgath", pageId: PAGE_B });
    expect(korgath?.children).toHaveLength(0);
  });

  it("reads a tree imported from a snapshot (the server -> client path)", () => {
    // Mirrors production: the server builds the tree, exports a snapshot, the
    // client imports it into a fresh doc and reads it. Exercises the import +
    // fractional-index handling that the local-build tests do not.
    const server = new LoroDoc();
    const actId = addFolder(server, undefined, "Act I");
    addPage(server, actId, "The Iron Citadel", PAGE_A);
    server.commit();
    const snapshot = server.export({ mode: "snapshot" });

    const client = new LoroDoc();
    client.import(snapshot);

    const tree = readTocTree(client);
    expect(tree).toHaveLength(1);
    expect(tree[0]?.entry).toMatchObject({ kind: "folder", title: "Act I" });
    expect(tree[0]?.children[0]?.entry).toMatchObject({
      kind: "page",
      title: "The Iron Citadel",
      pageId: PAGE_A,
    });
  });

  it("reading does not produce local updates or mutate the doc", () => {
    // Regression: readTocTree must be pure. A read that enables fractional index
    // (or otherwise writes) generates a local op the LoroAdaptor broadcasts,
    // clobbering the shared server tree under StrictMode's join/teardown race.
    const server = new LoroDoc();
    addPage(server, undefined, "A", PAGE_A);
    server.commit();
    const client = new LoroDoc();
    client.import(server.export({ mode: "snapshot" }));

    let localUpdates = 0;
    const unsub = client.subscribeLocalUpdates(() => {
      localUpdates += 1;
    });
    readTocTree(client);
    readTocTree(client);
    client.commit();
    unsub();

    expect(localUpdates).toBe(0);
    // The fractional-index flag is set by the server and survives import, so reads
    // never need to (and must not) enable it.
    expect(client.getTree(TOC_CONTAINER).isFractionalIndexEnabled()).toBe(true);
  });

  it("decodes the pageKind sum, with a session carrying its ordinal", () => {
    const doc = new LoroDoc();
    addPage(doc, undefined, "Korgath", PAGE_A); // defaults to entity
    addPage(doc, undefined, "The Fall", PAGE_B, { pageKind: "session", ordinal: 3 });
    doc.commit();

    const [entity, session] = readTocTree(doc);
    expect(entity?.entry).toMatchObject({ kind: "page", pageKind: { kind: "entity" } });
    expect(session?.entry).toMatchObject({
      kind: "page",
      pageKind: { kind: "session", ordinal: 3 },
    });
  });

  it("skips a page node missing its pageKind (strict decode)", () => {
    const doc = new LoroDoc();
    const node = getTocTree(doc).createNode();
    node.data.set(TOC_KEY_KIND, TOC_KIND_PAGE);
    node.data.set(TOC_KEY_TITLE, "No kind");
    node.data.set(TOC_KEY_PAGE_ID, PAGE_A);
    node.data.set(TOC_KEY_VISIBILITY, "gmOnly");
    // no pageKind set
    doc.commit();

    expect(readTocTree(doc)).toHaveLength(0);
  });

  it("skips suggestion and malformed nodes rather than throwing", () => {
    const doc = new LoroDoc();
    addFolder(doc, undefined, "Keeper");
    // A suggestion node (no client renderer yet) and a node missing its kind.
    const suggestion = getTocTree(doc).createNode();
    suggestion.data.set(TOC_KEY_KIND, "suggestion");
    suggestion.data.set(TOC_KEY_VISIBILITY, "gmOnly");
    const malformed = getTocTree(doc).createNode();
    malformed.data.set(TOC_KEY_TITLE, "no kind");
    doc.commit();

    const tree = readTocTree(doc);
    expect(tree).toHaveLength(1);
    expect(tree[0]?.entry.kind).toBe("folder");
  });
});

describe("moveTocNode", () => {
  it("reparents a node and places it at the given sibling index", () => {
    const doc = new LoroDoc();
    const actId = addFolder(doc, undefined, "Act I");
    addPage(doc, actId, "The Iron Citadel", PAGE_A);
    const korgathId = addPage(doc, undefined, "Korgath", PAGE_B);
    doc.commit();

    // Move Korgath from root to be the first child of Act I.
    moveTocNode(doc, korgathId, actId, 0);

    const tree = readTocTree(doc);
    expect(tree).toHaveLength(1);
    expect(tree[0]?.children).toHaveLength(2);
    expect(tree[0]?.children[0]?.entry).toMatchObject({ title: "Korgath" });
    expect(tree[0]?.children[1]?.entry).toMatchObject({ title: "The Iron Citadel" });
  });
});
