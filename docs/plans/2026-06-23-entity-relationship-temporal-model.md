# ADR: Entity Relationship Temporal Model

**Status:** Accepted
**Date:** 2026-06-23
**Supersedes:** `docs/archive/2026-04-10-entity-relationship-temporal-model.md` (the original single-axis model). This revision splits the temporal model into two independent authored axes; the graph model, journal pipeline, and predicate rules are carried forward unchanged.

## Context

familiar.systems models campaign worlds as a graph of Pages connected by Relationships. The GM needs to know not only what the state of their world is but what it *was*, and separately, what the *players* knew at a given point. Core queries:

- What is true as of right now? What was true during session 14?
- What did the party know going into session 8?
- How has this town's relationships evolved? Alliances, guilds, residents?

A relationship therefore moves along **two independent timed axes**, each stamped in **session** time:

- **Factuality** — when a fact was true in the fiction: an interval `[origin, superseded)`. Married in Vegas at S4, divorced at S7. Both facts are real history; a snapshot before S7 still shows the marriage.
- **Knowledge** — when the players learned a fact. A fact is public (known the moment it became true) or secret, and a secret can be **revealed** at a later session; the GM may also re-classify it (conceal, re-publicize) freely. The two axes are orthogonal: players can learn at S10 about a marriage that happened and ended years earlier.

The axes are **authored, never inferred.** The GM (or an AI suggestion the GM accepts) explicitly stamps the session each event happened in. There is no timestamp-range inference and no attempt to deduce "when was this true" from prose. Authored session stamps are precise, which is what makes a second (factuality) axis worth carrying: the cost the model pays is a stamp the author already knows, not a fuzzy inference.

## The Journal Pipeline

The journal pipeline is the primary mechanism by which the relationship graph evolves. Four artifacts feed it:

**Session Prep** is freeform text with @mentions. The GM writes plans and contingencies. No relationships are created; it is a writing surface only. The @mentions give the AI signal about which entities are relevant.

**Session Recording** is the audio of what happened at the table, the primary source of truth for play.

**Player Notes** are optional uploads, additional AI signal.

**Journal** is the canonical output. The AI reconciles prep against the recording and notes, produces a narrative summary, and proposes relationship changes. The GM reviews and accepts or rejects each proposal.

The AI is the primary author of relationship changes. It proposes mutations using the same operations available to the GM. Every AI proposal is gated by GM approval. The AI most commonly proposes create, supersede, and end; reveal proposals follow from "the party learned X this session." Retcon and delete proposals are rare (the AI seldom knows something was never true or was an error), but nothing artificially restricts what it can propose.

## The Graph Model

### The graph has two parts

**Pages** are the nodes. Authored world content (entities: NPCs, locations, factions, items; events) plus sessions, arcs, and tags are all pages. A page *is* a CRDT document, collaboratively edited via TipTap/Loro; its `kind` marks world-content pages as `entity`.

**Relationships** are the edges. A Relationship always connects exactly two pages. An interaction among more than two pages is modeled as an Event (itself a page) with multiple Relationships pointing at it.

### Relationships are bidirectional

A Relationship captures both directions in one row, a forward predicate and a reverse predicate:

```
John  --[is a resident of | is the home of]-->  Townsville
```

One row, one relationship, two directions. The stored `(page_a, page_b)` order is **canonical**: `page_a` is the lexicographically smaller `PageId`, with the predicate pair assigned to match, so each fact has exactly one encoding and a reversed duplicate is structurally impossible. Canonicalization keys on page *identity* (immutable by necessity, since it is the PK / URL / FK target), never on predicate *content*.

### Predicates are immutable

A Relationship row is born with its predicates and dies with those predicates. When a relationship evolves, either a new row is created alongside it (augmentation) or the old row is ended and a new row replaces it (supersession). Correcting mis-worded predicates is delete + recreate, not an edit.

### Relationship schema

