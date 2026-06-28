// The create-relationship flow as a sentence builder: [subject] [predicate]
// [object], ported from the design-system wireframe (copy + mechanics kept,
// em-dashes scrubbed). The subject is fixed to the current entity, so the GM only
// chooses the predicate and the other thing.
//
// Presentational on purpose: every data feed and network action arrives as a prop
// (predicates/sessions as data, search/create/submit as callbacks), so the whole
// flow is play-testable with spied callbacks and no socket - the same
// connector/presentational split as RelationshipsSection/RelationshipsWidget. The
// connector useCreateRelationship binds these props to the campaign API.
//
// The predicate and object are @familiar-systems/ui ComboBoxes (inline variant):
// React Aria owns the listbox a11y, keyboard nav, and dropdown dismissal. The
// predicate takes custom values (allowsCustomValue); the object composes a
// "Create <name>" sentinel item on top of the server results, so a not-yet-minted
// entity is selectable without the primitive knowing the domain.
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
import { Button, ComboBox, ComboBoxItem, Dialog, Modal } from "@familiar-systems/ui";
import { X } from "lucide-react";
import { useMemo, useRef, useState } from "react";

import { filterPredicates, reverseFor } from "./predicateMatch";
import { EntityChip, KnowledgeControl, SessionSelect } from "./relationshipChrome";

// The object can be an existing page or a not-yet-minted new entity (minted on
// submit, not on selection, so a cancelled modal never strands an orphan page).
type ObjectChoice = { kind: "existing"; id: PageId; name: string } | { kind: "new"; name: string };

