// The create-relationship flow as a sentence builder: [subject] [predicate]
// [object], ported from the design-system wireframe (copy + mechanics kept,
// em-dashes scrubbed, chrome restyled to Tailwind). The subject is fixed to the
// current entity, so the GM only chooses the predicate and the other thing.
//
// Presentational on purpose: every data feed and network action arrives as a prop
// (predicates/sessions as data, search/create/submit as callbacks), so the whole
// flow is play-testable with spied callbacks and no socket - the same
// connector/presentational split as RelationshipsSection/RelationshipsWidget. The
// connector useCreateRelationship binds these props to the campaign API.
//
// Two divergences from the wireframe, both for correctness: the reverse predicate
// is required (the server stores both directions), and the object search is a
// server query (debounced upstream), not a client filter over a local list.

import type {
  CreateRelationshipRequest,
  EntitySearchResult,
  KnowledgeInput,
  OriginInput,
  PageId,
  PredicatePairView,
  SessionsResponse,
} from "@familiar-systems/types-campaign";
import { Plus, Search, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { filterPredicates, reverseFor } from "./predicateMatch";
import { EntityChip, KnowledgeControl, SessionSelect } from "./relationshipChrome";
import { useTypeaheadSlot } from "./useTypeahead";

// The object can be an existing page or a not-yet-minted new entity (minted on
// submit, not on selection, so a cancelled modal never strands an orphan page).
type ObjectChoice = { kind: "existing"; id: PageId; name: string } | { kind: "new"; name: string };

interface CreateRelationshipModalProps {
  subjectName: string;
  subjectPageId: PageId;
  predicates: PredicatePairView[];
  sessions: SessionsResponse;
  onSearchEntities: (query: string) => Promise<EntitySearchResult[]>;
  onCreateEntity: (name: string) => Promise<{ id: PageId; name: string }>;
  onSubmit: (req: CreateRelationshipRequest) => Promise<void>;
  onClose: () => void;
}

export function CreateRelationshipModal({
  subjectName,
  subjectPageId,
  predicates,
  sessions,
  onSearchEntities,
  onCreateEntity,
  onSubmit,
  onClose,
}: CreateRelationshipModalProps): React.ReactElement {
  const [objectChoice, setObjectChoice] = useState<ObjectChoice | null>(null);
  const [objectQuery, setObjectQuery] = useState("");
  const [objectResults, setObjectResults] = useState<EntitySearchResult[]>([]);
  const [predicateForward, setPredicateForward] = useState("");
  const [predicateReverse, setPredicateReverse] = useState("");
  // Once the GM types into the reverse field we stop autofilling it from the graph,
  // so their wording is never overwritten as they keep editing the forward.
  const [reverseEdited, setReverseEdited] = useState(false);
  const [origin, setOrigin] = useState<OriginInput>(
    sessions.current !== null
      ? { kind: "session", content: sessions.current.id }
      : { kind: "prior" },
  );
  // Default born public; the GM marks it secret (and optionally revealed). Matches
  // the wireframe's create default.
  const [knowledge, setKnowledge] = useState<KnowledgeInput>({ kind: "public" });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const predicateInputRef = useRef<HTMLInputElement>(null);
  const objectInputRef = useRef<HTMLInputElement>(null);
  // Synchronous double-submit guard: submit is a two-request op (mint then relate),
  // so the same-tick Enter+click window is wider than usual.
  const submittingRef = useRef(false);
  // Last-write-wins for the async object search: a slow earlier response must not
  // clobber a faster later one.
  const searchSeqRef = useRef(0);
  // Refocus the object input when it returns after the GM clears a chosen object.
  const refocusObjectRef = useRef(false);

  // The predicate is the gesture, so land the cursor there on open.
  useEffect(() => {
    predicateInputRef.current?.focus();
  }, []);

  useEffect(() => {
    if (objectChoice === null && refocusObjectRef.current) {
      refocusObjectRef.current = false;
      objectInputRef.current?.focus();
    }
  }, [objectChoice]);

  async function searchObjects(query: string): Promise<void> {
    const seq = ++searchSeqRef.current;
    const results = await onSearchEntities(query);
    if (seq !== searchSeqRef.current) return; // a newer query already answered
    // A relationship needs two distinct pages, so the subject can't be the object.
    setObjectResults(results.filter((r) => r.id !== subjectPageId));
  }

  function setForward(value: string): void {
    setPredicateForward(value);
    if (!reverseEdited) setPredicateReverse(reverseFor(predicates, value) ?? "");
  }

  function commitPredicate(pair: PredicatePairView): void {
    setPredicateForward(pair.forward);
    setPredicateReverse(pair.reverse);
    setReverseEdited(false);
  }

  function commitObject(choice: ObjectChoice): void {
    setObjectChoice(choice);
    setError(null);
  }

  function clearObject(): void {
    refocusObjectRef.current = true;
    setObjectChoice(null);
    setObjectQuery("");
  }

  // Predicate typeahead: client filter over the known pairs + a "use custom" row.
  const predicateMatches = filterPredicates(predicates, predicateForward);
  const predicateSlot = useTypeaheadSlot({
    items: predicateMatches,
    query: predicateForward,
    keyOf: (p) => p.forward,
    onPickItem: commitPredicate,
    // "Use custom" sets nothing - the forward is already the typed text - but the row
    // must be non-null to be offered and keyboard-reachable.
    onPickExtra: () => {},
  });

  // Object typeahead: server search results + a "create new entity" row.
  const objectSlot = useTypeaheadSlot({
    items: objectResults,
    query: objectQuery,
    keyOf: (r) => r.name,
    onPickItem: (r) => commitObject({ kind: "existing", id: r.id, name: r.name }),
    onPickExtra: () => commitObject({ kind: "new", name: objectQuery.trim() }),
  });

  // Escape closes an open dropdown first, then the dialog (never mid-request). A
  // document listener, rebound when the open flags change, catches it whichever
  // control holds focus, and is the single authority for what Escape means.
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key !== "Escape" || busy) return;
      // Close any open suggestion dropdown first; only a second Escape, with
      // nothing open, dismisses the dialog. (The predicate dropdown opens on the
      // initial autofocus, so close both to be safe.)
      if (objectSlot.ta.open || predicateSlot.ta.open) {
        objectSlot.ta.setOpen(false);
        predicateSlot.ta.setOpen(false);
        return;
      }
      onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [
    busy,
    objectSlot.ta.open,
    objectSlot.ta.setOpen,
    predicateSlot.ta.open,
    predicateSlot.ta.setOpen,
    onClose,
  ]);

  const selfEdge = objectChoice?.kind === "existing" && objectChoice.id === subjectPageId;
  const canSubmit =
    objectChoice !== null &&
    predicateForward.trim() !== "" &&
    predicateReverse.trim() !== "" &&
    !selfEdge &&
    !busy;

  async function submit(): Promise<void> {
    if (!canSubmit || objectChoice === null || submittingRef.current) return;
    submittingRef.current = true;
    setBusy(true);
    setError(null);
    try {
      let otherId: PageId;
      if (objectChoice.kind === "new") {
        try {
          otherId = (await onCreateEntity(objectChoice.name)).id;
        } catch {
          throw new Error(`Couldn't create the entity "${objectChoice.name}". Try again.`);
        }
      } else {
        otherId = objectChoice.id;
      }
      await onSubmit({
        subject_page_id: subjectPageId,
        other_page_id: otherId,
        predicate_forward: predicateForward.trim(),
        predicate_reverse: predicateReverse.trim(),
        origin,
        knowledge,
        supersedes: null,
      });
      // Success unmounts this modal (the connector closes + refetches); no reset.
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create relationship.");
      setBusy(false);
      submittingRef.current = false;
    }
  }

  const showReverse = predicateForward.trim() !== "" && objectChoice !== null;
  const reverseBadge = reverseEdited
    ? "edited"
    : reverseFor(predicates, predicateForward) !== null
      ? "from graph"
      : "new pair";

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-foreground/20 px-4 pt-[12vh] backdrop-blur-sm"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !busy) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="create-relationship-title"
        className="w-full max-w-xl overflow-visible rounded-2xl border border-foreground/10 bg-background/95 p-5 shadow-2xl shadow-primary/10 backdrop-blur-md"
      >
        <div className="mb-4 flex items-baseline gap-2">
          <h2
            id="create-relationship-title"
            className="font-display text-lg font-semibold text-foreground"
          >
            New relationship
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

        {/* Sentence: [subject] [predicate] [object] */}
        <div className="flex flex-wrap items-center gap-2 rounded-xl border border-foreground/10 bg-background/60 p-3 font-sans text-[15px]">
          <EntityChip name={subjectName} />

          {/* Predicate: always an input, with a typeahead of known pairs. */}
          <div className="relative">
            <input
              ref={predicateInputRef}
              type="text"
              role="combobox"
              aria-expanded={predicateSlot.ta.open}
              aria-controls="predicate-listbox"
              aria-activedescendant={
                predicateSlot.ta.open ? `pred-opt-${predicateSlot.ta.activeIndex}` : undefined
              }
              aria-label="Predicate"
              autoComplete="off"
              placeholder="predicate..."
              value={predicateForward}
              disabled={busy}
              onChange={(e) => {
                setForward(e.target.value);
                predicateSlot.ta.setOpen(true);
              }}
              onFocus={() => predicateSlot.ta.setOpen(true)}
              onBlur={() => setTimeout(() => predicateSlot.ta.setOpen(false), 120)}
              onKeyDown={predicateSlot.ta.onKeyDown}
              className="min-w-37.5 border-b border-dashed border-foreground/30 bg-transparent px-1 py-0.5 font-sans text-[15px] text-foreground italic placeholder:text-muted-foreground/50 focus:border-gold/60 focus:outline-none"
            />
            {predicateSlot.ta.open && predicateSlot.itemCount > 0 ? (
              <ul
                id="predicate-listbox"
                role="listbox"
                className="absolute top-full left-0 z-10 mt-1 max-h-64 min-w-60 overflow-y-auto rounded-lg border border-foreground/10 bg-background/95 p-1 shadow-xl shadow-primary/10 backdrop-blur-md"
              >
                {predicateMatches.map((pair, i) => (
                  <li
                    id={`pred-opt-${i}`}
                    key={pair.forward}
                    role="option"
                    aria-selected={predicateSlot.ta.activeIndex === i}
                    onMouseEnter={() => predicateSlot.ta.setActiveIndex(i)}
                    onMouseDown={(e) => {
                      e.preventDefault();
                      predicateSlot.onPick(i);
                      predicateSlot.ta.setOpen(false);
                    }}
                    className={[
                      "flex cursor-pointer items-baseline justify-between gap-3 rounded-md px-2.5 py-1.5",
                      predicateSlot.ta.activeIndex === i ? "bg-gold/15" : "",
                    ].join(" ")}
                  >
                    <span className="font-sans text-sm text-foreground italic">{pair.forward}</span>
                    <span className="font-sans text-[10px] tracking-wide text-muted-foreground">
                      {pair.count} {pair.count === 1 ? "edge" : "edges"}
                    </span>
                  </li>
                ))}
                {predicateSlot.showExtra ? (
                  <li
                    id={`pred-opt-${predicateMatches.length}`}
                    role="option"
                    aria-selected={predicateSlot.ta.activeIndex === predicateMatches.length}
                    onMouseEnter={() => predicateSlot.ta.setActiveIndex(predicateMatches.length)}
                    onMouseDown={(e) => {
                      e.preventDefault();
                      predicateSlot.onPick(predicateMatches.length);
                      predicateSlot.ta.setOpen(false);
                    }}
                    className={[
                      "mt-0.5 flex cursor-pointer items-baseline gap-2 rounded-md border-t border-foreground/10 px-2.5 py-1.5 font-sans text-sm text-muted-foreground italic",
                      predicateSlot.ta.activeIndex === predicateMatches.length ? "bg-gold/10" : "",
                    ].join(" ")}
                  >
                    <Plus className="size-3 self-center text-primary" aria-hidden="true" />
                    Use{" "}
                    <span className="font-display font-semibold text-foreground not-italic">
                      {predicateForward.trim()}
                    </span>
                  </li>
                ) : null}
              </ul>
            ) : null}
          </div>

          {/* Object: a chip once chosen, an input + typeahead before that. */}
          {objectChoice !== null ? (
            <span className="inline-flex items-center gap-1">
              <EntityChip name={objectChoice.name} isNew={objectChoice.kind === "new"} />
              <button
                type="button"
                aria-label="Change thing"
                onClick={clearObject}
                disabled={busy}
                className="flex size-5 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-foreground/5 hover:text-foreground disabled:opacity-50"
              >
                <X className="size-3" />
              </button>
            </span>
          ) : (
            <div className="relative">
              <Search
                className="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground"
                aria-hidden="true"
              />
              <input
                ref={objectInputRef}
                type="text"
                role="combobox"
                aria-expanded={objectSlot.ta.open}
                aria-controls="object-listbox"
                aria-activedescendant={
                  objectSlot.ta.open ? `obj-opt-${objectSlot.ta.activeIndex}` : undefined
                }
                aria-label="Search entities"
                autoComplete="off"
                placeholder="choose a thing..."
                value={objectQuery}
                disabled={busy}
                onChange={(e) => {
                  setObjectQuery(e.target.value);
                  objectSlot.ta.setOpen(true);
                  void searchObjects(e.target.value);
                }}
                onFocus={() => {
                  objectSlot.ta.setOpen(true);
                  void searchObjects(objectQuery);
                }}
                onBlur={() => setTimeout(() => objectSlot.ta.setOpen(false), 120)}
                onKeyDown={objectSlot.ta.onKeyDown}
                className="min-w-42.5 rounded border border-foreground/15 bg-background/60 py-1 pr-2 pl-7 font-display text-[15px] font-semibold text-foreground placeholder:font-sans placeholder:font-normal placeholder:text-muted-foreground/50 placeholder:italic focus:border-gold/50 focus:ring-2 focus:ring-gold/20 focus:outline-none"
              />
              {objectSlot.ta.open && objectSlot.itemCount > 0 ? (
                <ul
                  id="object-listbox"
                  role="listbox"
                  className="absolute top-full left-0 z-10 mt-1 max-h-64 min-w-65 overflow-y-auto rounded-lg border border-foreground/10 bg-background/95 p-1 shadow-xl shadow-primary/10 backdrop-blur-md"
                >
                  {objectResults.map((result, i) => (
                    <li
                      id={`obj-opt-${i}`}
                      key={result.id}
                      role="option"
                      aria-selected={objectSlot.ta.activeIndex === i}
                      onMouseEnter={() => objectSlot.ta.setActiveIndex(i)}
                      onMouseDown={(e) => {
                        e.preventDefault();
                        objectSlot.onPick(i);
                        objectSlot.ta.setOpen(false);
                      }}
                      className={[
                        "flex cursor-pointer items-baseline rounded-md px-2.5 py-1.5",
                        objectSlot.ta.activeIndex === i ? "bg-gold/15" : "",
                      ].join(" ")}
                    >
                      <span className="font-display text-sm font-semibold text-foreground">
                        {result.name}
                      </span>
                    </li>
                  ))}
                  {objectSlot.showExtra ? (
                    <li
                      id={`obj-opt-${objectResults.length}`}
                      role="option"
                      aria-selected={objectSlot.ta.activeIndex === objectResults.length}
                      onMouseEnter={() => objectSlot.ta.setActiveIndex(objectResults.length)}
                      onMouseDown={(e) => {
                        e.preventDefault();
                        objectSlot.onPick(objectResults.length);
                        objectSlot.ta.setOpen(false);
                      }}
                      className={[
                        "mt-0.5 flex cursor-pointer items-baseline gap-2 rounded-md border-t border-foreground/10 px-2.5 py-1.5 font-sans text-sm text-muted-foreground italic",
                        objectSlot.ta.activeIndex === objectResults.length ? "bg-gold/10" : "",
                      ].join(" ")}
                    >
                      <Plus className="size-3 self-center text-primary" aria-hidden="true" />
                      Create{" "}
                      <span className="font-display font-semibold text-foreground not-italic">
                        {objectQuery.trim()}
                      </span>
                    </li>
                  ) : null}
                </ul>
              ) : null}
            </div>
          )}
        </div>

        {/* Reverse predicate: how the relationship reads from the other side. */}
        {showReverse ? (
          <div className="mt-2 flex flex-wrap items-baseline gap-2 px-1 font-sans text-[13px] text-muted-foreground">
            <span className="font-sans text-[10px] tracking-wide text-muted-foreground uppercase">
              Reverse
            </span>
            <span className="font-display font-semibold text-foreground">{objectChoice.name}</span>
            <input
              type="text"
              aria-label="Reverse predicate"
              autoComplete="off"
              placeholder="is ..."
              value={predicateReverse}
              disabled={busy}
              onChange={(e) => {
                setPredicateReverse(e.target.value);
                setReverseEdited(true);
              }}
              className="min-w-35 border-b border-dashed border-foreground/25 bg-transparent px-1 text-foreground italic placeholder:text-muted-foreground/50 focus:border-gold/60 focus:outline-none"
            />
            <span className="font-display font-semibold text-foreground">{subjectName}</span>
            <span className="ml-auto rounded border border-foreground/10 px-1.5 font-sans text-[9px] tracking-wide text-muted-foreground uppercase">
              {reverseBadge}
            </span>
          </div>
        ) : null}

        {/* As of: the session the fact became true in (Prior before the campaign). */}
        <div className="mt-4 flex items-center gap-2">
          <label
            htmlFor="create-relationship-asof"
            className="font-sans text-[10px] tracking-wide text-muted-foreground uppercase"
          >
            As of
          </label>
          <SessionSelect
            id="create-relationship-asof"
            sessions={sessions.sessions}
            current={sessions.current}
            value={origin.kind === "session" ? origin.content : null}
            disabled={busy}
            onSelect={(id) => setOrigin({ kind: "session", content: id })}
            prior={{
              label: "Prior, before the campaign",
              selected: origin.kind === "prior",
              onSelect: () => setOrigin({ kind: "prior" }),
            }}
          />
        </div>

        {/* To the players: public (known) or hidden (GM-only). A new fact starts on
            the public track; revealing a secret fact at a session is an edit, not a
            create, so `bornSecret={false}` keeps this a plain Public/Hidden choice. */}
        <div className="mt-3 flex flex-col gap-2">
          <span className="font-sans text-[10px] tracking-wide text-muted-foreground uppercase">
            To the players
          </span>
          <KnowledgeControl
            value={knowledge}
            disabled={busy}
            bornSecret={false}
            sessions={sessions.sessions}
            onChange={setKnowledge}
          />
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
            className="inline-flex items-center gap-2 rounded-full bg-gold px-5 py-2 font-sans text-sm font-medium text-white shadow-md shadow-gold/25 transition-colors hover:bg-gold/90 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {busy ? "Creating..." : "Create"}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
