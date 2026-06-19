// The ToC "New" flow as a modal: choose what to create, then name it. Replaces
// the old inline create-row. Two steps in one dialog: a picker (the "New menu"
// from the design system) and a naming step with the cursor in the field. The
// set of creatable kinds and their per-row metadata come from NEW_MENU, which is
// keyed off the generated PageKind so it can't drift from the server.
//
// Rendered through a portal to document.body: the ToC <aside> sets a
// backdrop-filter, which would otherwise become the containing block for this
// fixed overlay and trap it inside the 16rem sidebar.

import type { PageKind } from "@familiar-systems/types-campaign";
import { ChevronLeft } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { NEW_MENU_ROWS, type NewMenuColor, type NewMenuEntry } from "./newMenu";

// Concrete picker classes per accent token (`NewMenuEntry.color`). Literal
// strings so Tailwind can see them; newMenu.ts only chooses the token, so a new
// kind is a one-line data edit there, not three ternaries here.
const ROW_ACCENT: Record<NewMenuColor, { row: string; iconBox: string; label: string }> = {
  gold: {
    row: "hover:bg-gold/10",
    iconBox: "bg-gold/15 text-gold",
    label: "text-gold",
  },
  primary: {
    row: "hover:bg-primary/5",
    iconBox: "bg-primary/10 text-primary",
    label: "text-foreground",
  },
};

interface NewPageModalProps {
  /**
   * Create the chosen kind with the given name. Every kind requires a non-blank
   * name (the modal gates an empty submit). Throws on failure so the modal can
   * surface it; resolves once navigation is under way (the parent then unmounts
   * this modal).
   */
  onSubmit: (kind: PageKind, name: string | null) => Promise<void>;
  onClose: () => void;
}

type Chosen = { kind: PageKind; entry: NewMenuEntry };

export function NewPageModal({ onSubmit, onClose }: NewPageModalProps): React.ReactElement {
  const [chosen, setChosen] = useState<Chosen | null>(null);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  // Synchronous double-submit guard: `busy` is async React state, so a same-tick
  // Enter-keydown + button-click (or a fast double-click) can both clear the
  // `canSubmit` gate before the first `setBusy(true)` re-renders -- firing two
  // POSTs and creating two pages. A ref flips synchronously, closing that window.
  const submittingRef = useRef(false);

  // Escape closes the dialog, but never mid-request: an in-flight create should
  // not be orphaned with its UI gone.
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape" && !busy) onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [busy, onClose]);

  // Land the cursor in the name field on reaching the naming step; with a
  // default (session) select it so the GM can type straight over it.
  useEffect(() => {
    if (chosen === null) return;
    const el = inputRef.current;
    if (el === null) return;
    el.focus();
    if (el.value !== "") el.select();
  }, [chosen]);

  function choose(row: Chosen): void {
    setChosen(row);
    setName(row.entry.defaultName);
    setError(null);
  }

  const trimmed = name.trim();
  const canSubmit = chosen !== null && (!chosen.entry.nameRequired || trimmed !== "") && !busy;

  async function submit(): Promise<void> {
    if (chosen === null || !canSubmit || submittingRef.current) return;
    submittingRef.current = true;
    // All page kinds require a name today (the gate above enforces non-empty), so
    // `value` is the trimmed string; the per-kind `nameRequired` keeps the door
    // open for a future optional-name kind (which would send null when blank).
    const value = chosen.entry.nameRequired ? trimmed : trimmed || null;
    setBusy(true);
    setError(null);
    try {
      await onSubmit(chosen.kind, value);
      // Success unmounts this modal (the parent clears its state); no reset.
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create.");
      setBusy(false);
      submittingRef.current = false;
    }
  }

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-foreground/20 px-4 pt-[12vh] backdrop-blur-sm"
      onMouseDown={(e) => {
        // Backdrop click closes; clicks inside the card have a different target.
        if (e.target === e.currentTarget && !busy) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="new-page-title"
        className="w-full max-w-md overflow-hidden rounded-2xl border border-foreground/10 bg-background/95 p-2 shadow-2xl shadow-primary/10 backdrop-blur-md"
      >
        {chosen === null ? (
          <>
            <h2
              id="new-page-title"
              className="px-3 pt-3 pb-2 font-display text-lg font-semibold text-foreground"
            >
              What are you creating?
            </h2>
            <div className="flex flex-col">
              {NEW_MENU_ROWS.map(({ kind, entry }) => {
                const Icon = entry.icon;
                const accent = ROW_ACCENT[entry.color];
                return (
                  <button
                    key={kind}
                    type="button"
                    onClick={() => choose({ kind, entry })}
                    className={[
                      "grid grid-cols-[26px_1fr] items-center gap-3 rounded-xl px-3 py-3 text-left transition-colors",
                      accent.row,
                    ].join(" ")}
                  >
                    <span
                      className={[
                        "flex size-[26px] items-center justify-center rounded-md",
                        accent.iconBox,
                      ].join(" ")}
                    >
                      <Icon className="size-3.5" strokeWidth={1.75} />
                    </span>
                    <span className="flex min-w-0 flex-col">
                      <span
                        className={[
                          "font-display text-[15px] leading-tight font-semibold",
                          accent.label,
                        ].join(" ")}
                      >
                        {entry.label}
                      </span>
                      <span className="truncate font-sans text-xs leading-snug text-muted-foreground italic">
                        {entry.subtitle}
                      </span>
                    </span>
                  </button>
                );
              })}
            </div>
          </>
        ) : (
          <>
            <div className="flex items-center gap-1 px-1 pt-1">
              <button
                type="button"
                aria-label="Back"
                onClick={() => {
                  setChosen(null);
                  setError(null);
                }}
                disabled={busy}
                className="flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-foreground/5 hover:text-foreground disabled:opacity-50"
              >
                <ChevronLeft className="size-4" />
              </button>
              <h2
                id="new-page-title"
                className="font-display text-lg font-semibold text-foreground"
              >
                {chosen.entry.label}
              </h2>
            </div>
            <div className="space-y-2 p-3">
              <div className="flex items-baseline justify-between">
                <label htmlFor="new-page-name" className="text-sm font-medium text-foreground">
                  Name
                </label>
                <span className="text-xs tracking-wider text-muted-foreground uppercase">
                  {chosen.entry.nameRequired ? "Required" : "Optional"}
                </span>
              </div>
              <input
                id="new-page-name"
                ref={inputRef}
                type="text"
                value={name}
                disabled={busy}
                placeholder={
                  chosen.entry.nameRequired ? "Name this page" : chosen.entry.defaultName
                }
                maxLength={120}
                onChange={(e) => setName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    void submit();
                  }
                }}
                className="w-full rounded-xl border border-foreground/10 bg-background/60 px-4 py-3 font-display text-xl text-foreground placeholder:text-muted-foreground/60 focus:border-gold/50 focus:ring-2 focus:ring-gold/20 focus:outline-none disabled:opacity-60"
              />
              {error !== null ? (
                <p className="text-xs text-red-700 dark:text-red-400">{error}</p>
              ) : null}
              <div className="flex justify-end pt-2">
                <button
                  type="button"
                  onClick={() => void submit()}
                  disabled={!canSubmit}
                  className="inline-flex items-center gap-2 rounded-full bg-gold px-5 py-2 text-sm font-medium text-white shadow-md shadow-gold/25 transition-colors hover:bg-gold/90 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  {busy ? "Creating..." : "Create"}
                </button>
              </div>
            </div>
          </>
        )}
      </div>
    </div>,
    document.body,
  );
}