// Sentinel key for the "Create <name>" row; PageIds are ULIDs, so it can't collide.
const CREATE_KEY = "__create__";

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

  // Synchronous double-submit guard: submit is a two-request op (mint then relate),
  // so the same-tick Enter+click window is wider than usual.
  const submittingRef = useRef(false);
  // Last-write-wins for the async object search: a slow earlier response must not
  // clobber a faster later one.
  const searchSeqRef = useRef(0);

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

  function onPredicateSelect(key: string | null): void {
    if (key === null) return; // a custom value; setForward already captured it
    const pair = predicates.find((p) => p.forward === key);
    if (pair !== undefined) {
      setPredicateForward(pair.forward);
      setPredicateReverse(pair.reverse);
      setReverseEdited(false);
    }
  }

  // The object listbox rows: server results plus a trailing "Create <name>" row
  // when the query is non-empty and matches nothing exactly. A flat shape so the
  // ComboBox's render fn stays domain-agnostic.
  const objectItems = useMemo(() => {
    const rows = objectResults.map((r) => ({
      id: r.id as string,
      label: r.name,
      create: false,
    }));
    const q = objectQuery.trim();
    const exact = objectResults.some((r) => r.name.toLowerCase() === q.toLowerCase());
    if (q !== "" && !exact) rows.push({ id: CREATE_KEY, label: q, create: true });
    return rows;
  }, [objectResults, objectQuery]);

  function onObjectSelect(key: string | null): void {
    if (key === null) return;
    if (key === CREATE_KEY) {
      setObjectChoice({ kind: "new", name: objectQuery.trim() });
    } else {
      const result = objectResults.find((r) => r.id === key);
      if (result !== undefined) {
        setObjectChoice({ kind: "existing", id: result.id, name: result.name });
      }
    }
    setError(null);
  }

  function clearObject(): void {
    setObjectChoice(null);
    setObjectQuery("");
    setObjectResults([]);
  }

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

  // React Aria's collection keys items by `id`; PredicatePairView has none, so
  // brand the forward string as the id (it's the unique vocabulary key).
  const predicateItems = filterPredicates(predicates, predicateForward).map((p) => ({
    ...p,
    id: p.forward,
  }));

  return (
    <Modal
      isOpen
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      isDismissable={!busy}
      isKeyboardDismissDisabled={busy}
      className="max-w-xl overflow-visible"
    >
      <Dialog aria-labelledby="create-relationship-title" className="outline-none">
        <div className="mb-4 flex items-baseline gap-2">
          <h2
            id="create-relationship-title"
            className="font-display text-lg font-semibold text-foreground"
          >
            New relationship
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

        {/* Sentence: [subject] [predicate] [object] */}
        <div className="flex flex-wrap items-center gap-2 rounded-xl border border-foreground/10 bg-background/60 p-3 font-sans text-[15px]">
          <EntityChip name={subjectName} />

          {/* Predicate: a custom-value combobox over the known pairs. */}
          <ComboBox
            variant="inline"
            aria-label="Predicate"
            allowsCustomValue
            // First gesture of the flow; React Aria lands focus here on open. The
            // dropdown opens on input (default menuTrigger), NOT on focus: an open
            // ComboBox ariaHideOutside-hides the rest of the sentence, so we never
            // auto-open it on mount.
            autoFocus
            isDisabled={busy}
            placeholder="predicate..."
            inputValue={predicateForward}
            onInputChange={setForward}
            items={predicateItems}
            onSelectionChange={(key) => onPredicateSelect(key === null ? null : String(key))}
            className="min-w-37.5 text-foreground italic"
          >
            {(pair: PredicatePairView & { id: string }) => (
              <ComboBoxItem id={pair.id} textValue={pair.forward} className="justify-between gap-3">
                <span className="font-sans text-sm text-foreground italic">{pair.forward}</span>
                <span className="font-sans text-[10px] tracking-wide text-muted-foreground">
                  {pair.count} {pair.count === 1 ? "edge" : "edges"}
                </span>
              </ComboBoxItem>
            )}
          </ComboBox>

          {/* Object: a chip once chosen, a search combobox before that. */}
          {objectChoice !== null ? (
            <span className="inline-flex items-center gap-1">
              <EntityChip name={objectChoice.name} isNew={objectChoice.kind === "new"} />
              <Button
                variant="icon"
                size="sm"
                aria-label="Change thing"
                isDisabled={busy}
                onPress={clearObject}
                className="size-5 border-0 bg-transparent text-muted-foreground hover:bg-foreground/5 hover:text-foreground"
              >
                <X className="size-3" />
              </Button>
            </span>
          ) : (
            <ComboBox
              variant="inline"
              aria-label="Search entities"
              allowsEmptyCollection
              menuTrigger="focus"
              isDisabled={busy}
              placeholder="choose a thing..."
              inputValue={objectQuery}
              onInputChange={(value) => {
                setObjectQuery(value);
                void searchObjects(value);
              }}
              onOpenChange={(isOpen) => {
                if (isOpen) void searchObjects(objectQuery);
              }}
              items={objectItems}
              onSelectionChange={(key) => onObjectSelect(key === null ? null : String(key))}
              className="min-w-42.5 font-display font-semibold"
            >
              {(item: { id: string; label: string; create: boolean }) =>
                item.create ? (
                  <ComboBoxItem
                    id={item.id}
                    textValue={item.label}
                    className="gap-1 text-muted-foreground italic"
                  >
                    Create{" "}
                    <span className="font-display font-semibold text-foreground not-italic">
                      {item.label}
                    </span>
                  </ComboBoxItem>
                ) : (
                  <ComboBoxItem id={item.id} textValue={item.label}>
                    <span className="font-display text-sm font-semibold text-foreground">
                      {item.label}
                    </span>
                  </ComboBoxItem>
                )
              }
            </ComboBox>
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
              className="min-w-35 border-b border-dashed border-foreground/25 bg-transparent px-1 text-foreground italic placeholder:text-muted-foreground/50 focus:border-primary focus:outline-none"
            />
            <span className="font-display font-semibold text-foreground">{subjectName}</span>
            <span className="ms-auto rounded-sm border border-foreground/10 px-1.5 font-sans text-[9px] tracking-wide text-muted-foreground uppercase">
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
            create, so `allowReveal={false}` keeps this a plain Public/Hidden choice. */}
        <div className="mt-3 flex flex-col gap-2">
          <span className="font-sans text-[10px] tracking-wide text-muted-foreground uppercase">
            To the players
          </span>
          <KnowledgeControl
            value={knowledge}
            disabled={busy}
            allowReveal={false}
            sessions={sessions.sessions}
            onChange={setKnowledge}
          />
        </div>

        {error !== null ? (
          <p className="mt-3 font-sans text-xs text-red-700 dark:text-red-400">{error}</p>
        ) : null}

        <div className="mt-5 flex items-center gap-2 border-t border-foreground/10 pt-4">
          <Button variant="outline" isDisabled={busy} onPress={onClose}>
            Cancel
          </Button>
          <div className="flex-1" />
          <Button variant="primary" isDisabled={!canSubmit} onPress={() => void submit()}>
            {busy ? "Creating..." : "Create"}
          </Button>
        </div>
      </Dialog>
    </Modal>
  );
}
