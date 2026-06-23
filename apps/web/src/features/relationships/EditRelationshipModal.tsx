// The edit-relationship flow, ported from the design-system wireframe (copy +
// mechanics kept, em-dashes scrubbed, chrome restyled to Tailwind). Clicking a
// relationship row opens this; it applies one of the four lifecycle operations
// (supersede / end / retcon / delete) plus an always-present visibility change.
//
// Presentational on purpose: the row, the session list, and one `onSubmit`
// callback arrive as props, so every op is play-testable with a spied callback and
// no socket (the same split as the create modal). useEditRelationship binds the
// network.
//
// The five ops map onto three HTTP shapes, not one /op RPC (the REST surface is
// resource-oriented): supersede mints a new row via POST with `supersedes`; end /
// retcon / a visibility change are a PATCH; delete is a DELETE. The modal assembles
// the right `EditSubmit` and the connector routes it.
//
// Smart submit: the visibility toggle is independent of the radio op. A predicate
// edit under Supersede carries the new visibility into the new row; End/Retcon fold
// it into the same PATCH; and changing only visibility (Supersede selected, nothing
// edited) becomes a plain visibility PATCH labelled "Update visibility". Submit is
// disabled only when nothing changed.

import type {
  CreateRelationshipRequest,
  PageId,
  PatchRelationshipRequest,
  RelationshipView,
  SessionId,
  SessionsResponse,
  Visibility,
} from "@familiar-systems/types-campaign";
import { X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { EntityChip, VisibilityToggle } from "./relationshipChrome";

// What the modal hands back on submit: one of the three relationship-edit HTTP
// shapes, fully assembled (the modal holds every field). The connector dispatches
// on `kind` to POST / PATCH / DELETE.
export type EditSubmit =
  | { kind: "supersede"; body: CreateRelationshipRequest }
  | { kind: "patch"; body: PatchRelationshipRequest }
  | { kind: "delete" };

type EditOp = "supersede" | "end" | "retcon" | "delete";

const OP_ORDER: readonly EditOp[] = ["supersede", "end", "retcon", "delete"];

// Op card copy, ported verbatim from the wireframe (em-dashes scrubbed to commas /
// periods). The tag names the kind of change: evolution (the fiction moved),
// correction (the fiction never held), destructive (the row was a mistake).
const OP_META: Record<EditOp, { label: string; tag: string; desc: string; destructive?: boolean }> =
  {
    supersede: {
      label: "Supersede",
      tag: "evolution",
      desc: "Time moved forward. Both facts are real, this one stops being true, a new one takes its place. Both rows stay in the database; prior snapshots still see the old one.",
    },
    end: {
      label: "End",
      tag: "evolution",
      desc: "This is no longer true. No replacement is created. Invalidated as superseded; prior snapshots still see it.",
    },
    retcon: {
      label: "Retcon",
      tag: "correction",
      desc: "This was never true in the fiction, even though it was established in play. Excluded from historical snapshots, kept in the database as part of the tapestry.",
    },
    delete: {
      label: "Delete",
      tag: "destructive",
      desc: "Hard delete, no audit trail. Use only when this relationship should never have existed in the database: mistaken AI acceptance, test data.",
      destructive: true,
    },
  };

interface EditRelationshipModalProps {
  subjectName: string;
  subjectPageId: PageId;
  view: RelationshipView;
  sessions: SessionsResponse;
  onSubmit: (submit: EditSubmit) => Promise<void>;
  onClose: () => void;
}

export function EditRelationshipModal({
  subjectName,
  subjectPageId,
  view,
  sessions,
  onSubmit,
  onClose,
}: EditRelationshipModalProps): React.ReactElement {
  const [op, setOp] = useState<EditOp>("supersede");
  // Predicates are immutable per row, so editing them = superseding: the inputs
  // start at the current pair and a change is what makes Supersede actionable.
  const [forward, setForward] = useState(view.predicate);
  const [reverse, setReverse] = useState(view.predicate_reverse);
  const [asOf, setAsOf] = useState<SessionId | null>(sessions.current?.id ?? null);
  const [visibility, setVisibility] = useState<Visibility>(view.visibility);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const dialogRef = useRef<HTMLDivElement>(null);
  // Submit is one request, but Enter+click can still double-fire within a tick.
  const submittingRef = useRef(false);

  // Move focus into the dialog on open (the selected op card is the natural first
  // stop, but it can be disabled, so focus the dialog itself).
  useEffect(() => {
    dialogRef.current?.focus();
  }, []);

  // Escape dismisses the dialog (never mid-request). The edit modal has no
  // typeahead dropdowns, so this is the single, simple Escape authority.
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape" && !busy) onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [busy, onClose]);

  const alreadyInvalidated = view.invalidation !== null;
  const hasSessions = sessions.current !== null;
  // Supersede / End originate or end at a session and act on a live row; Retcon
  // acts on a live row but needs no session; Delete is always available.
  const available: Record<EditOp, boolean> = {
    supersede: hasSessions && !alreadyInvalidated,
    end: hasSessions && !alreadyInvalidated,
    retcon: !alreadyInvalidated,
    delete: true,
  };

  // The new origin / end session can't precede the fact's own origin (the backend
  // enforces this; the picker just doesn't offer earlier sessions).
  const originOrdinal = view.origin.kind === "session" ? view.origin.content.ordinal : null;
  const availableSessions = sessions.sessions.filter(
    (s) => originOrdinal === null || s.ordinal >= originOrdinal,
  );
  const ordinalOf = (id: SessionId): number | null =>
    sessions.sessions.find((s) => s.id === id)?.ordinal ?? null;

  const predicateChanged =
    forward.trim() !== view.predicate || reverse.trim() !== view.predicate_reverse;
  const visibilityChanged = visibility !== view.visibility;
  const vis = visibilityChanged ? visibility : null;

  // Resolve the selected op + the current field state into the action to submit
  // and the button label. `null` submit means nothing to do (disabled). A change
  // to visibility alone, when the lifecycle op isn't actionable, falls back to a
  // plain visibility PATCH.
  function visibilityFallback(opLabel: string): { submit: EditSubmit | null; label: string } {
    if (visibilityChanged) {
      return {
        submit: { kind: "patch", body: { visibility, invalidation: null } },
        label: "Update visibility",
      };
    }
    return { submit: null, label: opLabel };
  }

  function computeAction(): { submit: EditSubmit | null; label: string } {
    switch (op) {
      case "delete":
        return { submit: { kind: "delete" }, label: "Delete permanently" };
      case "retcon":
        if (!available.retcon) return visibilityFallback("Retcon");
        return {
          submit: {
            kind: "patch",
            body: { invalidation: { reason: "retconned", as_of: null }, visibility: vis },
          },
          label: "Retcon",
        };
      case "end":
        if (!available.end || asOf === null) return visibilityFallback("End");
        return {
          submit: {
            kind: "patch",
            body: { invalidation: { reason: "superseded", as_of: asOf }, visibility: vis },
          },
          label: `End to S${ordinalOf(asOf)}`,
        };
      case "supersede":
        if (available.supersede && predicateChanged && asOf !== null) {
          return {
            submit: {
              kind: "supersede",
              body: {
                subject_page_id: subjectPageId,
                other_page_id: view.other.id,
                predicate_forward: forward.trim(),
                predicate_reverse: reverse.trim(),
                visibility,
                origin: { kind: "session", content: asOf },
                supersedes: view.id,
              },
            },
            label: `Supersede to S${ordinalOf(asOf)}`,
          };
        }
        return visibilityFallback("Supersede");
    }
  }

  const action = computeAction();
  const canSubmit = action.submit !== null && !busy;
  const destructive = op === "delete";

  async function submit(): Promise<void> {
    if (action.submit === null || busy || submittingRef.current) return;
    submittingRef.current = true;
    setBusy(true);
    setError(null);
    try {
      await onSubmit(action.submit);
      // Success unmounts this modal (the connector closes + refetches); no reset.
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to update relationship.");
      setBusy(false);
      submittingRef.current = false;
    }
  }

  const originText =
    view.origin.kind === "prior" ? "Prior" : `Session ${view.origin.content.ordinal}`;

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-foreground/20 px-4 pt-[6vh] backdrop-blur-sm"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !busy) onClose();
      }}
    >
      {/* Four op cards plus a panel make this taller than the create modal, so the
          dialog scrolls internally rather than pushing the submit button off-screen.
          No typeahead dropdowns here, so it needs no overflow-visible. */}
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="edit-relationship-title"
        tabIndex={-1}
        className="max-h-[88vh] w-full max-w-xl overflow-y-auto rounded-2xl border border-foreground/10 bg-background/95 p-5 shadow-2xl shadow-primary/10 backdrop-blur-md focus:outline-none"
      >
        <div className="flex items-baseline gap-2">
          <h2
            id="edit-relationship-title"
            className="font-display text-lg font-semibold text-foreground"
          >
            Edit relationship
          </h2>
          <button
            type="button"
            aria-label="Close"
            onClick={onClose}
            disabled={busy}
            className="ml-auto flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-foreground/5 hover:text-foreground disabled:opacity-50"
          >
            <X className="size-4" />
          </button>
        </div>

        {/* What is being edited. */}
        <p className="mt-1 flex flex-wrap items-baseline gap-x-1.5 gap-y-1 border-b border-foreground/10 pb-3 font-sans text-[13px] text-muted-foreground italic">
          <EntityChip name={subjectName} />
          <span className="text-foreground/70">{view.predicate}</span>
          <EntityChip name={view.other.name} />
          <span className="ml-1 font-sans text-[11px] tracking-wide text-muted-foreground/80 not-italic">
            · origin {originText} · {view.visibility === "gm" ? "GM only" : "visible to players"}
          </span>
        </p>

        {/* Op selector. */}
        <div className="mt-4 grid gap-2" role="radiogroup" aria-label="Operation">
          {OP_ORDER.map((o) => (
            <OpCard
              key={o}
              meta={OP_META[o]}
              selected={op === o}
              disabled={!available[o]}
              onSelect={() => setOp(o)}
            />
          ))}
        </div>

        {/* Per-op panel. */}
        <div className="mt-4 border-t border-foreground/10 pt-4">
          {op === "supersede" ? (
            available.supersede ? (
              <SupersedePanel
                subjectName={subjectName}
                otherName={view.other.name}
                forward={forward}
                reverse={reverse}
                busy={busy}
                onForward={setForward}
                onReverse={setReverse}
                asOf={asOf}
                sessions={availableSessions}
                current={sessions.current}
                onAsOf={setAsOf}
                predicate={view.predicate}
                asOfOrdinal={asOf !== null ? ordinalOf(asOf) : null}
              />
            ) : (
              <UnavailableNote hasSessions={hasSessions} alreadyInvalidated={alreadyInvalidated} />
            )
          ) : null}

          {op === "end" ? (
            <div className="flex flex-col gap-3">
              <div className="flex items-center gap-2">
                <label
                  htmlFor="edit-relationship-end-asof"
                  className="font-sans text-[10px] tracking-wide text-muted-foreground uppercase"
                >
                  Ended in
                </label>
                <AsOfSelect
                  id="edit-relationship-end-asof"
                  asOf={asOf}
                  sessions={availableSessions}
                  current={sessions.current}
                  busy={busy}
                  onAsOf={setAsOf}
                />
              </div>
              <p className="font-sans text-[13px] text-muted-foreground italic">
                <em className="text-foreground/80">{view.predicate}</em> will be marked{" "}
                <strong className="font-semibold text-foreground not-italic">
                  Ended S{asOf !== null ? ordinalOf(asOf) : "?"}
                </strong>
                . It stays visible in snapshots of earlier sessions. No new relationship is created.
              </p>
            </div>
          ) : null}

          {op === "retcon" ? (
            <p className="font-sans text-[13px] text-muted-foreground italic">
              This relationship will be marked{" "}
              <strong className="font-semibold text-foreground not-italic">
                never true in the fiction
              </strong>
              . It vanishes from historical snapshots but remains in the database as part of the
              campaign's record of decisions.
            </p>
          ) : null}

          {op === "delete" ? (
            <p className="font-sans text-[13px] text-red-700 italic dark:text-red-400">
              <strong className="font-semibold not-italic">This cannot be undone.</strong> The row
              will be removed from the database with no trace. Use only for mistakes: if this
              relationship was once true and is no longer, use{" "}
              <strong className="not-italic">End</strong> instead.
            </p>
          ) : null}
        </div>

        {/* Visibility: always present, independent of the op. */}
        <div className="mt-4 flex items-center gap-3 border-t border-foreground/10 pt-4">
          <span className="font-sans text-[10px] tracking-wide text-muted-foreground uppercase">
            Visibility
          </span>
          <VisibilityToggle value={visibility} disabled={busy} onChange={setVisibility} />
        </div>

        {error !== null ? (
          <p className="mt-3 font-sans text-xs text-red-700 dark:text-red-400">{error}</p>
        ) : null}

        <div className="mt-5 flex items-center gap-2 border-t border-foreground/10 pt-4">
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            className="rounded-full border border-foreground/20 px-4 py-2 font-sans text-sm font-medium text-muted-foreground transition-colors hover:bg-foreground/5 disabled:opacity-50"
          >
            Cancel
          </button>
          <div className="flex-1" />
          <button
            type="button"
            onClick={() => void submit()}
            disabled={!canSubmit}
            className={[
              "inline-flex items-center gap-2 rounded-full px-5 py-2 font-sans text-sm font-medium text-white shadow-md transition-colors disabled:cursor-not-allowed disabled:opacity-50",
              destructive
                ? "bg-red-700 shadow-red-700/25 hover:bg-red-700/90"
                : "bg-gold shadow-gold/25 hover:bg-gold/90",
            ].join(" ")}
          >
            {busy ? "Saving..." : action.label}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

