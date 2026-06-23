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
  OriginInput,
  PageId,
  PredicatePairView,
  SessionsResponse,
  Visibility,
} from "@familiar-systems/types-campaign";
import { Plus, Search, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { filterPredicates, reverseFor } from "./predicateMatch";
import { EntityChip, VisibilityToggle } from "./relationshipChrome";
import { useTypeahead } from "./useTypeahead";

// The object can be an existing page or a not-yet-minted new entity (minted on
// submit, not on selection, so a cancelled modal never strands an orphan page).
type ObjectChoice = { kind: "existing"; id: PageId; name: string } | { kind: "new"; name: string };

// The "as of" select uses session ids as option values; this sentinel is the
// Prior option (session ids are ULIDs, so it can't collide).
const PRIOR_VALUE = "prior";

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
  const [visibility, setVisibility] = useState<Visibility>("gm");
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
  const predForwardTrim = predicateForward.trim().toLowerCase();
  const predicateExact = predicateMatches.some((p) => p.forward.toLowerCase() === predForwardTrim);
  const showUseCustom = predForwardTrim !== "" && !predicateExact;
  const predicateItemCount = predicateMatches.length + (showUseCustom ? 1 : 0);

  const onPredicatePick = (index: number): void => {
    const pair = predicateMatches[index];
    // The trailing row (index past the matches) is "use custom": the forward is
    // already the typed text, so there is nothing to set.
    if (pair !== undefined) commitPredicate(pair);
  };
  const predicateTA = useTypeahead(predicateItemCount, { onPick: onPredicatePick });

  // Object typeahead: server search results + a "create new entity" row.
  const objQueryTrim = objectQuery.trim().toLowerCase();
  const objectExact = objectResults.some((r) => r.name.toLowerCase() === objQueryTrim);
  const showCreateNew = objQueryTrim !== "" && !objectExact;
  const objectItemCount = objectResults.length + (showCreateNew ? 1 : 0);

  const onObjectPick = (index: number): void => {
    const result = objectResults[index];
    if (result !== undefined) commitObject({ kind: "existing", id: result.id, name: result.name });
    else commitObject({ kind: "new", name: objectQuery.trim() });
  };
  const objectTA = useTypeahead(objectItemCount, { onPick: onObjectPick });

  // Escape closes an open dropdown first, then the dialog (never mid-request). A
  // document listener, rebound when the open flags change, catches it whichever
  // control holds focus, and is the single authority for what Escape means.
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key !== "Escape" || busy) return;
      // Close any open suggestion dropdown first; only a second Escape, with
      // nothing open, dismisses the dialog. (The predicate dropdown opens on the
      // initial autofocus, so close both to be safe.)
      if (objectTA.open || predicateTA.open) {
        objectTA.setOpen(false);
        predicateTA.setOpen(false);
        return;
      }
      onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [busy, objectTA.open, objectTA.setOpen, predicateTA.open, predicateTA.setOpen, onClose]);

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
        visibility,
        origin,
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
  const asOfValue = origin.kind === "prior" ? PRIOR_VALUE : origin.content;

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
              aria-expanded={predicateTA.open}
              aria-controls="predicate-listbox"
              aria-activedescendant={
                predicateTA.open ? `pred-opt-${predicateTA.activeIndex}` : undefined
              }
              aria-label="Predicate"
              autoComplete="off"
              placeholder="predicate..."
              value={predicateForward}
              disabled={busy}
              onChange={(e) => {
                setForward(e.target.value);
                predicateTA.setOpen(true);
              }}
              onFocus={() => predicateTA.setOpen(true)}
              onBlur={() => setTimeout(() => predicateTA.setOpen(false), 120)}
              onKeyDown={predicateTA.onKeyDown}
              className="min-w-37.5 border-b border-dashed border-foreground/30 bg-transparent px-1 py-0.5 font-sans text-[15px] text-foreground italic placeholder:text-muted-foreground/50 focus:border-gold/60 focus:outline-none"
            />
            {predicateTA.open && predicateItemCount > 0 ? (
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
                    aria-selected={predicateTA.activeIndex === i}
                    onMouseEnter={() => predicateTA.setActiveIndex(i)}
                    onMouseDown={(e) => {
                      e.preventDefault();
                      onPredicatePick(i);
                      predicateTA.setOpen(false);
                    }}
                    className={[
                      "flex cursor-pointer items-baseline justify-between gap-3 rounded-md px-2.5 py-1.5",
                      predicateTA.activeIndex === i ? "bg-gold/15" : "",
                    ].join(" ")}
                  >
                    <span className="font-sans text-sm text-foreground italic">{pair.forward}</span>
                    <span className="font-sans text-[10px] tracking-wide text-muted-foreground">
                      {pair.count} {pair.count === 1 ? "edge" : "edges"}
                    </span>
                  </li>
                ))}
                {showUseCustom ? (
                  <li
                    id={`pred-opt-${predicateMatches.length}`}
                    role="option"
                    aria-selected={predicateTA.activeIndex === predicateMatches.length}
                    onMouseEnter={() => predicateTA.setActiveIndex(predicateMatches.length)}
                    onMouseDown={(e) => {
                      e.preventDefault();
                      onPredicatePick(predicateMatches.length);
                      predicateTA.setOpen(false);
                    }}
                    className={[
                      "mt-0.5 flex cursor-pointer items-baseline gap-2 rounded-md border-t border-foreground/10 px-2.5 py-1.5 font-sans text-sm text-muted-foreground italic",
                      predicateTA.activeIndex === predicateMatches.length ? "bg-gold/10" : "",
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
                aria-expanded={objectTA.open}
                aria-controls="object-listbox"
                aria-activedescendant={
                  objectTA.open ? `obj-opt-${objectTA.activeIndex}` : undefined
                }
                aria-label="Search entities"
                autoComplete="off"
                placeholder="choose a thing..."
                value={objectQuery}
                disabled={busy}
                onChange={(e) => {
                  setObjectQuery(e.target.value);
                  objectTA.setOpen(true);
                  void searchObjects(e.target.value);
                }}
                onFocus={() => {
                  objectTA.setOpen(true);
                  void searchObjects(objectQuery);
                }}
                onBlur={() => setTimeout(() => objectTA.setOpen(false), 120)}
                onKeyDown={objectTA.onKeyDown}
                className="min-w-42.5 rounded border border-foreground/15 bg-background/60 py-1 pr-2 pl-7 font-display text-[15px] font-semibold text-foreground placeholder:font-sans placeholder:font-normal placeholder:text-muted-foreground/50 placeholder:italic focus:border-gold/50 focus:ring-2 focus:ring-gold/20 focus:outline-none"
              />
              {objectTA.open && objectItemCount > 0 ? (
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
                      aria-selected={objectTA.activeIndex === i}
                      onMouseEnter={() => objectTA.setActiveIndex(i)}
                      onMouseDown={(e) => {
                        e.preventDefault();
                        onObjectPick(i);
                        objectTA.setOpen(false);
                      }}
                      className={[
                        "flex cursor-pointer items-baseline rounded-md px-2.5 py-1.5",
                        objectTA.activeIndex === i ? "bg-gold/15" : "",
                      ].join(" ")}
                    >
                      <span className="font-display text-sm font-semibold text-foreground">
                        {result.name}
                      </span>
                    </li>
                  ))}
                  {showCreateNew ? (
                    <li
                      id={`obj-opt-${objectResults.length}`}
                      role="option"
                      aria-selected={objectTA.activeIndex === objectResults.length}
                      onMouseEnter={() => objectTA.setActiveIndex(objectResults.length)}
                      onMouseDown={(e) => {
                        e.preventDefault();
                        onObjectPick(objectResults.length);
                        objectTA.setOpen(false);
                      }}
                      className={[
                        "mt-0.5 flex cursor-pointer items-baseline gap-2 rounded-md border-t border-foreground/10 px-2.5 py-1.5 font-sans text-sm text-muted-foreground italic",
                        objectTA.activeIndex === objectResults.length ? "bg-gold/10" : "",
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
          <select
            id="create-relationship-asof"
            value={asOfValue}
            disabled={busy}
            onChange={(e) => {
              const value = e.target.value;
              if (value === PRIOR_VALUE) {
                setOrigin({ kind: "prior" });
                return;
              }
              const match = sessions.sessions.find((s) => s.id === value);
              if (match !== undefined) setOrigin({ kind: "session", content: match.id });
            }}
            className="rounded border border-gold/40 bg-background/60 px-2 py-1 font-sans text-xs text-foreground focus:border-gold/60 focus:outline-none disabled:opacity-50"
          >
            <option value={PRIOR_VALUE}>Prior, before the campaign</option>
            {sessions.sessions.map((s) => (
              <option key={s.id} value={s.id}>
                Session {s.ordinal}
                {sessions.current !== null && s.id === sessions.current.id ? " (current)" : ""}
              </option>
            ))}
          </select>
        </div>

        {/* Visibility: who can see the fact, independent of when it became true. */}
        <div className="mt-3 flex items-center gap-3">
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
