// The edit-relationship flow, ported from the design-system wireframe (copy +
// mechanics kept, em-dashes scrubbed, chrome restyled to Tailwind). Clicking a
// relationship row opens this. It edits the two orthogonal axes, knowledge first
// and factuality below, with corrections (retcon / delete) tucked in a drawer.
//
// Presentational on purpose: the row, the session list, and one `onSubmit` callback
// arrive as props, so every edit is play-testable with a spied callback and no
// socket (the same split as the create modal). useEditRelationship binds the network.
//
// The composed diff maps onto three HTTP shapes (the REST surface is
// resource-oriented): independent axis changes (knowledge set wholesale / superseded /
// retcon set or cleared) apply as one atomic PATCH; End-with-successor mints a new row
// via POST with `supersedes` (the successor carries the row's knowledge); delete is a
// DELETE. Knowledge is freely mutable - the control reveals, conceals, or re-hides.

import type {
  CreateRelationshipRequest,
  KnowledgeInput,
  KnowledgeView,
  PageId,
  PatchRelationshipRequest,
  RelationshipView,
  SessionId,
  SessionStampPatch,
  SessionsResponse,
  ViewSessionOrdinal,
} from "@familiar-systems/types-campaign";
import { RotateCcw, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { EntityChip, KnowledgeControl } from "./relationshipChrome";

// What the modal hands back on submit: one of the three relationship-edit HTTP
// shapes, fully assembled (the modal holds every field). The connector dispatches on
// `kind` to POST / PATCH / DELETE.
export type EditSubmit =
  | { kind: "supersede"; body: CreateRelationshipRequest }
  | { kind: "patch"; body: PatchRelationshipRequest }
  | { kind: "delete" };

type SessionRef = SessionsResponse["sessions"][number];

const set = (content: SessionId): SessionStampPatch => ({ kind: "set", content });
const clear: SessionStampPatch = { kind: "clear" };

/** The view's knowledge (ordinals) as an editable input (session ids). */
function knowledgeInputFromView(k: KnowledgeView, sessions: SessionRef[]): KnowledgeInput {
  if (k.kind === "public") return { kind: "public" };
  if (k.kind === "hidden") return { kind: "hidden" };
  const match = sessions.find((s) => s.ordinal === k.content.ordinal);
  // A broken reveal ordinal (no matching session) degrades to hidden rather than
  // crashing the modal; the backend FK makes this unreachable in practice.
  return match !== undefined ? { kind: "revealed", content: match.id } : { kind: "hidden" };
}

/** Whether two knowledge inputs are the same state (revealed compares its session). */
function sameKnowledge(a: KnowledgeInput, b: KnowledgeInput): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === "revealed" && b.kind === "revealed") return a.content === b.content;
  return true;
}

