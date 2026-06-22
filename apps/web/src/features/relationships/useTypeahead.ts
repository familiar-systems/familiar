// Keyboard navigation for a typeahead dropdown, factored out so the object slot
// (async entity search) and the predicate slot (client-filtered vocab) share one
// arrow/enter handler instead of duplicating the fiddly index math. The parent
// owns the items and the commit logic; this owns only open/active state. Escape is
// deliberately NOT handled here: the modal owns it at the document level so one
// authority decides "close the dropdown" vs "close the dialog".
//
// `itemCount` is the *rendered* row count and must include the trailing
// "create new" / "use custom" affordance row, so it is reachable by keyboard like
// any match. The active row resets to the top whenever the list changes (a fresh
// query renders a fresh list), matching the wireframe's per-render reset.

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
  opts: { onPick: (index: number) => void },
): Typeahead {
  const { onPick } = opts;
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);

  // A new query renders a new list; point the highlight back at the top so a stale
  // index can't select a row that has shifted underneath it.
  useEffect(() => {
    setActiveIndex(0);
  }, [itemCount]);

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
