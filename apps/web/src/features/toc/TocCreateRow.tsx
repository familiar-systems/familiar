// The inline "name your new page" input, rendered transiently in the tree at the
// root level or as a child of the row whose "+" was clicked. Enter submits,
// Escape or blur cancels. Not a sortable item: it is throwaway UI, removed once
// the page is created (the real node then arrives over the ToC sync).

import { useState } from "react";

import { ROW_INDENT_BASE } from "./tree-utils";

interface TocCreateRowProps {
  depth: number;
  indentWidth: number;
  busy: boolean;
  onSubmit: (name: string) => void;
  onCancel: () => void;
}

export function TocCreateRow({
  depth,
  indentWidth,
  busy,
  onSubmit,
  onCancel,
}: TocCreateRowProps): React.ReactElement {
  const [name, setName] = useState("");

  return (
    <div className="py-0.5 pr-1" style={{ paddingLeft: depth * indentWidth + ROW_INDENT_BASE }}>
      <input
        autoFocus
        value={name}
        disabled={busy}
        placeholder="Page name"
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && name.trim() !== "") {
            onSubmit(name.trim());
          } else if (e.key === "Escape") {
            onCancel();
          }
        }}
        // Blur cancels, but not while the create request is in flight (the input
        // disables, which would otherwise fire a cancel and drop the pending row).
        onBlur={() => {
          if (!busy) onCancel();
        }}
        className="w-full rounded-md bg-background/80 px-2 py-1 text-sm text-foreground ring-1 ring-primary/30 outline-none placeholder:text-muted-foreground/60 focus:ring-primary/60 disabled:opacity-60"
      />
    </div>
  );
}