```
id:                    ULID (primary key)
page_a:                FK -> pages          (canonical-smaller endpoint)
page_b:                FK -> pages          (canonical-larger endpoint)
predicate_a_to_b:      TEXT (immutable after creation)
predicate_b_to_a:      TEXT (immutable after creation)

-- Factuality axis
origin_session_id:     FK -> sessions NULLABLE   (NULL = Prior: true since before the campaign)
superseded_session_id: FK -> sessions NULLABLE   (NULL = still true)
retcon_session_id:     FK -> sessions NULLABLE   (NULL = not retconned)

-- Knowledge axis (freely mutable)
is_secret:             BOOL NOT NULL             (false = public, always known)
reveal_session_id:     FK -> sessions NULLABLE   (NULL = not yet revealed to players)

created_at:            TIMESTAMP                 (when the row was written, debug/audit only)
```

The in-memory domain model gives these the sum types SQLite cannot enforce at rest; the `*Col`/domain boundary reconstitutes them (mirroring how `Origin` already maps a nullable FK):

- `Origin { Prior, Session(SessionId) }` — used for `origin`, and as `Option<Origin>` for `superseded` and `retcon`.
- `Knowledge { Public, Hidden, Revealed(SessionId) }` — reconstituted from `(is_secret, reveal_session_id)`, and set wholesale (the bit is freely mutable):
  - `(false, NULL)` → `Public` (always known)
  - `(true, NULL)` → `Hidden` (GM-only)
  - `(true, Some(s))` → `Revealed(s)` (secret, learned at session s)
  - `(false, Some(s))` → illegal: a public fact has no reveal event.

## Sessions are the unit of knowledge time

A session is the atomic unit of time for both axes. All relationship mutations the AI proposes are tagged with the session that produced them; all temporal queries operate on session identity. There are no interstitial sessions and no "next session" sentinel: the session exists the moment the GM starts planning, by writing into its prep. Relationships enter the graph only when proposed and accepted (or authored directly by the GM).

`origin = Prior` is the primordial world state, true before the campaign began.

## The Factuality Axis: how relationships end

A fact's factual lifespan is the half-open interval `[origin, superseded)`. It ends one of two ways, plus a terminal correction:

**Superseded (End).** Narrative progression: the relationship was true and is no longer, because the fiction moved forward. `superseded_session_id` is set to the session it ended. It **remains visible in historical snapshots** because it was true at the time. A *Replace* (supersede-with-successor) ends the old row and creates a new row in one atomic gesture; a plain *End* sets the endpoint with no replacement.

**Retconned.** The GM declares the fact was **never** true in the fiction, even if it was established in play. `retcon_session_id` records the session the correction was made (for the timeline and the diff), but the fact is **excluded from every snapshot** regardless of T: retcon is timeless erasure of factuality, not a bound on a true-interval. The row is **kept** in the database; retcons are part of the tapestry of the game. Retcon strikes factuality but **preserves knowledge** (see below). GM-only.

**Deleted.** Hard delete, no audit trail; the row should never have existed (a spurious AI extraction the GM accepted, or test data). Deletion is **not** an invalidation and has no axis stamp. GM-only.

Both endings are **reversible** while the row lives: clearing `superseded_session_id` un-ends a fact, clearing `retcon_session_id` un-retcons it. This makes the edit surface a corrective tool rather than an append-only log. (Un-ending a fact whose supersede minted a successor would resurrect a duplicate live fact; the live-fact uniqueness index rejects that.)

`superseded` and `retcon` may coexist on one row (a fact ended at S12 and later retconned at S14). While retconned, the supersede stamp is dormant; un-retconning restores the ended state.

## The Knowledge Axis: who knows, and since when

Knowledge is **independent** of factuality. A relationship is:

- **Public** — known to the players from the moment it became true (`is_secret = false`).
- **Hidden / GM-only** — secret and not yet revealed (`is_secret = true`, no reveal).
- **Revealed at S** — secret, learned by the players at session S (`is_secret = true`, `reveal_session_id = s`).

A reveal stamps *when* the table learned the fact, so "what did the players know at session T" is answerable. Revealing in the same session a fact became true reads as plain public (no hidden interval). Retcon does **not** touch this axis: if the players were told a thing and later it was retconned, the record that they believed it survives.

