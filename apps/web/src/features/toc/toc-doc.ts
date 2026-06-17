// Client-side read/write surface for the table-of-contents LoroTree, the mirror
// of the Rust `LoroTocDoc` (apps/campaign/src/loro/toc.rs). The tree is the only
// source of truth: the sidebar renders from `readTocTree`, and a drag emits one
// `moveTocNode` which syncs over the `"toc"` room (the LoroAdaptor auto-sends the
// commit). Suggestion nodes are skipped here for now: there is no client renderer
// for them yet (the AI system is not built).

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
  pageIdSchema,
} from "@familiar-systems/types-campaign";
import type { PageId, Status, TocEntry, TocPageKind } from "@familiar-systems/types-campaign";
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

// The ordinal is stored as a Loro integer; the binding may surface it as a number
// or a bigint depending on magnitude, so normalize both to a JS number (ordinals
// are small). A missing/non-numeric value is null.
function asOrdinal(value: unknown): number | null {
  if (typeof value === "number") return value;
  if (typeof value === "bigint") return Number(value);
  return null;
}

// Decode a page node's `TocPageKind` from its meta. Strict, mirroring the Rust
// `read_page_kind`: a missing/unknown `pageKind`, or a `session` with no ordinal,
// returns null so the caller skips the node (the doc is server-authored and
// rebuilt from SQLite each checkout, so a malformed node is corruption, not an
// old format to tolerate).
function readPageKind(data: LoroMap): TocPageKind | null {
  switch (asString(data.get(TOC_KEY_PAGE_KIND))) {
    case "entity":
      return { kind: "entity" };
    case "template":
      return { kind: "template" };
    case "session": {
      const ordinal = asOrdinal(data.get(TOC_KEY_ORDINAL));
      return ordinal === null ? null : { kind: "session", ordinal };
    }
    default:
      return null;
  }
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
    case TOC_KIND_PAGE: {
      const title = asString(data.get(TOC_KEY_TITLE));
      const rawId = pageIdSchema.safeParse(data.get(TOC_KEY_PAGE_ID));
      const pageKind = readPageKind(data);
      if (title === null || !rawId.success || pageKind === null) return null;
      return { kind: "page", title, pageId: rawId.data, pageKind, visibility, suggestions: [] };
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

/** A ToC entry narrowed to the `page` variant (carries pageId/pageKind/ordinal). */
export type TocPageEntry = Extract<TocEntry, { kind: "page" }>;

/**
 * Find the Page entry for `pageId` anywhere in the tree. The page header uses it
 * to read the immutable kind/ordinal it composes its prefix from (the editable
 * name stays sourced from the live page doc). Returns null until the page shows
 * up in the synced ToC.
 */
export function findTocPageEntry(nodes: TocTreeNode[], pageId: PageId): TocPageEntry | null {
  for (const node of nodes) {
    if (node.entry.kind === "page" && node.entry.pageId === pageId) return node.entry;
    const found = findTocPageEntry(node.children, pageId);
    if (found !== null) return found;
  }
  return null;
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
