// Keyboard navigation for a typeahead dropdown, factored out so the object slot
// (async entity search) and the predicate slot (client-filtered vocab) share one
// arrow/enter handler instead of duplicating the fiddly index math. The parent
// owns the items and the commit logic; this owns only open/active state. Escape is
// deliberately NOT handled here: the modal owns it at the document level so one
// authority decides "close the dropdown" vs "close the dialog".
//
// `itemCount` is the *rendered* row count and must include the trailing
// "create new" / "use custom" affordance row, so it is reachable by keyboard like
// any match. `resetKey` identifies the *current list* (the query string): the active
// row snaps back to the top whenever it changes, so a stale highlight can't commit a
// row that shifted underneath it. Keying on the query, not the row count, catches the
// case the count misses - retyping to a different list that happens to be the same
// length (count unchanged, contents wholly different).

import { useEffect, useState } from "react";

export interface Typeahead {
  open: boolean;
  setOpen: (open: boolean) => void;
  activeIndex: number;
  setActiveIndex: (index: number) => void;
  onKeyDown: (e: React.KeyboardEvent) => void;
}

export function useTypeahead(
  itemCount: number,
  resetKey: string,
  opts: { onPick: (index: number) => void },
): Typeahead {
  const { onPick } = opts;
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);

  // A new query renders a new list; point the highlight back at the top so a stale
  // index can't select a row that has shifted underneath it.
  useEffect(() => {
    setActiveIndex(0);
  }, [resetKey]);

  const onKeyDown = (e: React.KeyboardEvent): void => {
    if (!open || itemCount === 0) return;
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setActiveIndex((i) => (i + 1) % itemCount);
        break;
      case "ArrowUp":
        e.preventDefault();
        setActiveIndex((i) => (i - 1 + itemCount) % itemCount);
        break;
      case "Enter":
        if (activeIndex >= 0 && activeIndex < itemCount) {
          e.preventDefault();
          onPick(activeIndex);
          // Every commit in this modal closes the dropdown; doing it here keeps
          // the pick handlers from having to reference this hook's own setOpen.
          setOpen(false);
        }
        break;
    }
  };

  return { open, setOpen, activeIndex, setActiveIndex, onKeyDown };
}

export interface TypeaheadSlot {
  ta: Typeahead;
  /** Whether to render the trailing "create new" / "use custom" affordance row. */
  showExtra: boolean;
  /** Rendered row count (matches + the extra row), already fed to the keyboard nav. */
  itemCount: number;
  /** Commit the row at `index`: a match, or - past the matches - the extra action. */
  onPick: (index: number) => void;
}

// The bookkeeping both create-modal slots share: from a list of `items` and the
// current `query`, decide whether to show a trailing "extra" row (a non-empty query
// with no exact match by `keyOf`), size the list for keyboard nav, and route a pick
// to either the matched item or the extra action. The keyboard (`ta.onKeyDown`) and
// the mouse handlers both call the same `onPick`, so the two paths can't diverge.
export function useTypeaheadSlot<T>(opts: {
  items: readonly T[];
  query: string;
  /** The field the query matches against, used for the exact-match test. */
  keyOf: (item: T) => string;
  onPickItem: (item: T) => void;
  /** The trailing-row action, or null for a slot with no "create new" affordance. */
  onPickExtra: (() => void) | null;
}): TypeaheadSlot {
  const { items, query, keyOf, onPickItem, onPickExtra } = opts;
  const queryTrim = query.trim().toLowerCase();
  const exact = items.some((item) => keyOf(item).toLowerCase() === queryTrim);
  const showExtra = onPickExtra !== null && queryTrim !== "" && !exact;
  const itemCount = items.length + (showExtra ? 1 : 0);
  const onPick = (index: number): void => {
    const item = items[index];
    if (item !== undefined) onPickItem(item);
    // The trailing row is past the matches: there is no `item`, so it is the extra.
    else onPickExtra?.();
  };
  const ta = useTypeahead(itemCount, query, { onPick });
  return { ta, showExtra, itemCount, onPick };
}