The knowledge axis is **freely mutable**: the GM sets it wholesale to any of the three states - reveal (`Hidden → Revealed(s)`), conceal (`Public | Revealed(s) → Hidden`), or re-publicize (`→ Public`). The `is_secret`/`reveal` pair is always written together from a `Knowledge` value, so the illegal `(public, reveal)` combo is unreachable. Concealing a public fact is **lossy** - it keeps no record the fact was ever public, so re-revealing stamps a reveal session rather than restoring "always known." That is acceptable: conceal is a GM correction ("the players never actually knew this"), and "always was public" is one click back to Public before save, or a delete + recreate. Despite being mutable, this is *not* the old static `visibility` flag - the reveal session still carries the temporal "since when" that a boolean could not.

(Per-player visibility, e.g. the Bard's Dark Patron, is a future expansion, likely a grants table over this axis.)

## Live-fact uniqueness

At most one **live** row may exist per canonical fact (same `page_a`, `page_b`, predicate pair). Liveness is **factuality only** — two currently-true rows of the same fact are a duplicate regardless of who knows them:

```sql
-- partial unique index over (page_a, page_b, predicate_a_to_b, predicate_b_to_a)
WHERE superseded_session_id IS NULL AND retcon_session_id IS NULL
```

Superseded and retconned rows drop out of the index, so history coexists with the present, and a `Replace` that collides with an existing live fact is caught here.

## Temporal Queries

### Snapshot

"What was true as of session 10":

```sql
(origin_session_id IS NULL OR origin.session <= 10)
AND (superseded_session_id IS NULL OR superseded.session > 10)
AND retcon_session_id IS NULL
```

For a **player-facing** snapshot, additionally post-filter on the knowledge axis: keep rows that are `Public`, or `Revealed(s)` with `s <= 10`. The AI and GM see everything unfiltered.

### Diff

"What changed in session 14" returns rows where any axis event lands in that session:

```sql
origin.session = 14
  OR superseded.session = 14
  OR reveal_session_id.session = 14
  OR retcon_session_id.session = 14
```

A reveal at S14 is a first-class diff event (the party learned something), distinct from a factuality change.

## In-Memory Representation

petgraph holds the current-state graph in memory (nodes = `PageId`, edges = relationship rows, live and invalidated). Relationships load from the indexed columns on startup; all traversal happens in petgraph; the database is write-behind cold storage. The graph is owned by a single server-authoritative actor: every mutation flows through it, so invariants (canonical ordering, predicate immutability, the per-axis ordering rules, FK integrity, live-fact uniqueness) and the "AI proposes, GM disposes" gate hold at one consistency boundary. The actor decomposes each operation into single-statement writes committed in one transaction (supersede = create + end, atomically).

## Invariants

Enforced on the write path by the owning actor, with a CHECK as defense-in-depth where it is local to the row:

- A public fact has no reveal event: `NOT (is_secret = false AND reveal_session_id IS NOT NULL)` (CHECK; the wholesale knowledge write keeps the illegal combo unreachable).
- Each axis event is at or after origin: `reveal >= origin`, `superseded >= origin`, `retcon >= origin`. This compares session *ordinals* (held in the `sessions` table), so it is actor-enforced, not a CHECK.
- Predicates are immutable; canonical `page_a < page_b`.

## Names

Entity names are an alias list on the page, not a temporal relationship. Names accumulate rather than replace; a display-name pointer determines what the UI shows; all aliases are indexed for search.

## Consequences

- A relationship carries two orthogonal authored axes: factuality `[origin, superseded)` with a terminal retcon, and knowledge (`Public | Hidden | Revealed(s)`). Neither is inferred.
- The AI is the primary author of relationship changes via the journal pipeline (prep + recording + notes → proposals → GM approval); GM manual tools are the direct-edit escape hatch.
- Sessions are the atomic unit of time for both axes; snapshot and diff operate on session identity.
- Snapshots reconstruct both "what was true at T" and, with the knowledge post-filter, "what the players knew at T."
- Superseded facts remain in historical snapshots; retconned facts are struck from every snapshot but retained in the database, with knowledge preserved; factual errors are hard-deleted with no trace.
- Endings are reversible corrections, not append-only events; the live-fact uniqueness index backstops the one case (un-end with an existing successor) that would otherwise duplicate a live fact.
- One live row per predicate pair per page pair; multiple concurrent relationships between two pages are multiple rows.
- The bidirectional predicate pair lets traversal work from either direction without duplicate rows; canonical ordering guarantees a single encoding per fact.
