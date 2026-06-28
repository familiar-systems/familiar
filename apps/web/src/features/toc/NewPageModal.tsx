// The ToC "New" flow as a modal: choose what to create, then name it. Two steps
// in one dialog: a picker (the "New menu" from the design system) and a naming
// step with the cursor in the field. The set of creatable kinds and their
// per-row metadata come from NEW_MENU, keyed off the generated PageKind so it
// can't drift from the server.
//
// The shell is @familiar-systems/ui's Modal/Dialog (React Aria): it portals to
// document.body (escaping the ToC <aside>'s backdrop-filter containing block),
// traps focus, locks scroll, and dismisses on outside-press/Escape. It's held
// open during an in-flight create via isDismissable/isKeyboardDismissDisabled so
// a create is never orphaned with its UI gone.

import type { PageKind } from "@familiar-systems/types-campaign";
import { Button, Dialog, Modal, TextField } from "@familiar-systems/ui";
import { ChevronLeft } from "lucide-react";
import { useRef, useState } from "react";

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
  // Synchronous double-submit guard: `busy` is async React state, so a same-tick
  // Enter-keydown + button-click (or a fast double-click) can both clear the
  // `canSubmit` gate before the first `setBusy(true)` re-renders -- firing two
  // POSTs and creating two pages. A ref flips synchronously, closing that window.
  const submittingRef = useRef(false);

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

  return (
    <Modal
      isOpen
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      isDismissable={!busy}
      isKeyboardDismissDisabled={busy}
      className="max-w-md"
    >
      <Dialog aria-labelledby="new-page-title" className="outline-none">
        {chosen === null ? (
          <>
            <h2
              id="new-page-title"
              className="pb-2 font-display text-lg font-semibold text-foreground"
            >
              What are you creating?
            </h2>
            <div className="-mx-2 flex flex-col">
              {NEW_MENU_ROWS.map(({ kind, entry }) => {
                const Icon = entry.icon;
                const accent = ROW_ACCENT[entry.color];
                return (
                  <button
                    key={kind}
                    type="button"
                    onClick={() => choose({ kind, entry })}
                    className={[
                      "grid grid-cols-[26px_1fr] items-center gap-3 rounded-xl px-3 py-3 text-start transition-colors",
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
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void submit();
            }}
          >
            <div className="flex items-center gap-1 pb-3">
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
            <div className="space-y-2">
              <TextField
                label="Name"
                hint={chosen.entry.nameRequired ? "Required" : "Optional"}
                // The field mounts only once a kind is chosen, so autoFocus lands here
                // then; selecting the prefilled default name lets the GM type over it.
                autoFocus
                value={name}
                onChange={setName}
                isDisabled={busy}
                placeholder={
                  chosen.entry.nameRequired ? "Name this page" : chosen.entry.defaultName
                }
                inputProps={{
                  maxLength: 120,
                  className: "font-display text-xl",
                  onFocus: (e) => e.currentTarget.select(),
                }}
              />
              {error !== null ? (
                <p className="text-xs text-red-700 dark:text-red-400">{error}</p>
              ) : null}
              <div className="flex justify-end pt-2">
                <Button type="submit" isDisabled={!canSubmit}>
                  {busy ? "Creating..." : "Create"}
                </Button>
              </div>
            </div>
          </form>
        )}
      </Dialog>
    </Modal>
  );
}
