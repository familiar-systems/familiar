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
import {
  Button,
  Checkbox,
  Dialog,
  Modal,
  SegmentedControl,
  SegmentedItem,
} from "@familiar-systems/ui";
import { RotateCcw, X } from "lucide-react";
import { useRef, useState } from "react";

import { EntityChip, KnowledgeControl, SessionSelect } from "./relationshipChrome";

// What the modal hands back on submit: one of the three relationship-edit HTTP
// shapes, fully assembled (the modal holds every field). The connector dispatches on
// `kind` to POST / PATCH / DELETE.
export type EditSubmit =
  | { kind: "supersede"; body: CreateRelationshipRequest }
  | { kind: "patch"; body: PatchRelationshipRequest }
  | { kind: "delete" };

// The factuality axis as the GM edits it: the fact is live, or it ended at a session -
// optionally replaced by a successor edge (both predicates filled -> a supersede that
// mints a new row; left blank -> a plain end).
type Factuality =
  | { kind: "live" }
  | { kind: "ended"; asOf: SessionId | null; forward: string; reverse: string };

// The correction axis (the drawer): none, a retcon at a session, or a hard delete.
// Retcon and delete are mutually exclusive, so this single sum makes arming both at
// once unrepresentable - the illegal state the old parallel `*Armed` booleans allowed.
//
// Factuality and correction are kept as *two* sums rather than one `mode`: the data
// model lets a single row carry both `superseded` and `retcon` at once (see the
// `patch_superseded_and_retcon_coexist` route test), so a single discriminant would
// force un-retconning a both-set row to silently clear its end. Independent axes match
// the data and the independent-axis PATCH shape.
type Correction =
  | { kind: "none" }
  | { kind: "retcon"; asOf: SessionId | null }
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

  // Knowledge, freely mutable to any of its three states; edit offers all three (create
  // restricts to Public/Hidden).
  const [knowledge, setKnowledge] = useState<KnowledgeInput>(() =>
    knowledgeInputFromView(view.knowledge, allSessions),
  );
  // `asOf` is null only for the degraded "ended/retconned at a session that was
  // renumbered away" case the picker then can't resolve; the diff guards on it.
  const [factuality, setFactuality] = useState<Factuality>(() =>
    view.superseded !== null
      ? {
          kind: "ended",
          asOf: sessionIdForOrdinal(view.superseded, allSessions) ?? defaultAsOf,
          forward: "",
          reverse: "",
        }
      : { kind: "live" },
  );
  const [correction, setCorrection] = useState<Correction>(() =>
    view.retcon !== null
      ? { kind: "retcon", asOf: sessionIdForOrdinal(view.retcon, allSessions) ?? defaultAsOf }
      : { kind: "none" },
  );
  // The corrections drawer open/closed (disclosure UI, not a domain axis).
  const [correctionsOpen, setCorrectionsOpen] = useState(view.retcon !== null);

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Re-arming an axis defaults its session back to where it was persisted (if any), so
  // toggling Ongoing -> Ended (or re-checking Retcon) doesn't silently move the stamp.
  const endDefaultAsOf = sessionIdForOrdinal(view.superseded, allSessions) ?? defaultAsOf;
  const retconDefaultAsOf = sessionIdForOrdinal(view.retcon, allSessions) ?? defaultAsOf;

  const submittingRef = useRef(false);

  const hasSessions = availableSessions.length > 0;
  // Arming retcon dims the factuality axis (a retcon governs factuality wholesale).
  const factualityDisabled = busy || correction.kind === "retcon";

  // --- Diff each axis into a stamp patch (null = unchanged) -----------------

  function knowledgeDiff(): KnowledgeInput | null {
    // The knowledge axis is set wholesale: emit the new state if it differs from the
    // opening one (a conceal Public -> Hidden, a reveal, a re-hide all count).
    const original = knowledgeInputFromView(view.knowledge, allSessions);
    return sameKnowledge(knowledge, original) ? null : knowledge;
  }

  function retconDiff(): SessionStampPatch | null {
    const wasOrdinal = view.retcon?.ordinal ?? null;
    if (correction.kind === "retcon" && correction.asOf !== null) {
      return ordinalOf(correction.asOf) !== wasOrdinal ? set(correction.asOf) : null;
    }
    return wasOrdinal !== null ? clear : null; // un-retcon
  }

  function supersededDiff(): SessionStampPatch | null {
    // Retcon dims factuality, so it is not edited while armed.
    if (correction.kind === "retcon") return null;
    const wasOrdinal = view.superseded?.ordinal ?? null;
    if (factuality.kind === "ended" && factuality.asOf !== null) {
      return ordinalOf(factuality.asOf) !== wasOrdinal ? set(factuality.asOf) : null;
    }
    return wasOrdinal !== null ? clear : null; // un-end
  }

  const succForward = factuality.kind === "ended" ? factuality.forward : "";
  const succReverse = factuality.kind === "ended" ? factuality.reverse : "";
  const fwdFilled = succForward.trim() !== "";
  const revFilled = succReverse.trim() !== "";
  const successorIntent =
    correction.kind !== "retcon" && factuality.kind === "ended" && (fwdFilled || revFilled);

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
    if (correction.kind === "delete") {
      return { submit: { kind: "delete" }, label: "Delete permanently" };
    }

    const knowledgePatch = knowledgeDiff();

    // End-with-successor mints a new row (the successor carries the knowledge); it is
    // mutually exclusive with retcon, which dims factuality. The `factuality.kind`
    // re-check narrows the union (successorIntent already implies it).
    if (successorIntent && factuality.kind === "ended") {
      if (!(fwdFilled && revFilled) || factuality.asOf === null) {
        return { submit: null, label: "Fill both successor predicates" };
      }
      const parts: string[] = [];
      if (knowledgePatch !== null) parts.push(knowledgeLabel(knowledgePatch));
      parts.push(`End S${ordinalOf(factuality.asOf)} · +successor`);
      return {
        submit: {
          kind: "supersede",
          body: {
            subject_page_id: subjectPageId,
            other_page_id: view.other.id,
            predicate_forward: factuality.forward.trim(),
            predicate_reverse: factuality.reverse.trim(),
            origin: { kind: "session", content: factuality.asOf },
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

  // Two axes plus a corrections drawer make this taller than the create modal, so
  // the panel scrolls internally rather than pushing submit off-screen.
  return (
    <Modal
      isOpen
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      isDismissable={!busy}
      isKeyboardDismissDisabled={busy}
      className="max-h-[88vh] max-w-xl overflow-y-auto"
    >
      <Dialog aria-labelledby="edit-relationship-title" className="outline-none">
        <div className="flex items-baseline gap-2">
          <h2
            id="edit-relationship-title"
            className="font-display text-lg font-semibold text-foreground"
          >
            Edit relationship
          </h2>
          <Button
            variant="icon"
            size="sm"
            aria-label="Close"
            isDisabled={busy}
            onPress={onClose}
            className="ms-auto border-0 bg-transparent text-muted-foreground hover:bg-foreground/5 hover:text-foreground"
          >
            <X className="size-4" />
          </Button>
        </div>

        {/* Current line: what is being edited, and its persisted state. */}
        <p className="mt-1 flex flex-wrap items-baseline gap-x-1.5 gap-y-1 border-b border-foreground/10 pb-3 font-sans text-[13px] text-muted-foreground italic">
          <EntityChip name={subjectName} />
          <span className="text-foreground/70">{view.predicate}</span>
          <EntityChip name={view.other.name} />
          <span className="ms-1 font-sans text-[11px] tracking-wide text-muted-foreground/80 not-italic">
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
              allowReveal
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

          <SegmentedControl
            aria-label="In the fiction"
            className="mt-2"
            isDisabled={factualityDisabled}
            selectedKeys={factuality.kind === "live" ? ["live"] : ["ended"]}
            onSelectionChange={(keys) => {
              if ([...keys][0] === "live") {
                setFactuality({ kind: "live" });
              } else {
                // Re-pressing Ended is a no-op for the group (already selected), so
                // this only fires on a fresh end (from live), starting blank at the
                // persisted session; the existing successor draft is never reset.
                setFactuality((f) =>
                  f.kind === "ended"
                    ? f
                    : { kind: "ended", asOf: endDefaultAsOf, forward: "", reverse: "" },
                );
              }
            }}
          >
            <SegmentedItem
              id="live"
              className="data-[selected]:bg-gold/15 data-[selected]:text-foreground"
            >
              Ongoing
            </SegmentedItem>
            <SegmentedItem
              id="ended"
              isDisabled={!hasSessions}
              className="data-[selected]:bg-gold/15 data-[selected]:text-foreground"
            >
              <RotateCcw className="size-3.5" aria-hidden="true" />
              Ended
            </SegmentedItem>
          </SegmentedControl>
          {!hasSessions ? (
            <p className="mt-2 font-sans text-[12px] text-muted-foreground italic">
              This campaign has no sessions yet, so a fact can't be ended.
            </p>
          ) : null}

          {factuality.kind === "ended" && correction.kind !== "retcon" ? (
            <div className="mt-3 flex flex-col gap-3">
              <div className="flex items-center gap-2">
                <label
                  htmlFor="edit-relationship-end-asof"
                  className="font-sans text-[10px] tracking-wide text-muted-foreground uppercase"
                >
                  Ended in
                </label>
                <SessionSelect
                  id="edit-relationship-end-asof"
                  sessions={availableSessions}
                  current={sessions.current}
                  value={factuality.asOf}
                  disabled={busy}
                  onSelect={(id) =>
                    setFactuality((f) => (f.kind === "ended" ? { ...f, asOf: id } : f))
                  }
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
                    onChange={(e) =>
                      setFactuality((f) =>
                        f.kind === "ended" ? { ...f, forward: e.target.value } : f,
                      )
                    }
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
                      onChange={(e) =>
                        setFactuality((f) =>
                          f.kind === "ended" ? { ...f, reverse: e.target.value } : f,
                        )
                      }
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
                <Checkbox
                  isSelected={correction.kind === "retcon"}
                  isDisabled={busy || !hasSessions}
                  onChange={(checked) =>
                    setCorrection(
                      checked ? { kind: "retcon", asOf: retconDefaultAsOf } : { kind: "none" },
                    )
                  }
                  className="text-[13px]"
                >
                  Retcon
                </Checkbox>
                <p className="font-sans text-[12px] text-muted-foreground italic">
                  Never happened in the fiction, struck as a believed falsehood in snapshots before
                  it was caught.
                </p>
                {correction.kind === "retcon" ? (
                  <div className="flex items-center gap-2">
                    <label
                      htmlFor="edit-relationship-retcon-asof"
                      className="font-sans text-[10px] tracking-wide text-muted-foreground uppercase"
                    >
                      Caught in
                    </label>
                    <SessionSelect
                      id="edit-relationship-retcon-asof"
                      sessions={availableSessions}
                      current={sessions.current}
                      value={correction.asOf}
                      disabled={busy}
                      onSelect={(id) =>
                        setCorrection((c) => (c.kind === "retcon" ? { ...c, asOf: id } : c))
                      }
                    />
                  </div>
                ) : null}
              </div>

              <div className="flex flex-col gap-2 border-t border-foreground/10 pt-3">
                <Checkbox
                  tone="danger"
                  isSelected={correction.kind === "delete"}
                  isDisabled={busy}
                  onChange={(checked) =>
                    setCorrection(checked ? { kind: "delete" } : { kind: "none" })
                  }
                  className="text-[13px]"
                >
                  Delete
                </Checkbox>
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
          <Button variant="outline" isDisabled={busy} onPress={onClose}>
            Cancel
          </Button>
          <div className="flex-1" />
          <Button
            variant={correction.kind === "delete" ? "danger" : "primary"}
            isDisabled={!canSubmit}
            onPress={() => void submit()}
          >
            {busy ? "Saving..." : action.label}
          </Button>
        </div>
      </Dialog>
    </Modal>
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
