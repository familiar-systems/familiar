import type { TocEntry } from "@familiar-systems/types-campaign";
import type { TreeID } from "loro-crdt";
import { describe, expect, it } from "vitest";

import type { TocTreeNode } from "./toc-doc";
import {
  type FlatTocNode,
  flattenToc,
  getMovePlacement,
  getProjection,
  removeDescendants,
} from "./tree-utils";

const tid = (n: number): TreeID => `${n}@0` as TreeID;

function folder(title: string): TocEntry {
  return { kind: "folder", title, visibility: "known", suggestions: [] };
}

function node(id: TreeID, entry: TocEntry, children: TocTreeNode[] = []): TocTreeNode {
  return { treeId: id, entry, children };
}

// A:           B
//  ├ A1
//  └ A2
const A = node(tid(1), folder("A"), [node(tid(2), folder("A1")), node(tid(3), folder("A2"))]);
const B = node(tid(4), folder("B"));
const tree: TocTreeNode[] = [A, B];

const ids = (items: FlatTocNode[]): TreeID[] => items.map((i) => i.treeId);

describe("flattenToc", () => {
  it("flattens depth-first with depth and parent links", () => {
    const flat = flattenToc(tree, new Set());
    expect(ids(flat)).toEqual([tid(1), tid(2), tid(3), tid(4)]);
    expect(flat.map((i) => i.depth)).toEqual([0, 1, 1, 0]);
    expect(flat.map((i) => i.parentId)).toEqual([null, tid(1), tid(1), null]);
    expect(flat[0]?.hasChildren).toBe(true);
    expect(flat[3]?.hasChildren).toBe(false);
  });

  it("hides children of a collapsed node", () => {
    const flat = flattenToc(tree, new Set([tid(1)]));
    expect(ids(flat)).toEqual([tid(1), tid(4)]);
    expect(flat[0]?.collapsed).toBe(true);
  });
});

describe("removeDescendants", () => {
  it("keeps the active node but drops its subtree", () => {
    const flat = flattenToc(tree, new Set());
    expect(ids(removeDescendants(flat, tid(1)))).toEqual([tid(1), tid(4)]);
  });
});

describe("getProjection / getMovePlacement", () => {
  const flat = flattenToc(tree, new Set()); // [A, A1, A2, B]
  const indent = 16;
  const maxDepth = 2; // TOC_MAX_DEPTH (3) - 1

  it("drags a root node to the top with a leftward offset", () => {
    const proj = getProjection(flat, tid(4), tid(1), -100, indent, maxDepth);
    expect(proj).toEqual({ depth: 0, parentId: null });

    const place = getMovePlacement(flat, tid(4), tid(1), proj);
    expect(place).toEqual({ parentId: null, index: 0 });
  });

  it("nests a node under the row above when dragged rightward", () => {
    // Drop B over A2 with one indent of rightward offset -> sibling of A1/A2.
    const proj = getProjection(flat, tid(4), tid(3), indent, indent, maxDepth);
    expect(proj).toEqual({ depth: 1, parentId: tid(1) });

    const place = getMovePlacement(flat, tid(4), tid(3), proj);
    // Inserted after A1 and A2 are its siblings; B lands at index 1 (after A1).
    expect(place.parentId).toBe(tid(1));
    expect(place.index).toBe(1);
  });

  it("clamps depth to maxDepth", () => {
    // A huge rightward offset cannot push past one level under the previous row.
    const proj = getProjection(flat, tid(4), tid(3), 500, indent, maxDepth);
    expect(proj.depth).toBeLessThanOrEqual(maxDepth);
  });
});
