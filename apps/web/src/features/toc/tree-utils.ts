// Pure tree/drag math for the ToC sidebar, adapted from the dnd-kit "sortable
// tree" example (https://github.com/clauderic/dnd-kit, examples/.../Tree). The
// tree itself lives in the LoroTree; these helpers only turn the nested snapshot
// into a flat, indented, drag-projectable list and compute where a drop lands.
//
// Depth is 0-indexed here (root = 0). The shared schema's TOC_MAX_DEPTH counts
// levels from 1, so the deepest allowed 0-indexed depth is TOC_MAX_DEPTH - 1.

import type { TocEntry } from "@familiar-systems/types-campaign";
import type { TreeID } from "loro-crdt";

import type { TocTreeNode } from "./toc-doc";

/** Horizontal pixels per nesting level (drives drag projection + render indent). */
export const INDENT_WIDTH = 16;
/** Base left padding before the first indent level. */
export const ROW_INDENT_BASE = 8;

// Local, dependency-free array move (the dnd-kit one lives in a DOM-coupled module
// we do not want to pull into this pure, node-testable helper).
function arrayMove<T>(items: T[], from: number, to: number): T[] {
  const copy = items.slice();
  const [moved] = copy.splice(from, 1);
  if (moved !== undefined) copy.splice(to, 0, moved);
  return copy;
}

export interface FlatTocNode {
  treeId: TreeID;
  entry: TocEntry;
  parentId: TreeID | null;
  depth: number;
  /** Whether the node has children in the source tree (drives the chevron). */
  hasChildren: boolean;
  /** Whether this node is currently collapsed (its children are hidden). */
  collapsed: boolean;
}

/** Where a drag would land: a new parent and the projected indent depth. */
export interface Projection {
  depth: number;
  parentId: TreeID | null;
}

/**
 * Depth-first flatten of the nested snapshot into a render list. Children of a
 * collapsed node are omitted so they neither render nor participate in dragging.
 */
export function flattenToc(
  nodes: TocTreeNode[],
  collapsed: ReadonlySet<TreeID>,
  parentId: TreeID | null = null,
  depth = 0,
): FlatTocNode[] {
  const out: FlatTocNode[] = [];
  for (const node of nodes) {
    const isCollapsed = collapsed.has(node.treeId);
    out.push({
      treeId: node.treeId,
      entry: node.entry,
      parentId,
      depth,
      hasChildren: node.children.length > 0,
      collapsed: isCollapsed,
    });
    if (!isCollapsed && node.children.length > 0) {
      out.push(...flattenToc(node.children, collapsed, node.treeId, depth + 1));
    }
  }
  return out;
}

/**
 * Drop the active node's descendants from the list during a drag, so it cannot be
 * dropped into one of its own children (which the LoroTree move would reject). The
 * active node itself is kept (it stays draggable).
 */
export function removeDescendants(items: FlatTocNode[], rootId: TreeID): FlatTocNode[] {
  const excluded = new Set<TreeID>([rootId]);
  return items.filter((item) => {
    if (item.parentId !== null && excluded.has(item.parentId)) {
      excluded.add(item.treeId);
      return false;
    }
    return true;
  });
}

function dragDepth(offset: number, indentWidth: number): number {
  return Math.round(offset / indentWidth);
}

function maxAllowedDepth(previous: FlatTocNode | undefined, ceiling: number): number {
  if (previous === undefined) return 0;
  return Math.min(previous.depth + 1, ceiling);
}

function minAllowedDepth(next: FlatTocNode | undefined): number {
  return next?.depth ?? 0;
}

/**
 * Project the dragged item's target depth + parent from the horizontal drag
 * offset, clamped so it can only nest one level under the row above and no deeper
 * than `maxDepth`. `items` must already exclude the active subtree.
 */
export function getProjection(
  items: FlatTocNode[],
  activeId: TreeID,
  overId: TreeID,
  offsetLeft: number,
  indentWidth: number,
  maxDepth: number,
): Projection {
  const overIndex = items.findIndex((i) => i.treeId === overId);
  const activeIndex = items.findIndex((i) => i.treeId === activeId);
  const active = items[activeIndex];
  if (active === undefined || overIndex === -1) return { depth: 0, parentId: null };

  const moved = arrayMove(items, activeIndex, overIndex);
  const previous = moved[overIndex - 1];
  const next = moved[overIndex + 1];

  const projected = active.depth + dragDepth(offsetLeft, indentWidth);
  const ceiling = maxAllowedDepth(previous, maxDepth);
  const floor = minAllowedDepth(next);
  const depth = projected >= ceiling ? ceiling : projected < floor ? floor : projected;

  return { depth, parentId: parentForDepth(moved, overIndex, depth, previous) };
}

function parentForDepth(
  moved: FlatTocNode[],
  overIndex: number,
  depth: number,
  previous: FlatTocNode | undefined,
): TreeID | null {
  if (depth === 0 || previous === undefined) return null;
  if (depth === previous.depth) return previous.parentId;
  if (depth > previous.depth) return previous.treeId;
  // Shallower than the previous row: walk back to the nearest row at this depth
  // and adopt its parent.
  for (let i = overIndex - 1; i >= 0; i--) {
    const candidate = moved[i];
    if (candidate !== undefined && candidate.depth === depth) return candidate.parentId;
  }
  return null;
}

/**
 * Translate a projection into a concrete `(parentId, index)` for `tree.move`:
 * the sibling index is the count of the new parent's children that precede the
 * active node's new flat position.
 */
export function getMovePlacement(
  items: FlatTocNode[],
  activeId: TreeID,
  overId: TreeID,
  projection: Projection,
): { parentId: TreeID | null; index: number } {
  const activeIndex = items.findIndex((i) => i.treeId === activeId);
  const overIndex = items.findIndex((i) => i.treeId === overId);
  const moved = arrayMove(items, activeIndex, overIndex);

  let index = 0;
  for (const node of moved) {
    if (node.treeId === activeId) break;
    if (node.parentId === projection.parentId) index++;
  }
  return { parentId: projection.parentId, index };
}
