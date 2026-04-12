# ADR: Entity Relationship Temporal Model

**Status:** Accepted (draft)
**Date:** 2026-04-10

## Context

familiar.systems models campaign worlds as a graph of Things connected by Relationships. The GM needs to know not only what the state of their world is but what the state of their world was. Core queries:

- What is true as of right now?
- What happened during journal 14?
- How has this town's relationships evolved? Alliances? Guilds? Residents?

The strategy is a relationship graph with elements of bitemporality but not a full bitemporal model. We evaluated full bitemporal modeling and rejected it because valid time (when something is true in the fiction) is too fuzzy in most campaigns to justify a dedicated axis.

## The Journal Pipeline

The journal pipeline is the primary mechanism by which the relationship graph evolves. Four artifacts feed into it:

**Session Prep** is freeform text with @mentions. The GM writes plans and contingencies: "If the players go to @Brittany then the @Duc Croissant will lay siege to @Burgundy." No relationships are created. No structured data. It is a writing surface only. The @mentions give the AI signal about which entities are relevant to the upcoming session.

**Session Recording** is the audio of what actually happened at the table. This is the primary source of truth for what occurred during play.

**Player Notes** are optional uploads from players. Additional signal for the AI.

**Journal** is the canonical output. The AI reconciles session prep against the session recording and player notes, produces a narrative summary, and proposes relationship changes. The GM reviews and accepts or rejects each proposal.

The AI is the primary author of relationship changes. It proposes mutations using the same five operations available to the GM (create, end, replace, retcon, delete). Every AI proposal is gated by GM approval. The AI most commonly proposes create, replace, and end. Retcon and delete proposals are rare because the AI seldom has sufficient context to know something was never true or was an error, but there is no artificial restriction on which operations it can propose.

## The Graph Model

### The graph is two things

**Things** are the nodes. Entities, events, locations, factions, items. A Thing has a page (a CRDT document, collaboratively edited via TipTap/Loro).

**Relationships** are the edges. A Relationship always connects exactly two Things. Multi-entity interactions are modeled as an Event (which is a Thing) with multiple Relationships pointing at it.

### Relationships are bidirectional

A Relationship captures both directions in a single row with a forward predicate and a reverse predicate:

```
John  --[is a resident of | is the home of]-->  Townsville
```

One row, one relationship, two directions.

### Relationship schema

```
id:                  UUID (primary key)
thing_a:             FK -> things
thing_b:             FK -> things
predicate_a_to_b:    TEXT (immutable after creation)
predicate_b_to_a:    TEXT (immutable after creation)
visibility:          ENUM { gm, players }
origin:              ENUM { prior, session(FK) }
created_at:          TIMESTAMP (when the row was written, debug/audit only)
invalidated_by:      FK -> sessions NULLABLE
invalidated_at:      TIMESTAMP NULLABLE (when the row was invalidated, debug/audit only)
invalidation_reason: ENUM { superseded, retconned } NULLABLE
content:             JSONB (history blob, opened only on demand)
```

### Predicates are immutable

A Relationship row is born with its predicates and dies with those predicates. Predicates never change. When a relationship evolves, either a new row is created alongside it (augmentation) or the old row is invalidated and a new row replaces it (supersession). This distinction is always a GM decision.

## Origin

The `origin` field records where a fact came from.

- `Prior`: true before the campaign started. The primordial world state.
- `Session(n)`: became true in the context of session n. This includes relationships extracted from session recordings, post-session clarifications, and GM prep that the GM confirmed into the graph.

Origin is always present, never nullable, and immutable.

### Sessions are the unit of knowledge time

A session is the atomic unit of knowledge time. All relationship mutations proposed by the AI are tagged with the session that produced them. All temporal queries operate on session identity.

GM prep is freeform text written into the upcoming session's prep. It contains no relationships. When the journal pipeline processes the session, the AI uses the prep as context alongside the recording and player notes. Relationships enter the graph only when the AI proposes them and the GM accepts.

