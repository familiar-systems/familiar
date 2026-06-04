// The drag-and-drop ToC tree. dnd-kit owns the drag gesture only; the tree data
// lives in the LoroTree (passed in as `tree`). A drag emits one `onMove(node,
// parent, index)` which the caller turns into a single LoroTree move. While a
// drag is in flight we render from a frozen snapshot so a remote edit cannot
// reflow the list under the pointer; the doc keeps importing regardless and the
// move still merges conflict-free.

import { TOC_MAX_DEPTH } from "@familiar-systems/types-campaign";
import type { ThingId } from "@familiar-systems/types-campaign";
import {
  closestCenter,
  DndContext,
  DragOverlay,
  KeyboardSensor,
  PointerSensor,
  type DragEndEvent,
  type DragMoveEvent,
  type DragOverEvent,
  type DragStartEvent,
  type UniqueIdentifier,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  SortableContext,
  sortableKeyboardCoordinates,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import type { TreeID } from "loro-crdt";
import { useMemo, useState } from "react";

import type { TocTreeNode } from "./toc-doc";
import { TocCreateRow } from "./TocCreateRow";
import { TocRow } from "./TocRow";
import {
  flattenToc,
  getMovePlacement,
  getProjection,
  INDENT_WIDTH,
  removeDescendants,
  type FlatTocNode,
} from "./tree-utils";

// dnd-kit ids are UniqueIdentifier (string | number); ours are always TreeIDs.
const asTreeId = (id: UniqueIdentifier): TreeID => String(id) as TreeID;

const MAX_DRAG_DEPTH = TOC_MAX_DEPTH - 1; // depth here is 0-indexed

interface TocTreeProps {
  tree: TocTreeNode[];
  /** The page currently open in the editor (for active-row highlight). */
  activeThingId: ThingId | null;
  /** undefined = not creating, null = creating at root, ThingId = under that page. */
  pendingParent: ThingId | null | undefined;
  creating: boolean;
  onNavigate: (thingId: ThingId) => void;
  onMove: (node: TreeID, parent: TreeID | null, index: number) => void;
  onAddChild: (parent: ThingId) => void;
  onSubmitCreate: (name: string) => void;
  onCancelCreate: () => void;
}

export function TocTree({
  tree,
  activeThingId,
  pendingParent,
  creating,
  onNavigate,
  onMove,
  onAddChild,
  onSubmitCreate,
  onCancelCreate,
}: TocTreeProps): React.ReactElement {
  const [collapsed, setCollapsed] = useState<ReadonlySet<TreeID>>(new Set());
  const [activeId, setActiveId] = useState<TreeID | null>(null);
  const [overId, setOverId] = useState<TreeID | null>(null);
  const [offsetLeft, setOffsetLeft] = useState(0);
  // Snapshot captured at drag start; while non-null, remote updates do not reflow.
  const [frozen, setFrozen] = useState<FlatTocNode[] | null>(null);

  const liveItems = useMemo(() => flattenToc(tree, collapsed), [tree, collapsed]);
  const baseItems = frozen ?? liveItems;
  const items = useMemo(
    () => (activeId !== null ? removeDescendants(baseItems, activeId) : baseItems),
    [baseItems, activeId],
  );

  const projection = useMemo(
    () =>
      activeId !== null && overId !== null
        ? getProjection(items, activeId, overId, offsetLeft, INDENT_WIDTH, MAX_DRAG_DEPTH)
        : null,
    [items, activeId, overId, offsetLeft],
  );

  const sensors = useSensors(
    // A small distance threshold so a click navigates and only a deliberate drag
    // starts a sort.
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const sortableIds = useMemo(() => items.map((i) => i.treeId), [items]);
  const activeNode = activeId !== null ? (items.find((i) => i.treeId === activeId) ?? null) : null;

  function toggleCollapse(treeId: TreeID): void {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(treeId)) next.delete(treeId);
      else next.add(treeId);
      return next;
    });
  }

  function handleAddChild(thingId: ThingId): void {
    // Expand the target so the inline input (and existing children) are visible.
    const match = items.find((i) => i.entry.kind === "thing" && i.entry.thingId === thingId);
    if (match !== undefined) {
      setCollapsed((prev) => {
        const next = new Set(prev);
        next.delete(match.treeId);
        return next;
      });
    }
    onAddChild(thingId);
  }

  function reset(): void {
    setActiveId(null);
    setOverId(null);
    setOffsetLeft(0);
    setFrozen(null);
  }

  function handleDragStart(e: DragStartEvent): void {
    const id = asTreeId(e.active.id);
    setFrozen(liveItems);
    setActiveId(id);
    setOverId(id);
    setOffsetLeft(0);
  }

  function handleDragEnd(e: DragEndEvent): void {
    const over = e.over === null ? null : asTreeId(e.over.id);
    if (activeId !== null && over !== null) {
      // Recompute from the end event's final delta/over rather than the memoized
      // `projection`: under coalesced pointer events the last onDragMove may not
      // have flushed setOffsetLeft/setOverId before drag end, which would commit
      // the drop one indent level (or one target) off. e.delta.x / e.over are
      // authoritative. `items` is safe to read here (derived from the frozen
      // snapshot + activeId, both stable for the drag's duration).
      const finalProjection = getProjection(
        items,
        activeId,
        over,
        e.delta.x,
        INDENT_WIDTH,
        MAX_DRAG_DEPTH,
      );
      const placement = getMovePlacement(items, activeId, over, finalProjection);
      onMove(activeId, placement.parentId, placement.index);
    }
    reset();
  }

  // Render list, splicing the inline create-row in at root or under its parent.
  const rows: React.ReactNode[] = [];
  if (pendingParent === null) {
    rows.push(
      <TocCreateRow
        key={`create-${pendingParent ?? "root"}`}
        depth={0}
        indentWidth={INDENT_WIDTH}
        busy={creating}
        onSubmit={onSubmitCreate}
        onCancel={onCancelCreate}
      />,
    );
  }
  for (const node of items) {
    const isActive = node.treeId === activeId;
    const depth = isActive && projection !== null ? projection.depth : node.depth;
    const entry = node.entry;
    const open = entry.kind === "thing" && entry.thingId === activeThingId;
    rows.push(
      <TocRow
        key={node.treeId}
        node={node}
        depth={depth}
        indentWidth={INDENT_WIDTH}
        active={open}
        onNavigate={onNavigate}
        onToggleCollapse={toggleCollapse}
        onAddChild={handleAddChild}
      />,
    );
    if (
      pendingParent !== null &&
      pendingParent !== undefined &&
      entry.kind === "thing" &&
      entry.thingId === pendingParent
    ) {
      rows.push(
        <TocCreateRow
          key={`create-${pendingParent ?? "root"}`}
          depth={node.depth + 1}
          indentWidth={INDENT_WIDTH}
          busy={creating}
          onSubmit={onSubmitCreate}
          onCancel={onCancelCreate}
        />,
      );
    }
  }

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragStart={handleDragStart}
      onDragMove={(e: DragMoveEvent) => setOffsetLeft(e.delta.x)}
      onDragOver={(e: DragOverEvent) => setOverId(e.over === null ? null : asTreeId(e.over.id))}
      onDragEnd={handleDragEnd}
      onDragCancel={reset}
    >
      <SortableContext items={sortableIds} strategy={verticalListSortingStrategy}>
        <div className="flex flex-col gap-0.5">{rows}</div>
      </SortableContext>
      <DragOverlay>
        {activeNode !== null ? (
          <div className="rounded-md bg-background/95 px-2 py-1 text-sm text-foreground shadow-lg ring-1 shadow-primary/10 ring-primary/20">
            {activeNode.entry.title ?? "Untitled"}
          </div>
        ) : null}
      </DragOverlay>
    </DndContext>
  );
}