function OpCard({
  meta,
  selected,
  disabled,
  onSelect,
}: {
  meta: { label: string; tag: string; desc: string; destructive?: boolean };
  selected: boolean;
  disabled: boolean;
  onSelect: () => void;
}): React.ReactElement {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      disabled={disabled}
      onClick={onSelect}
      className={[
        "group flex items-start gap-3 rounded-2xl border p-3 text-left transition-all duration-200 disabled:cursor-not-allowed disabled:opacity-40",
        selected
          ? meta.destructive
            ? "border-red-600/50 bg-red-600/10"
            : "border-gold/60 bg-bronze-muted/30 shadow-md shadow-gold/10"
          : "border-foreground/10 bg-background/40 enabled:hover:border-primary/30 enabled:hover:bg-foreground/[0.02]",
      ].join(" ")}
    >
      <RadioPip selected={selected} destructive={meta.destructive ?? false} />
      <span className="flex-1 space-y-1">
        <span className="flex flex-wrap items-baseline justify-between gap-2">
          <span className="font-display text-base font-semibold text-foreground">{meta.label}</span>
          <span className="font-sans text-[9.5px] tracking-wide text-muted-foreground uppercase">
            {meta.tag}
          </span>
        </span>
        <span className="block font-sans text-[13px] leading-snug text-muted-foreground italic">
          {meta.desc}
        </span>
      </span>
    </button>
  );
}