There are no interstitial sessions. There is no "next session" sentinel. The GM preps by writing into the prep for the upcoming session. The session exists the moment the GM starts planning.

## How Relationships End

**Superseded.** Narrative progression. The relationship was true and is no longer because the fiction moved forward. Invalidated with `reason: superseded`. Remains visible in historical snapshots because it was true at the time. This is the only invalidation type the AI can propose (via propose replace or propose terminate).

**Retconned.** The GM declares this was never true in the fiction, even if it was established in a prior session. Invalidated with `reason: retconned`. Excluded from historical snapshots. The row is kept in the database because retcons are part of the tapestry of the game. GM-only operation.

**Deleted.** Hard delete, no audit trail. The relationship should never have existed. Two cases only:
- The GM changed their mind about a GM-only relationship that was never established during play.
- The AI proposed something incorrect and the GM accidentally accepted it.

Deletion is not an invalidation. There is no `corrected` enum value. GM-only operation.

## GM Manual Tools

The GM can bypass the journal pipeline and manage relationships directly. Five operations:

- **Create.** Add a new relationship between two Things. If a relationship already exists between the pair, the new one coexists alongside it. There is no separate "augment" operation.
- **End.** The relationship was true and is no longer. Invalidated with `reason: superseded`. No replacement is created.
- **Replace.** End an existing relationship and create a new one in a single gesture. Shortcut for end + create.
- **Retcon.** Invalidate a relationship as never having been true in the fiction.
- **Delete.** Hard delete a relationship that should never have existed.

These operations use the same schema and temporal model as AI-proposed changes. The only difference is that they are not gated by an approval step.

## Visibility

Two-value enum for now: `gm` and `players`. Visibility is mutable: the GM can reveal or hide relationships at any time without triggering invalidation. Origin answers "when did this become true." Visibility answers "who knows about it." These are independent concerns.

Per-player visibility (e.g. the Bard's Dark Patron) is a future expansion, likely via a grants table.

## Temporal Queries

### Snapshot

"Show me session 10" returns all Relationships where:

```sql
(origin = 'prior' OR origin.session <= 10)
AND invalidation_reason IS DISTINCT FROM 'retconned'
AND (invalidated_by IS NULL OR invalidated_by.session > 10)
```

Filtered by `visibility` for player-facing views. The AI sees everything unfiltered.

### Diff

"What changed in session 14" returns all Relationships where:

```sql
origin = Session(14) OR invalidated_by = Session(14)
```

## In-Memory Representation

petgraph holds the current-state graph in memory. Relationships are loaded from the indexed columns on startup. The history blob is not loaded. All traversal happens in petgraph. The database is write-behind cold storage.

## Names

Entity names are an alias list on the Thing, not a temporal relationship. Names accumulate rather than replace. A display name pointer determines what the UI shows. All aliases are indexed for search.

## Consequences

- The AI is the primary author of relationship changes. The journal pipeline (prep + recording + player notes -> AI proposals -> GM approval) is the main path by which the graph evolves.
- GM manual tools exist as an escape hatch for direct manipulation, retcons, and error correction.
- Every relationship has a non-nullable, immutable origin. No timestamp-range inference.
- Sessions are the atomic unit of knowledge time. Snapshot and diff queries operate on session identity.
- One row per predicate pair per entity pair. Multiple concurrent relationships between two Things are multiple rows.
- The GM decides whether a new relationship supersedes or augments an existing one. The AI always proposes replacements; the GM can downgrade a replacement to an augmentation by rejecting the supersession and accepting only the new row.
- Retconned facts vanish from the fictional timeline but remain in the database.
- Factual errors are hard-deleted with no trace.
- Visualization of the graph through time is supported by stepping the snapshot query through session boundaries.
- The bidirectional predicate pair ensures graph traversal works naturally from either direction without duplicate rows.
- Session prep contains no structured data. @mentions provide entity signal to the AI but do not create or modify relationships.