/** The session id whose ordinal matches a view point, or null. */
function sessionIdForOrdinal(
  o: ViewSessionOrdinal | null,
  sessions: SessionRef[],
): SessionId | null {
  if (o === null) return null;
  return sessions.find((s) => s.ordinal === o.ordinal)?.id ?? null;
}

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
  const allSessions = sessions.sessions;
  // No axis event may precede the fact's origin (the backend enforces this; the
  // pickers just don't offer earlier sessions).
  const originOrdinal = view.origin.kind === "session" ? view.origin.content.ordinal : null;
  const availableSessions = allSessions.filter(
    (s) => originOrdinal === null || s.ordinal >= originOrdinal,
  );
  const defaultAsOf = sessions.current?.id ?? availableSessions.at(-1)?.id ?? null;
  const ordinalOf = (id: SessionId): number | null =>
    allSessions.find((s) => s.id === id)?.ordinal ?? null;

  // Knowledge, freely mutable. `bornSecret` is the secret bit frozen at open: it tells
  // the control whether its "known" segment means plain "Public" or "Revealed at a
  // session" (a born-public fact reveals as Public; a secret one stamps a session).
  const bornSecret = view.knowledge.kind !== "public";
  const [knowledge, setKnowledge] = useState<KnowledgeInput>(() =>
    knowledgeInputFromView(view.knowledge, allSessions),
  );
  // Factuality.
  const [ended, setEnded] = useState(view.superseded !== null);
  const [endAsOf, setEndAsOf] = useState<SessionId | null>(
    () => sessionIdForOrdinal(view.superseded, allSessions) ?? defaultAsOf,
  );
  const [succForward, setSuccForward] = useState("");
  const [succReverse, setSuccReverse] = useState("");
  // Corrections.
  const [correctionsOpen, setCorrectionsOpen] = useState(view.retcon !== null);
  const [retconArmed, setRetconArmed] = useState(view.retcon !== null);
  const [retconAsOf, setRetconAsOf] = useState<SessionId | null>(
    () => sessionIdForOrdinal(view.retcon, allSessions) ?? defaultAsOf,
  );
  const [deleteArmed, setDeleteArmed] = useState(false);

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const dialogRef = useRef<HTMLDivElement>(null);
  const submittingRef = useRef(false);

  useEffect(() => {
    dialogRef.current?.focus();
  }, []);

  // The edit modal has no typeahead dropdowns, so Escape is the single, simple
  // authority: it dismisses the dialog (never mid-request).
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape" && !busy) onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [busy, onClose]);

  const hasSessions = availableSessions.length > 0;
  // Arming retcon dims the factuality axis (a retcon governs factuality wholesale).
  const factualityDisabled = busy || retconArmed;

  // --- Diff each axis into a stamp patch (null = unchanged) -----------------

  function knowledgeDiff(): KnowledgeInput | null {
    // The knowledge axis is set wholesale: emit the new state if it differs from the
    // opening one (a conceal Public -> Hidden, a reveal, a re-hide all count).
    const original = knowledgeInputFromView(view.knowledge, allSessions);
    return sameKnowledge(knowledge, original) ? null : knowledge;
  }

  function retconDiff(): SessionStampPatch | null {
    const wasOrdinal = view.retcon?.ordinal ?? null;
    if (retconArmed && retconAsOf !== null) {
      return ordinalOf(retconAsOf) !== wasOrdinal ? set(retconAsOf) : null;
    }
    return wasOrdinal !== null ? clear : null; // un-retcon
  }

  function supersededDiff(): SessionStampPatch | null {
    // Retcon dims factuality, so it is not edited while armed.
    if (retconArmed) return null;
    const wasOrdinal = view.superseded?.ordinal ?? null;
    if (ended && endAsOf !== null) {
      return ordinalOf(endAsOf) !== wasOrdinal ? set(endAsOf) : null;
    }
    return wasOrdinal !== null ? clear : null; // un-end
  }

  const fwdFilled = succForward.trim() !== "";
  const revFilled = succReverse.trim() !== "";
  const successorIntent = !retconArmed && ended && (fwdFilled || revFilled);

  function knowledgeLabel(k: KnowledgeInput): string {
    if (k.kind === "revealed") return `Reveal S${ordinalOf(k.content)}`;
    if (k.kind === "public") return "Show players";
    return "Conceal";
  }
  function supersededLabel(patch: SessionStampPatch): string {
    return patch.kind === "set" ? `End S${ordinalOf(patch.content)}` : "Un-end";
  }
  function retconLabel(patch: SessionStampPatch): string {
    return patch.kind === "set" ? `Retcon S${ordinalOf(patch.content)}` : "Un-retcon";
  }

  function computeAction(): { submit: EditSubmit | null; label: string } {
    if (deleteArmed) return { submit: { kind: "delete" }, label: "Delete permanently" };

    const knowledgePatch = knowledgeDiff();

    // End-with-successor mints a new row (the successor carries the knowledge); it is
    // mutually exclusive with retcon, which dims factuality.
    if (successorIntent) {
      if (!(fwdFilled && revFilled) || endAsOf === null) {
        return { submit: null, label: "Fill both successor predicates" };
      }
      const parts: string[] = [];
      if (knowledgePatch !== null) parts.push(knowledgeLabel(knowledgePatch));
      parts.push(`End S${ordinalOf(endAsOf)} · +successor`);
      return {
        submit: {
          kind: "supersede",
          body: {
            subject_page_id: subjectPageId,
            other_page_id: view.other.id,
            predicate_forward: succForward.trim(),
            predicate_reverse: succReverse.trim(),
            origin: { kind: "session", content: endAsOf },
            knowledge,
            supersedes: view.id,
          },
        },
        label: parts.join(" · "),
      };
    }

    const supersededPatch = supersededDiff();
    const retconPatch = retconDiff();
    if (knowledgePatch === null && supersededPatch === null && retconPatch === null) {
      return { submit: null, label: "No changes" };
    }
    const parts: string[] = [];
    if (knowledgePatch !== null) parts.push(knowledgeLabel(knowledgePatch));
    if (supersededPatch !== null) parts.push(supersededLabel(supersededPatch));
    if (retconPatch !== null) parts.push(retconLabel(retconPatch));
    return {
      submit: {
        kind: "patch",
        body: { knowledge: knowledgePatch, superseded: supersededPatch, retcon: retconPatch },
      },
      label: parts.join(" · "),
    };
  }

  const action = computeAction();
  const canSubmit = action.submit !== null && !busy;

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

  // --- Current-line + now-summaries (the persisted state) -------------------

  const originText =
    view.origin.kind === "prior" ? "Prior" : `Session ${view.origin.content.ordinal}`;
  const lifeWord =
    view.retcon !== null
      ? `retconned S${view.retcon.ordinal}`
      : view.superseded !== null
        ? `ended S${view.superseded.ordinal}`
        : "still true";
  const visWord =
    view.knowledge.kind === "revealed"
      ? `revealed S${view.knowledge.content.ordinal}`
      : view.knowledge.kind === "public"
        ? "public"
        : "GM-only";
  const factNow =
    view.superseded !== null
      ? `ended S${view.superseded.ordinal}`
      : `true from ${originText}, ongoing`;

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-foreground/20 px-4 pt-[6vh] backdrop-blur-sm"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !busy) onClose();
      }}
    >
      {/* Two axes plus a corrections drawer make this taller than the create modal,
          so the dialog scrolls internally rather than pushing submit off-screen. */}
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

        {/* Current line: what is being edited, and its persisted state. */}
        <p className="mt-1 flex flex-wrap items-baseline gap-x-1.5 gap-y-1 border-b border-foreground/10 pb-3 font-sans text-[13px] text-muted-foreground italic">
          <EntityChip name={subjectName} />
          <span className="text-foreground/70">{view.predicate}</span>
          <EntityChip name={view.other.name} />
          <span className="ml-1 font-sans text-[11px] tracking-wide text-muted-foreground/80 not-italic">
            · true of {originText} · {lifeWord} · {visWord}
          </span>
        </p>

        {/* Knowledge axis, first. */}
        <section className="mt-4">
          <div className="flex items-baseline gap-2">
            <h3 className="font-sans text-[10px] tracking-wide text-muted-foreground uppercase">
              To the players
            </h3>
            <span className="font-sans text-[11px] text-muted-foreground/70 italic">{visWord}</span>
          </div>
          <div className="mt-2">
            <KnowledgeControl
              value={knowledge}
              disabled={busy}
              bornSecret={bornSecret}
              sessions={availableSessions}
              onChange={setKnowledge}
            />
          </div>
        </section>

        {/* Factuality axis, below. */}
        <section className="mt-5 border-t border-foreground/10 pt-4">
          <div className="flex items-baseline gap-2">
            <h3 className="font-sans text-[10px] tracking-wide text-muted-foreground uppercase">
              In the fiction
            </h3>
            <span className="font-sans text-[11px] text-muted-foreground/70 italic">{factNow}</span>
          </div>

          <div
            role="radiogroup"
            aria-label="In the fiction"
            className="mt-2 inline-flex w-fit overflow-hidden rounded-lg border border-foreground/15"
          >
            <FactButton
              active={!ended}
              disabled={factualityDisabled}
              label="Ongoing"
              onClick={() => setEnded(false)}
            />
            <FactButton
              active={ended}
              disabled={factualityDisabled || !hasSessions}
              label="Ended"
              Icon={RotateCcw}
              onClick={() => setEnded(true)}
            />
          </div>
          {!hasSessions ? (
            <p className="mt-2 font-sans text-[12px] text-muted-foreground italic">
              This campaign has no sessions yet, so a fact can't be ended.
            </p>
          ) : null}

          {ended && !retconArmed ? (
            <div className="mt-3 flex flex-col gap-3">
              <div className="flex items-center gap-2">
                <label
                  htmlFor="edit-relationship-end-asof"
                  className="font-sans text-[10px] tracking-wide text-muted-foreground uppercase"
                >
                  Ended in
                </label>
                <AsOfSelect
                  id="edit-relationship-end-asof"
                  asOf={endAsOf}
                  sessions={availableSessions}
                  current={sessions.current}
                  busy={busy}
                  onAsOf={setEndAsOf}
                />
              </div>

              {/* The successor editor: fill both predicates to supersede (mint a new
                  edge to the same object); leave empty for a plain End. */}
              <div className="rounded-xl border border-foreground/10 bg-background/40 p-3">
                <p className="mb-2 font-sans text-[11px] tracking-wide text-muted-foreground uppercase">
                  and replaced by{" "}
                  <span className="text-muted-foreground/60 normal-case italic">
                    the new edge (optional → supersede)
                  </span>
                </p>
                <Edge name={subjectName}>
                  <input
                    type="text"
                    aria-label="Successor forward predicate"
                    autoComplete="off"
                    placeholder="new predicate..."
                    value={succForward}
                    disabled={busy}
                    onChange={(e) => setSuccForward(e.target.value)}
                    className="w-full rounded border border-foreground/18 bg-background/60 px-2 py-1 font-sans text-[15px] text-foreground italic focus:border-gold/60 focus:ring-2 focus:ring-gold/20 focus:outline-none disabled:opacity-50"
                  />
                </Edge>
                <div className="mt-2">
                  <Edge name={view.other.name}>
                    <input
                      type="text"
                      aria-label="Successor reverse predicate"
                      autoComplete="off"
                      placeholder="is ..."
                      value={succReverse}
                      disabled={busy}
                      onChange={(e) => setSuccReverse(e.target.value)}
                      className="w-full rounded border border-foreground/18 bg-background/60 px-2 py-1 font-sans text-[15px] text-foreground italic focus:border-gold/60 focus:ring-2 focus:ring-gold/20 focus:outline-none disabled:opacity-50"
                    />
                  </Edge>
                </div>
              </div>
            </div>
          ) : null}
        </section>

        {/* Corrections drawer, tucked. */}
        <section className="mt-5 border-t border-foreground/10 pt-4">
          <button
            type="button"
            onClick={() => setCorrectionsOpen((o) => !o)}
            aria-expanded={correctionsOpen}
            className="font-sans text-[11px] tracking-wide text-muted-foreground uppercase transition-colors hover:text-foreground"
          >
            Corrections {correctionsOpen ? "−" : "+"}{" "}
            <span className="text-muted-foreground/60 normal-case italic">
              it was never true, or shouldn't exist
            </span>
          </button>

          {correctionsOpen ? (
            <div className="mt-3 flex flex-col gap-4">
              <div className="flex flex-col gap-2">
                <label className="inline-flex items-center gap-2 font-sans text-[13px] text-foreground">
                  <input
                    type="checkbox"
                    checked={retconArmed}
                    disabled={busy || !hasSessions}
                    onChange={(e) => setRetconArmed(e.target.checked)}
                    className="accent-gold"
                  />
                  Retcon
                </label>
                <p className="font-sans text-[12px] text-muted-foreground italic">
                  Never happened in the fiction, struck as a believed falsehood in snapshots before
                  it was caught.
                </p>
                {retconArmed ? (
                  <div className="flex items-center gap-2">
                    <label
                      htmlFor="edit-relationship-retcon-asof"
                      className="font-sans text-[10px] tracking-wide text-muted-foreground uppercase"
                    >
                      Caught in
                    </label>
                    <AsOfSelect
                      id="edit-relationship-retcon-asof"
                      asOf={retconAsOf}
                      sessions={availableSessions}
                      current={sessions.current}
                      busy={busy}
                      onAsOf={setRetconAsOf}
                    />
                  </div>
                ) : null}
              </div>

              <div className="flex flex-col gap-2 border-t border-foreground/10 pt-3">
                <label className="inline-flex items-center gap-2 font-sans text-[13px] text-red-700 dark:text-red-400">
                  <input
                    type="checkbox"
                    checked={deleteArmed}
                    disabled={busy}
                    onChange={(e) => setDeleteArmed(e.target.checked)}
                    className="accent-red-700"
                  />
                  Delete
                </label>
                <p className="font-sans text-[12px] text-muted-foreground italic">
                  Expunge the record, no audit trail. Only for spurious AI extractions or test data.
                </p>
              </div>
            </div>
          ) : null}
        </section>

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
              deleteArmed
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

function FactButton({
  active,
  disabled,
  label,
  Icon,
  onClick,
}: {
  active: boolean;
  disabled: boolean;
  label: string;
  Icon?: typeof RotateCcw;
  onClick: () => void;
}): React.ReactElement {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={active}
      disabled={disabled}
      onClick={onClick}
      className={[
        "inline-flex items-center gap-1.5 border-foreground/12 px-3 py-1.5 font-sans text-[13px] transition-colors [&+&]:border-l disabled:opacity-40",
        active
          ? "bg-gold/15 font-semibold text-foreground"
          : "text-muted-foreground hover:bg-gold/6 hover:text-foreground",
      ].join(" ")}
    >
      {Icon !== undefined ? <Icon className="size-3.5" aria-hidden="true" /> : null}
      {label}
    </button>
  );
}

// One direction of the relationship as an editable edge: the page, an arrow, and the
// predicate input that reads from it.
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
  sessions: SessionRef[];
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