function RadioPip({
  selected,
  destructive,
}: {
  selected: boolean;
  destructive: boolean;
}): React.ReactElement {
  const fill = destructive ? "border-red-700 bg-red-700" : "border-gold bg-gold";
  return (
    <span
      aria-hidden="true"
      className={[
        "mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full border transition-colors",
        selected ? fill : "border-foreground/20 bg-background",
      ].join(" ")}
    >
      {selected ? <span className="size-2 rounded-full bg-white" /> : null}
    </span>
  );
}

function SupersedePanel({
  subjectName,
  otherName,
  forward,
  reverse,
  busy,
  onForward,
  onReverse,
  asOf,
  sessions,
  current,
  onAsOf,
  predicate,
  asOfOrdinal,
}: {
  subjectName: string;
  otherName: string;
  forward: string;
  reverse: string;
  busy: boolean;
  onForward: (v: string) => void;
  onReverse: (v: string) => void;
  asOf: SessionId | null;
  sessions: SessionsResponse["sessions"];
  current: SessionsResponse["current"];
  onAsOf: (id: SessionId) => void;
  predicate: string;
  asOfOrdinal: number | null;
}): React.ReactElement {
  return (
    <div className="flex flex-col gap-3">
      <Edge name={subjectName}>
        <input
          type="text"
          aria-label="Forward predicate"
          autoComplete="off"
          value={forward}
          disabled={busy}
          onChange={(e) => onForward(e.target.value)}
          className="w-full rounded border border-foreground/18 bg-background/60 px-2 py-1 font-sans text-[15px] text-foreground italic focus:border-gold/60 focus:ring-2 focus:ring-gold/20 focus:outline-none disabled:opacity-50"
        />
      </Edge>
      <Edge name={otherName}>
        <input
          type="text"
          aria-label="Reverse predicate"
          autoComplete="off"
          value={reverse}
          disabled={busy}
          onChange={(e) => onReverse(e.target.value)}
          className="w-full rounded border border-foreground/18 bg-background/60 px-2 py-1 font-sans text-[15px] text-foreground italic focus:border-gold/60 focus:ring-2 focus:ring-gold/20 focus:outline-none disabled:opacity-50"
        />
      </Edge>
      <div className="flex items-center gap-2">
        <label
          htmlFor="edit-relationship-sup-asof"
          className="font-sans text-[10px] tracking-wide text-muted-foreground uppercase"
        >
          As of
        </label>
        <AsOfSelect
          id="edit-relationship-sup-asof"
          asOf={asOf}
          sessions={sessions}
          current={current}
          busy={busy}
          onAsOf={onAsOf}
        />
      </div>
      <p className="font-sans text-[13px] text-muted-foreground italic">
        The current row, <em className="text-foreground/80">{predicate}</em>, will be marked{" "}
        <strong className="font-semibold text-foreground not-italic">
          Ended S{asOfOrdinal ?? "?"}
        </strong>{" "}
        and remain visible in prior snapshots. The new row takes effect from{" "}
        <strong className="font-semibold text-foreground not-italic">
          Session {asOfOrdinal ?? "?"}
        </strong>{" "}
        forward.
      </p>
    </div>
  );
}

