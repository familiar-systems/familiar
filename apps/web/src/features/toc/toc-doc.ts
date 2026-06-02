// Client-side read/write surface for the table-of-contents LoroTree, the mirror
// of the Rust `LoroTocDoc` (apps/campaign/src/loro/toc.rs). The tree is the only
// source of truth: the sidebar renders from `readTocTree`, and a drag emits one
// `moveTocNode` which syncs over the `"toc"` room (the LoroAdaptor auto-sends the
// commit). Suggestion nodes are skipped here for now: there is no client renderer
// for them yet (the AI system is not built).

import {
  TOC_CONTAINER,
  TOC_KEY_KIND,
  TOC_KEY_THING_ID,
  TOC_KEY_TITLE,
  TOC_KEY_VISIBILITY,
  TOC_KIND_FOLDER,
  TOC_KIND_THING,
  thingIdSchema,
} from "@familiar-systems/types-campaign";
import type { Status, TocEntry } from "@familiar-systems/types-campaign";
import type { LoroDoc, LoroMap, LoroTree, LoroTreeNode, TreeID } from "loro-crdt";

// A node read out of the LoroTree: its stable TreeID, the decoded entry, and its
// children. Mirrors the Rust `TocTreeNode`.
export interface TocTreeNode {
  treeId: TreeID;
  entry: TocEntry;
  children: TocTreeNode[];
}

/**
 * Get the ToC tree, ensuring fractional indexing is on so positional moves
 * (`move(target, parent, index)`) work. The server enables it too
 * (`enable_fractional_index(0)`); we guard rather than blindly re-enable.
 */
export function getTocTree(doc: LoroDoc): LoroTree {
  const tree = doc.getTree(TOC_CONTAINER);
  if (!tree.isFractionalIndexEnabled()) {
    tree.enableFractionalIndex(0);
  }
  return tree;
}

function asString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function asStatus(value: unknown): Status | null {
  return value === "gmOnly" || value === "known" || value === "retconned" ? value : null;
}

// Decode one node's metadata map into a `TocEntry`. Returns null for entries we do
// not render (suggestions, unknown kinds) or that are missing required fields, so a
// single malformed node is skipped rather than breaking the whole tree. Mirrors the
// Rust `read_entry_from_meta`.
function readEntry(data: LoroMap): TocEntry | null {
  const kind = asString(data.get(TOC_KEY_KIND));
  const visibility = asStatus(data.get(TOC_KEY_VISIBILITY));
  if (kind === null || visibility === null) return null;

  switch (kind) {
    case TOC_KIND_FOLDER: {
      const title = asString(data.get(TOC_KEY_TITLE));
      if (title === null) return null;
      return { kind: "folder", title, visibility, suggestions: [] };
    }
    case TOC_KIND_THING: {
      const title = asString(data.get(TOC_KEY_TITLE));
      const rawId = thingIdSchema.safeParse(data.get(TOC_KEY_THING_ID));
      if (title === null || !rawId.success) return null;
      return { kind: "thing", title, thingId: rawId.data, visibility, suggestions: [] };
    }
    default:
      // Suggestion entries and any unknown kind: skipped for now.
      return null;
  }
}

function readNodes(nodes: LoroTreeNode[] | undefined): TocTreeNode[] {
  if (nodes === undefined) return [];
  const out: TocTreeNode[] = [];
  for (const node of nodes) {
    const entry = readEntry(node.data);
    if (entry === null) continue;
    out.push({ treeId: node.id, entry, children: readNodes(node.children()) });
  }
  return out;
}

/** Read the full ToC tree as an immutable snapshot. Mirrors Rust `read_tree`. */
export function readTocTree(doc: LoroDoc): TocTreeNode[] {
  return readNodes(getTocTree(doc).roots());
}

/**
 * Move `node` to be the child of `parent` (or a root when `parent` is null) at
 * `index` among its siblings, then commit so the LoroAdaptor broadcasts the delta.
 * The CRDT move is conflict-free: concurrent peer moves merge rather than corrupt.
 */
export function moveTocNode(
  doc: LoroDoc,
  node: TreeID,
  parent: TreeID | null,
  index: number,
): void {
  getTocTree(doc).move(node, parent ?? undefined, index);
  doc.commit();
}
