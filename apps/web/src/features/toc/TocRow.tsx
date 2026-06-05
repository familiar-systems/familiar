// One row in the ToC tree: a sortable, indented entry with a chevron, kind icon,
// GM-only glyph, a hover "add sub-page" button, and a hover drag handle. The drag
// listeners live on the handle (not the whole row) so a plain click still
// navigates. Presentational only: all state lives in TocTree.

import type { PageId } from "@familiar-systems/types-campaign";
import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import type { TreeID } from "loro-crdt";
import {
  ChevronDown,
  ChevronRight,
  EyeOff,
  FileText,
  Folder,
  GripVertical,
  Plus,
} from "lucide-react";

import { ROW_INDENT_BASE, type FlatTocNode } from "./tree-utils";

interface TocRowProps {
  node: FlatTocNode;
  /** Visual depth (may differ from node.depth while a drag previews nesting). */
  depth: number;
  indentWidth: number;
  /** Whether this entry is the page currently open in the editor. */
  active: boolean;
  onNavigate: (pageId: PageId) => void;
  onToggleCollapse: (treeId: TreeID) => void;
  onAddChild: (parent: PageId) => void;
}

export function TocRow({
  node,
  depth,
  indentWidth,
  active,
  onNavigate,
  onToggleCollapse,
  onAddChild,
}: TocRowProps): React.ReactElement {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: node.treeId,
  });

  const entry = node.entry;
  // Narrow once into a const so the closures below stay type-safe.
  const pageId: PageId | null = entry.kind === "page" ? entry.pageId : null;
  const gmOnly = entry.visibility === "gmOnly";

  return (
    <div
      ref={setNodeRef}
      style={{
        transform: CSS.Translate.toString(transform),
        transition,
        paddingLeft: depth * indentWidth + ROW_INDENT_BASE,
      }}
      className={[
        "group flex items-center gap-1 rounded-md py-1 pr-1 text-sm",
        isDragging ? "opacity-40" : "",
        active ? "bg-primary/10 text-foreground" : "text-foreground/80 hover:bg-primary/5",
      ].join(" ")}
    >
      {node.hasChildren ? (
        <button
          type="button"
          aria-label={node.collapsed ? "Expand" : "Collapse"}
          onClick={() => onToggleCollapse(node.treeId)}
          className="flex size-4 shrink-0 items-center justify-center rounded text-muted-foreground/70 hover:text-foreground"
        >
          {node.collapsed ? (
            <ChevronRight className="size-3.5" />
          ) : (
            <ChevronDown className="size-3.5" />
          )}
        </button>
      ) : (
        <span className="size-4 shrink-0" aria-hidden="true" />
      )}

      <button
        type="button"
        onClick={() => (pageId !== null ? onNavigate(pageId) : onToggleCollapse(node.treeId))}
        className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
      >
        {pageId !== null ? (
          <FileText className="size-4 shrink-0 text-muted-foreground" />
        ) : (
          <Folder className="size-4 shrink-0 text-bronze" />
        )}
        <span className="truncate font-sans">{entry.title ?? "Untitled"}</span>
      </button>

      {gmOnly ? (
        <EyeOff className="size-3.5 shrink-0 text-muted-foreground/50" aria-label="GM only" />
      ) : null}

      {pageId !== null ? (
        <button
          type="button"
          aria-label="Add sub-page"
          onClick={() => onAddChild(pageId)}
          className="flex size-5 shrink-0 items-center justify-center rounded text-muted-foreground/60 opacity-0 transition-opacity group-hover:opacity-100 hover:text-primary"
        >
          <Plus className="size-3.5" />
        </button>
      ) : null}

      <button
        type="button"
        aria-label="Drag to reorder"
        className="flex size-5 shrink-0 cursor-grab items-center justify-center rounded text-muted-foreground/40 opacity-0 transition-opacity group-hover:opacity-100 hover:text-muted-foreground"
        {...attributes}
        {...listeners}
      >
        <GripVertical className="size-3.5" />
      </button>
    </div>
  );
}