// One direction of the relationship as an editable edge: the page, an arrow, and
// the predicate input that reads from it.
function Edge({ name, children }: { name: string; children: React.ReactNode }): React.ReactElement {
  return (
    <div className="grid grid-cols-[max-content_auto_1fr] items-center gap-2">
      <EntityChip name={name} />
      <span className="font-sans text-[13px] text-gold/70">to</span>
      {children}
    </div>
  );
}

function AsOfSelect({
  id,
  asOf,
  sessions,
  current,
  busy,
  onAsOf,
}: {
  id: string;
  asOf: SessionId | null;
  sessions: SessionsResponse["sessions"];
  current: SessionsResponse["current"];
  busy: boolean;
  onAsOf: (id: SessionId) => void;
}): React.ReactElement {
  return (
    <select
      id={id}
      value={asOf ?? ""}
      disabled={busy}
      onChange={(e) => {
        const match = sessions.find((s) => s.id === e.target.value);
        if (match !== undefined) onAsOf(match.id);
      }}
      className="rounded border border-gold/40 bg-background/60 px-2 py-1 font-sans text-xs text-foreground focus:border-gold/60 focus:outline-none disabled:opacity-50"
    >
      {sessions.map((s) => (
        <option key={s.id} value={s.id}>
          Session {s.ordinal}
          {current !== null && s.id === current.id ? " (current)" : ""}
        </option>
      ))}
    </select>
  );
}

function UnavailableNote({
  hasSessions,
  alreadyInvalidated,
}: {
  hasSessions: boolean;
  alreadyInvalidated: boolean;
}): React.ReactElement {
  const text = alreadyInvalidated
    ? "This relationship is already invalidated. You can delete it or change its visibility."
    : !hasSessions
      ? "Superseding needs a session to originate the new fact in. This campaign has no sessions yet, so end and supersede are unavailable."
      : "This operation is unavailable for this relationship.";
  return <p className="font-sans text-[13px] text-muted-foreground italic">{text}</p>;
}
