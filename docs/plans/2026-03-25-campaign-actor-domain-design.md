# ADR: Campaign Actor Domain Design

**Status:** Draft (rev 2)
**Date:** 2026-03-25 (rev 2: 2026-04-02)
**Supersedes:** None (new decision area; refines and extends [Hocuspocus Architecture ADR](../archive/plans/2026-03-14-hocuspocus-architecture.md))
**Related decisions:** [AI Serialization Format v2](./2026-03-25-ai-serialization-format-v2.md), [Hocuspocus Architecture ADR](../archive/plans/2026-03-14-hocuspocus-architecture.md), [AI Workflow Unification](./2026-02-14-ai-workflow-unification-design.md)

### Key External Dependencies

| Dependency               | Role                                                                                                                                                                                                  | Links                                                                                                                                                                                                                                                                                                                                      |
| ------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Loro**                 | CRDT library. Each ThingActor, TocActor, and AgentConversation holds a LoroDoc.                                                                                                                       | [loro-dev/loro](https://github.com/loro-dev/loro) · [docs](https://loro.dev/docs)                                                                                                                                                                                                                                                          |
| **loro-dev/protocol**    | Transport-agnostic CRDT sync protocol. Room-based multiplexing, 256KB message limit, fragmentation, ack/error semantics. The wire format between clients and the Rust backend.                        | [repo](https://github.com/loro-dev/protocol) · [protocol spec](https://github.com/loro-dev/protocol/blob/main/protocol.md) · [Rust crate source](https://github.com/loro-dev/protocol/tree/main/rust/loro-protocol/src) · [protocol.rs (message types)](https://github.com/loro-dev/protocol/blob/main/rust/loro-protocol/src/protocol.rs) |
| **kameo**                | Rust actor framework. Typed actor refs, async message passing, supervision trees. Each actor in the topology is a kameo actor.                                                                        | [tqwewe/kameo](https://github.com/tqwewe/kameo) · [docs](https://docs.rs/kameo)                                                                                                                                                                                                                                                            |
| **axum**                 | HTTP/WebSocket server. Handles the WS upgrade, REST endpoints, and spawns per-connection read/write tasks.                                                                                            | [tokio-rs/axum](https://github.com/tokio-rs/axum) · [docs](https://docs.rs/axum)                                                                                                                                                                                                                                                           |
| **petgraph**             | In-memory graph representation for the RelationshipGraph actor. Loaded at campaign checkout, ~500 nodes / ~2,000 edges.                                                                               | [petgraph/petgraph](https://github.com/petgraph/petgraph) · [docs](https://docs.rs/petgraph)                                                                                                                                                                                                                                               |
| **libSQL / Turso**       | Campaign database. Database-per-campaign as isolated `.db` files. Turso is the identified upgrade path (MIT-licensed Rust rewrite of SQLite with `BEGIN CONCURRENT` and native vector search).        | [tursodatabase/libsql](https://github.com/tursodatabase/libsql)                                                                                                                                                                                                                                                                            |
| **TipTap / ProseMirror** | Frontend rich text editor. The LoroDoc content must round-trip through ProseMirror's document model. TipTap extensions define custom node types (suggestion marks, transclusion blocks, etc.).        | [ueberdosis/tiptap](https://github.com/ueberdosis/tiptap) · [TipTap docs](https://tiptap.dev/docs) · [TipTap comments (architectural reference for suggestion marks)](https://tiptap.dev/docs/comments/getting-started/overview)                                                                                                           |
| **loro-prosemirror**     | Official ProseMirror binding for Loro. Provides `LoroSyncPlugin` (bidirectional doc ↔ editor sync), `LoroUndoPlugin`, `LoroEphemeralCursorPlugin`. TipTap compatible. Validated in prior integration. | [loro-dev/loro-prosemirror](https://github.com/loro-dev/loro-prosemirror)                                                                                                                                                                                                                                                                  |
| **Hetzner**              | Compute (CX22, hel1 datacenter), object storage (campaign DB source of truth), volumes (local NVMe cache).                                                                                            | [hetzner.com](https://www.hetzner.com)                                                                                                                                                                                                                                                                                                     |
| **Nebius**               | GPU inference for open-weight LLMs. Finnish infrastructure, EU data residency.                                                                                                                        | [nebius.com](https://nebius.com)                                                                                                                                                                                                                                                                                                           |

### Key Internal References

| Document                                                                              | What it decides                                                                                                                                                                                            |
| ------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [AI Serialization Format v2](./2026-03-25-ai-serialization-format-v2.md)              | The markdown format agents read/write, progressive disclosure tiers, suggestion model (marks on blocks), compiler interface, tool signatures                                                               |
| [Hocuspocus Architecture ADR](../archive/plans/2026-03-14-hocuspocus-architecture.md) | The Node.js/Yjs architecture this design replaces. Campaign checkout/checkin model, blob-free files at rest, lossless reconstruction requirement. Many concepts carry forward; the implementation changes. |
| [AI Workflow Unification](./2026-02-14-ai-workflow-unification-design.md)             | The three AI workflows (SessionIngest, P&R, Q&A), suggestion lifecycle, conversation system                                                                                                                |
| [AI PRD](./2026-02-22-ai-prd.md)                                                      | Tool system, suggestion types, retrieval capabilities                                                                                                                                                      |
| [Templates as Prototype Pages](./2026-02-20-templates-as-prototype-pages.md)          | Templates are Things, categorization via tags-as-relationships, OnCreate directives                                                                                                                        |

---

## Context

Loreweaver is moving from a Node.js/Hocuspocus collaboration layer to a Rust backend built on kameo (actor framework) and Loro (CRDT library), with the loro-dev/protocol crate handling wire-level sync. This document defines the actor topology, trait system, and interaction patterns that replace Hocuspocus's role as the collaboration and persistence layer.

The Node.js architecture had specific constraints — single-threaded event loop, shared memory pressure across all documents, Y.Doc lifecycle tightly coupled to Hocuspocus hooks — that drove decisions like "don't load Y.Docs for read-only access" and "two write paths (WebSocket for active, HTTP for inactive)." The Rust actor model eliminates these constraints. Each actor is an independent async task. Loading a document in one actor has zero impact on any other actor. This changes what is simple and what is complex, which changes the right design.

The loro-dev/protocol defines a transport-agnostic CRDT sync protocol with room-based multiplexing over a single connection. Each room has a CRDT type (`%LOR` for Loro documents, `%EPH` for ephemeral stores, etc.), a room ID, and a message vocabulary: JoinRequest/JoinResponseOk/JoinError, DocUpdate (with batching and fragmentation), Ack, RoomError, and Leave. The protocol supports overlapping room membership on a single connection — a client can join multiple rooms simultaneously.

### Constraints

- **Long-term reasoning over short-term convenience.** Operational complexity matters, but the right abstraction is worth the upfront cost if it prevents larger problems later. Don't optimize for "easy to build first time" at the expense of "easy to reason about in six months."
- **Campaign-as-file isolation.** Each campaign is a self-contained libSQL database file. All actors for a campaign operate against the same file. Cross-campaign interaction is architecturally impossible.
- **"AI proposes, GM disposes."** The AI never modifies the campaign graph directly. All AI output is provisional until explicitly accepted.
- **EU/EEA infrastructure.** All compute and data stays in EU/EEA. LLM inference runs on Nebius (Finnish infrastructure). Claude never sees user data.

---

## Decision

### Actor Topology

A checked-out campaign has the following actor tree:

```
CampaignSupervisor (one per checked-out campaign)
├── CampaignVocabulary (one per campaign — entity name lookup service)
├── ThingActor (per active Thing — NPC page, location page, etc.)
├── TocActor (one per campaign — the GM's organizational structure)
├── RelationshipGraph (one per campaign — the full entity graph)
├── UserSession (per connected user)
│   ├── AgentConversation (per conversation — P&R, Q&A, etc.)
│   ├── AgentConversation
│   └── ...
```

#### Why these are the actor boundaries

**ThingActor** is an actor because each Thing has an independent lifecycle (loaded on demand, evicted on idle), holds a LoroDoc that syncs with connected editors via the CRDT protocol, and has state that must be protected from concurrent access. Two users editing different Things should never contend.

**TocActor** is an actor because the table of contents is a user-authored organizational structure — not a materialized view derivable from Thing metadata. Each campaign's organizational hierarchy is arbitrary and game-specific (planets → spaceports → NPCs in Star Wars, kingdoms → cities → guilds in fantasy). The ToC is itself a collaborative document that syncs via CRDT, with the same lifecycle semantics as a ThingActor (persistence, eviction, real-time sync). Reconciliation with Thing creation/deletion is necessary regardless — the same infrastructure that reconciles AI-proposed entities handles ToC dangling references.

**RelationshipGraph** is a dedicated actor (not owned by the CampaignSupervisor) because graph queries are on the hot path for AI context building and the serialization compiler. At campaign scale (~500 nodes, ~2,000 edges), the full graph loads into memory at checkout time (trivially small — roughly 100KB). The actor owns the in-memory petgraph representation and the persistence path back to libSQL. It is NOT a CRDT room — relationships are server-authoritative, mutated via REST, with change notifications broadcast over the websocket side-channel.

**Why the full graph in memory, not partial loading:** The AI agent's context-building pass traverses relationships for entities that are overwhelmingly not being edited. "What do we know about Kael? What's his relationship to Dantooine?" is a multi-hop query touching inactive entities. If the graph only held edges for active Things, every AI context query would fall through to the database. At 2,000 edges, the in-memory representation costs nothing and saves the complexity of a partial-loading lifecycle. If campaigns grow to 10,000+ nodes (unlikely — that's an enormous campaign), lazy loading can be added then.

**Why not SurrealDB or a graph database:** ~500 nodes and ~2,000 relationships per campaign is solved by recursive CTEs on SQLite. A graph database would add an operational dependency for ergonomic gains that don't manifest at this scale. petgraph in memory gives the traversal performance. libSQL gives the persistence and portability (campaign-as-file). The combination is simpler to operate than any graph database.

**UserSession** is an actor because it carries user-scoped state (role, permissions, active conversations), has its own lifecycle (connect → idle → reconnect → disconnect), and is the natural supervision boundary for AgentConversations. The alternative — the CampaignSupervisor tracking user state directly — dilutes the supervisor's campaign-level responsibilities with per-user concerns.

**AgentConversation** is an actor because each conversation is a stateful, long-lived interaction with independent lifecycle management. A conversation:

1. Connects to an LLM inference endpoint (Nebius)
2. Runs the serialization compiler to build prompts and apply suggestions
3. Routes compiled suggestions to the correct ThingActor
4. Manages progressive disclosure context construction (which Things at which retrieval tier)
5. Holds conversation state for P&R or Q&A sessions
6. Accepts user messages for this specific conversation
7. Carries a conversation ID that stamps provenance onto every suggestion it produces

Each user has many conversations. Opening an existing conversation or starting a new one spins up a new AgentConversation actor. Conversations persist to the campaign database for "hammock time" — the user can close a conversation, come back days later, and resume with full history.

**CampaignSupervisor** is the root actor. It handles campaign checkout/checkin from object storage, spawns and tracks all child actors, routes incoming websocket messages to the correct room actor, and manages the campaign-level database connection. It does not implement any domain traits — it is pure orchestration.

---

### CampaignVocabulary Actor

CampaignVocabulary is an in-memory projection of all entity names in the campaign. It is a sibling of RelationshipGraph under the CampaignSupervisor, loaded at restoration time, and lives for the lifetime of the campaign checkout.

#### Internal State

```rust
struct CampaignVocabulary {
    entries: Vec<VocabularyEntry>,
    subscribers: Vec<Subscriber>,
    // index structures (phonetic keys, n-grams, etc.) added as needed
}

struct VocabularyEntry {
    thing_id: ThingId,
    canonical_name: String,
}
```

At ~500 entities this is trivially small. Matching strategies start simple (normalized substring for autocomplete, Levenshtein distance for fuzzy matching) and can be made more sophisticated independently without changing the actor's interface or role in the architecture.

#### Query Interface

```rust
enum VocabularyQuery {
    /// Editor autocomplete. Prefix/substring match against entity names.
    Mention { prefix: String, limit: usize },
    /// STT correction and entity recognition. Fuzzy match a candidate
    /// string against the campaign vocabulary.
    FuzzyMatch { candidate: String, threshold: f32 },
}
```

Both queries are the same shape: string in, matches out. The vocabulary actor is a lookup service. It does not scan text or extract candidates — consumers that need entity recognition (STT correction pipeline, AI context building) call FuzzyMatch repeatedly with their own candidate tokens. The scanning and extraction logic lives in the pipeline stage, not the vocabulary.

#### Consumers

**Editor autocomplete:** A REST endpoint queries the vocabulary actor with `Mention`. Interactive latency. The primary user-facing consumer.

**STT correction (pipeline phase 2):** The correction dictionary that normalizes ASR output against known campaign entity names. "Yorgath" needs to find "Jorgath." Calls `FuzzyMatch` for each candidate token the ASR produced. The vocabulary actor handles the matching; the pipeline stage owns the logic of which tokens to check.

**AI context building:** The serialization compiler uses the vocabulary for name matching when it encounters mentions in documents. The full entity list for prompt headers comes from `CampaignReader` (the debounce freshness gap is acceptable for AI prompts — a Thing created 2 seconds ago not appearing in the next prompt is fine).

#### Event-Driven Freshness

The CampaignSupervisor publishes domain events to the vocabulary actor as regular kameo messages. The vocabulary actor does not subscribe to anything — it receives events from the supervisor, which already mediates Thing lifecycle.

```rust
// Messages from CampaignSupervisor
struct ThingCreated(ThingHandle);
struct ThingRenamed { id: ThingId, new_name: String }
struct ThingDeleted(ThingId);
```

The vocabulary is always immediately fresh. No polling, no index rebuild, no DB reads on the hot path after initial restoration.

#### Notifications to Clients

CampaignVocabulary implements `Notifiable` because clients need to know when the vocabulary changes independently of any specific document update. The editor's mention popup, any open search UI, and anything that displays entity names outside of a document context all need this notification.

```rust
enum VocabularyNotification {
    ThingCreated(ThingHandle),
    ThingRenamed { id: ThingId, new_name: String },
    ThingDeleted(ThingId),
}
```

#### Trait Composition

| Trait     | Implements? | Why                                                                                           |
| --------- | ----------- | --------------------------------------------------------------------------------------------- |
| Notifiable | yes        | Clients need vocabulary change notifications independently of document updates                 |
| Queryable  | yes        | REST endpoints and pipeline stages query for mentions and fuzzy matches                       |
| Persistent | no         | Derived entirely from Thing data. No independent state to write back.                         |
| Evictable  | no         | Too cheap to evict and too widely depended on to risk being absent. Lives for campaign lifetime. |
| CrdtRoom   | no         | Server-authoritative, not collaborative.                                                      |
| Mutable    | no         | Does not accept external commands via REST. Receives domain events from the supervisor.        |

---

### Mention Model

Mentions in the LoroDoc store a `ThingId` and a display label. The relational data (libSQL) stores only the `ThingId` as a foreign key. The display label does not exist in the database.

```
LoroDoc (live editing):   { type: "mention", attrs: { thingId: "abc123", label: "Korgath" } }
Relational (persistence): thing_id = "abc123"  (no label column)
```

The label is a rendering cache, not a source of truth. Every layer of the system treats it this way.

#### Rename Propagation

When a GM renames a Thing (e.g., "Korgath" → "Kurgath"):

1. ThingActor (for Korgath's own page) processes the rename
2. ThingActor tells CampaignSupervisor: `ThingRenamed { id, new_name }`
3. Supervisor tells CampaignVocabulary: update the entry
4. Supervisor tells active ThingActors: `MentionRenamed { thing_id, new_name }` — each walks its live LoroDoc and updates matching mention label attributes
5. CampaignVocabulary notifies connected clients via `VocabularyNotification::ThingRenamed`

**Inactive Things require no propagation.** Their relational data stores only the ThingId. When an inactive Thing is next restored, `restore()` resolves mention ThingIds to current names using the CampaignVocabulary (or CampaignReader if the vocabulary isn't up yet). The reconstructed LoroDoc gets the correct label at reconstruction time.

**The RelationshipGraph requires no update.** It stores edges between ThingIds. Entity names never appear there.

#### Recovery Semantics

The mention model gives a spectrum of recovery quality rather than a binary works/broken:

**Normal operation:** Vocabulary actor is up, full reconstruction from relational data. Every mention resolves to the current name. Perfect fidelity.

**Hard restart from hot LoroDoc snapshot, vocabulary available:** Mention labels in the snapshot might be stale (a rename happened after the last snapshot). A reconciliation pass on load can fix them, or they fix on next edit. IDs are correct. Cosmetically stale, not structurally wrong.

**Hard restart from hot LoroDoc snapshot, vocabulary not yet available:** Stale labels, no way to fix them immediately. Pages render, mentions are clickable (valid IDs), names might be wrong. When the vocabulary comes up, the next restore or edit fixes them.

No recovery path requires special ceremony. The mention's truth is always the ThingId. The label is a cache that is correct when convenient and harmlessly stale otherwise.

---

### CampaignDatabase Module

The campaign database is encapsulated as a module, not exposed as raw connections. The `CampaignDatabase` struct is the module's public face — the CampaignSupervisor holds it and passes its read and write handles to child actors. No actor outside the module ever sees a connection, a query, or a row.

```rust
/// The module's public face. The supervisor holds this.
pub struct CampaignDatabase {
    reader: CampaignReaderImpl,
    writer: ActorRef<DatabaseActor>,
    path: PathBuf,
}
```

#### Read Algebra

Reads go through a trait that speaks the domain language. The implementation holds a pool of read-only libSQL connections (WAL mode allows concurrent readers). The trait is `Clone + Send + Sync` so it can be handed to every actor at spawn time.

```rust
trait CampaignReader: Clone + Send + Sync + 'static {
    async fn restore_thing(&self, id: &ThingId) -> Result<ThingSnapshot>;
    async fn restore_toc(&self) -> Result<TocSnapshot>;
    async fn restore_graph(&self) -> Result<GraphSnapshot>;
    async fn restore_conversation(&self, id: &ConversationId)
        -> Result<ConversationSnapshot>;
    async fn list_thing_handles(&self) -> Result<Vec<ThingHandle>>;
}
```

This is where all SELECT queries live. Adding a new actor type means adding one method to the algebra and one implementation in the persistence module. No SQL leaks into actor code.

#### Write Actor

Writes are domain-typed commands sent to a `DatabaseActor` that owns the single read-write connection. The actor's mailbox serializes writes — one message processed at a time — but this is a convenience for clean shutdown draining, not a correctness requirement. libSQL in WAL mode with `busy_timeout = 5000` already serializes writers at the database level. The actor prevents the CampaignSupervisor from blocking on IO during debounce writebacks.

Snapshot types carry their own identity and convert into persistence commands:

```rust
struct ThingSnapshot {
    id: ThingId,
    blocks: Vec<BlockRecord>,
    mentions: Vec<MentionRecord>,
    suggestions: Vec<SuggestionRecord>,
}

impl From<ThingSnapshot> for PersistenceCommand {
    fn from(s: ThingSnapshot) -> Self {
        PersistenceCommand::SnapshotThing(s)
    }
}

struct TocSnapshot {
    entries: Vec<TocEntry>,
}

impl From<TocSnapshot> for PersistenceCommand {
    fn from(s: TocSnapshot) -> Self {
        PersistenceCommand::SnapshotToc(s)
    }
}

struct ConversationSnapshot {
    id: ConversationId,
    messages: Vec<MessageRecord>,
}

impl From<ConversationSnapshot> for PersistenceCommand {
    fn from(s: ConversationSnapshot) -> Self {
        PersistenceCommand::SnapshotConversation(s)
    }
}
```

The `PersistenceCommand` enum takes snapshots directly:

```rust
enum PersistenceCommand {
    SnapshotThing(ThingSnapshot),
    SnapshotToc(TocSnapshot),
    SnapshotConversation(ConversationSnapshot),
    SnapshotGraph(GraphMutation),               // delta, not snapshot
    RecordSuggestionOutcome(SuggestionOutcome),  // point write, not snapshot
}

struct DatabaseActor {
    write_conn: libsql::Connection,
}
```

`SnapshotGraph` and `RecordSuggestionOutcome` are not produced by `Persistent` trait impls. The graph is mutated via deltas. Suggestion outcomes are point writes triggered by domain events. Both are covered by the `dead_code` lint (must be constructed somewhere) and the exhaustive match in the DatabaseActor handler (must be handled). See the Trait System section for how `Persistent` connects to `PersistenceCommand`.

Each child actor holds an `ActorRef<DatabaseActor>`. When a debounce timer fires, the actor snapshots its state and enqueues a write. `tell()` resolves when the message lands in the DatabaseActor's mailbox (a channel send, microseconds). It does not wait for the write to complete.

```rust
// Inside any Persistent + Evictable actor (ThingActor shown)
// tell() enqueues only; the write is async in the DatabaseActor.
async fn persist(&mut self) {
    let command: PersistenceCommand = self.snapshot().into();
    match self.db_writer.tell(command).await {
        Ok(_) => {
            self.dirty = false;
            if self.persistence_degraded {
                self.persistence_degraded = false;
                self.notify(ThingNotification::PersistenceRestored);
            }
        }
        Err(e) => {
            tracing::error!(thing_id = %self.id, error = %e,
                "DatabaseActor unreachable, snapshot queued for retry");
            if !self.persistence_degraded {
                self.persistence_degraded = true;
                self.notify(ThingNotification::PersistenceDegraded);
            }
            // dirty remains true, next debounce tick retries
        }
    }
}
```

**Why log and notify rather than silently discard:** A `let _ =` pattern would silently drop send errors. If the DatabaseActor is dead, every snapshot is lost. For debounce writes this is recoverable -- the LoroDoc is still in memory, `dirty` remains true, and the next debounce tick retries. But silent loss means nobody knows the write path is broken until data is actually lost on eviction or shutdown. Logging makes the failure visible in observability. The notification makes it visible to the user. The `persistence_degraded` flag ensures both fire once per degradation episode, not on every tick.

#### Storage Backend

Where the libSQL file physically lives — local filesystem vs. object storage — is a separate concern from what gets written to it. A `CampaignStore` algebra abstracts the storage lifecycle:

```rust
trait CampaignStore: Send + Sync + 'static {
    async fn checkout(&self, campaign_id: &CampaignId) -> Result<PathBuf>;
    async fn writeback(&self, campaign_id: &CampaignId, path: &Path) -> Result<()>;
    async fn release(&self, campaign_id: &CampaignId, path: &Path) -> Result<()>;
}
```

- **Local (self-hosted):** `checkout` returns the path on disk. `writeback` and `release` are no-ops. The file is already where it needs to be.
- **Hosted:** `checkout` downloads from Hetzner Object Storage to the local Hetzner Volume. `writeback` uploads the current file for durability (called on a periodic timer — ~30 seconds). `release` does a final upload and deletes the local copy.

The CampaignSupervisor owns the `CampaignStore`. The `CampaignDatabase` module consumes it during checkout and release but does not hold a reference to it — the storage lifecycle is the supervisor's responsibility, the connection lifecycle is the module's.

#### Module Lifecycle

```rust
impl CampaignDatabase {
    /// Downloads the campaign file (if hosted), opens connections,
    /// spawns the write actor. Returns when the database is ready.
    pub async fn checkout(
        store: &impl CampaignStore,
        campaign_id: &CampaignId,
    ) -> Result<Self> {
        let path = store.checkout(campaign_id).await?;
        let read_pool = open_read_pool(&path).await?;
        let write_conn = open_write_connection(&path).await?;
        let writer = kameo::spawn(DatabaseActor { write_conn });
        Ok(Self {
            reader: CampaignReaderImpl::new(read_pool),
            writer,
            path,
        })
    }

    pub fn reader(&self) -> &CampaignReaderImpl { &self.reader }
    pub fn writer(&self) -> &ActorRef<DatabaseActor> { &self.writer }

    /// Drains pending writes, does final writeback, releases
    /// the file. Consumes self — use after release is a compile error.
    pub async fn release(
        self,
        store: &impl CampaignStore,
        campaign_id: &CampaignId,
    ) -> Result<()> {
        self.writer.stop_gracefully().await?;
        store.release(campaign_id, &self.path).await?;
        Ok(())
    }
}
```

`release` consuming `self` is intentional. The type system enforces that no reads or writes can happen after release.

#### Module Boundary

All SQL, row mapping, and schema knowledge lives in a single `persistence` module. Nothing outside this module touches a connection or a row.

```
campaign/
├── actors/
│   ├── supervisor.rs
│   ├── thing.rs
│   ├── toc.rs
│   ├── graph.rs
│   ├── vocabulary.rs
│   ├── session.rs
│   └── conversation.rs
├── persistence/
│   ├── mod.rs          // CampaignDatabase, re-exports
│   ├── reader.rs       // CampaignReader trait + CampaignReaderImpl
│   ├── writer.rs       // DatabaseActor, PersistenceCommand
│   ├── restore.rs      // restore_thing(), restore_graph(), etc.
│   ├── schema.rs       // table definitions, migrations
│   └── snapshots.rs    // ThingSnapshot, TocSnapshot, From impls
├── compiler/           // serialization compiler
└── domain.rs           // ThingId, BlockId, etc.
```

---

### Campaign Startup Lifecycle

kameo actors process one message at a time. The `handle` method is async, but awaiting inside a handler yields the thread back to the tokio runtime, not the actor's mailbox. Other messages queue until the handler returns. If checkout takes 2-3 seconds (object storage download, connection setup, graph restoration), a synchronous startup would block the supervisor's mailbox — heartbeats queue up, the platform thinks the server is dead.

The startup is interrupt-driven: the supervisor spawns checkout as a background task, returns immediately, and receives a completion message when the database is ready. A separate timeout races against the completion.

#### Supervisor State Machine

```rust
enum SupervisorState {
    /// Checkout in progress. Heartbeats respond. Room joins rejected.
    Starting,
    /// Actors being restored from the database.
    Restoring { db: CampaignDatabase },
    /// Normal operation.
    Ready { db: CampaignDatabase },
    /// Child actors draining before release.
    Draining,
}
```

Note: `Starting` and `Restoring` are separate states. `Starting` means the database file is being downloaded and connections are being opened. `Restoring` means the database is ready but child actors (RelationshipGraph, TocActor, etc.) are being spawned and populated. Both are non-blocking. Both respond to heartbeats with their current phase.

#### Startup Sequence

```rust
impl Message<CheckoutCampaign> for CampaignSupervisor {
    type Reply = ();
    async fn handle(&mut self, _msg: CheckoutCampaign, ctx: Context<'_, Self, Self::Reply>) {
        let store = self.store.clone();
        let campaign_id = self.campaign_id.clone();
        let self_ref = ctx.actor_ref().clone();

        // Spawn the checkout as a background task
        tokio::spawn(async move {
            match CampaignDatabase::checkout(&*store, &campaign_id).await {
                Ok(db) => { self_ref.tell(CheckoutComplete(db)).await.ok(); }
                Err(e) => { self_ref.tell(CheckoutFailed(e)).await.ok(); }
            }
        });

        // Race a timeout against the completion
        tokio::spawn({
            let self_ref = ctx.actor_ref().clone();
            async move {
                tokio::time::sleep(CHECKOUT_TIMEOUT).await;
                self_ref.tell(CheckoutTimedOut).await.ok();
            }
        });

        self.state = SupervisorState::Starting;
    }
}
```

#### Completion Transitions

```rust
impl Message<CheckoutComplete> for CampaignSupervisor {
    type Reply = ();
    async fn handle(&mut self, msg: CheckoutComplete, ctx: Context<'_, Self, Self::Reply>) {
        let SupervisorState::Starting = &self.state else { return; };
        let db = msg.0;

        self.state = SupervisorState::Restoring { db };

        // Spawn actor restoration as another background task
        let reader = self.db().reader().clone();
        let writer = self.db().writer().clone();
        let self_ref = ctx.actor_ref().clone();
        tokio::spawn(async move {
            match restore_campaign_actors(&reader, &writer).await {
                Ok(actors) => { self_ref.tell(RestoreComplete(actors)).await.ok(); }
                Err(e) => { self_ref.tell(RestoreFailed(e)).await.ok(); }
            }
        });
    }
}

impl Message<RestoreComplete> for CampaignSupervisor {
    type Reply = ();
    async fn handle(&mut self, msg: RestoreComplete, _ctx: Context<'_, Self, Self::Reply>) {
        let SupervisorState::Restoring { db } = std::mem::replace(
            &mut self.state, SupervisorState::Starting // placeholder
        ) else { return; };

        self.actors = msg.0;
        self.state = SupervisorState::Ready { db };
    }
}

impl Message<CheckoutTimedOut> for CampaignSupervisor {
    type Reply = ();
    async fn handle(&mut self, _msg: CheckoutTimedOut, _ctx: Context<'_, Self, Self::Reply>) {
        // If we're already Ready or Restoring, the timeout lost the race. Ignore it.
        let SupervisorState::Starting = &self.state else { return; };
        // Log, notify platform, terminate
    }
}
```

The loser of the race is always a no-op. `CheckoutComplete` arrives after timeout? Supervisor is no longer in `Starting`, early return. Timeout arrives after completion? Same.

#### Heartbeats Report Phase

```rust
impl Message<Heartbeat> for CampaignSupervisor {
    type Reply = HeartbeatAck;
    async fn handle(&mut self, _msg: Heartbeat, _ctx: Context<'_, Self, Self::Reply>) -> HeartbeatAck {
        HeartbeatAck {
            campaign_id: self.campaign_id.clone(),
            phase: match &self.state {
                SupervisorState::Starting   => CampaignPhase::Downloading,
                SupervisorState::Restoring { .. } => CampaignPhase::Restoring,
                SupervisorState::Ready { .. }     => CampaignPhase::Ready,
                SupervisorState::Draining   => CampaignPhase::Draining,
            }
        }
    }
}
```

The platform forwards the phase to connected clients. A client that connects while the campaign is starting sees "Downloading campaign data..." then "Restoring entities..." then the editor loads. This turns an otherwise opaque wait into descriptive progress. The phases can be made more granular later (e.g., `Restoring` could report which actors have been spawned) without changing the state machine structure.

#### Room Joins Gate on Ready

```rust
impl Message<JoinRoom> for CampaignSupervisor {
    type Reply = Result<RoomHandle, JoinError>;
    async fn handle(&mut self, msg: JoinRoom, _ctx: Context<'_, Self, Self::Reply>) -> Result<RoomHandle, JoinError> {
        let SupervisorState::Ready { db } = &self.state else {
            return Err(JoinError::CampaignNotReady);
        };
        // ... normal room join logic using db.reader() and db.writer()
    }
}
```

Clients that attempt to join rooms before the campaign is ready receive `CampaignNotReady` and can retry. The frontend uses the heartbeat phase to decide whether to show a loading indicator or an error.

#### Shutdown is the Same Pattern in Reverse

Shutdown mirrors startup: the supervisor evicts all child actors (each one snapshots via the DatabaseActor), then spawns the release as a background task. Heartbeats continue responding with `CampaignPhase::Draining` throughout. A timeout races against the release. The only difference is that after `ReleaseComplete`, the supervisor terminates itself.

---

### ThingActor Internal State

```rust
struct ThingActor {
    id: ThingId,
    doc: LoroDoc,
    subscribers: Vec<Subscriber>,
    dirty: bool,
    persistence_degraded: bool,
    last_activity: Instant,
    db_writer: ActorRef<DatabaseActor>,
}
```

`persistence_degraded` gates notification delivery (one per degradation episode, not every debounce tick) and blocks eviction (cannot safely discard in-memory state when the write path is broken). `db_writer` is the handle to the DatabaseActor for snapshot writes. The subscriber list is shared between CrdtRoom and Notifiable -- both push messages through the same outbound `mpsc::Sender` per client, multiplexed by the write_task on the websocket.

**The LoroDoc is always reconstructed on actor startup.** There is no "cold" state where the actor holds only relational data. `restore()` reads from libSQL and builds the full LoroDoc via the equivalent of `toYdoc()`. The doc is live from the moment the actor exists.

**Why no two-phase state (cold relational / hot CRDT):** The Hocuspocus architecture had two read paths (Y.Doc for active pages, libSQL for inactive pages) because loading a Y.Doc on the Node.js event loop consumed shared memory and blocked the single thread. In Rust, each actor is an independent async task. Reconstructing a LoroDoc in one actor has zero impact on any other actor. The reconstruction cost is a few milliseconds of CPU to walk relational rows and build a document tree. At campaign scale, even a context-building pass that spins up 30 ThingActors costs ~30ms of CPU and ~3MB of memory. The actors evict themselves after idle timeout.

One state representation means one read path, one write path, and no conditional logic around "do I have a doc or not." The compiler always reads from a LoroDoc. The CRDT room is always joinable. The debounce timer always has a doc to snapshot. Every code path is exercised in every scenario.

**Debounce is per-actor.** Each ThingActor manages its own persistence timer. When the timer fires, the actor snapshots its LoroDoc to relational data and writes to the campaign database. 30 active Things means 30 independent timers — they're atomic, they don't interact, and if one fires late, nothing else cares. A centralized "sweep dirty actors" tick would couple actors that have no reason to be coupled.

---

### Trait System

The trait system captures two kinds of contracts:

**Interface traits** have external consumers that interact with actors through them. The websocket read_task dispatches to CrdtRoom implementors. REST handlers query Queryable actors. The serialization compiler reads from DocumentState. These are real polymorphic boundaries, dispatched through typed enums rather than vtables.

**Pattern traits** encode repeated internal behavior. No external consumer calls them polymorphically. Their value is consistency and compile-time verification: they connect actors to the systems they participate in and let the compiler check the wiring.

#### Pattern Traits

```rust
/// Participates in the persistence system. The Into<PersistenceCommand>
/// bound connects the actor's snapshot type to the DatabaseActor's
/// command vocabulary at compile time.
///
/// restore() is NOT on this trait. Restoration is a free function in
/// the persistence module, because each actor needs different inputs
/// to reconstruct (ThingId + reader + writer, reader + writer,
/// reader only, etc.). The trait covers the write-back side only.
trait Persistent {
    type Snapshot: Into<PersistenceCommand>;
    fn snapshot(&self) -> Self::Snapshot;
    fn is_dirty(&self) -> bool;
}
```

**What the `Into<PersistenceCommand>` bound proves at compile time:**

- If a new snapshot type is defined but `From<NewSnapshot> for PersistenceCommand` is missing, the trait impl fails.
- If a new `PersistenceCommand` variant is added but nothing ever constructs it, `dead_code` flags it (warnings-as-errors makes this a build failure).
- The exhaustive match in the DatabaseActor's handler guarantees every variant is consumed.
- Together: every persistent actor produces a snapshot, every snapshot maps to a command, every command is constructed, every command is handled. The compiler proves the wiring exists end to end.

The compiler can't prove the persistence logic is *correct* -- that the SQL writes the right rows, that the snapshot captures all necessary state. But it proves the system is trying. The gaps between compile-time checkpoints are small, concrete functions in the persistence module.

**Actors that don't fit the snapshot pattern don't implement Persistent.** RelationshipGraph is mutated edge-by-edge via `Mutable`, persisted as deltas, not snapshotted whole. CampaignVocabulary is derived from Thing data and has no independent state to persist. Neither implements `Persistent`.

**Point writes that aren't snapshots** (e.g., `RecordSuggestionOutcome`) live in `PersistenceCommand` but don't come from a `Persistent` impl. The `dead_code` lint covers them independently -- if no code constructs the variant, the build fails.

```rust
/// Self-manages lifecycle based on activity. The actor tracks its own
/// idleness and decides whether it can safely be removed from memory.
/// The supervisor triggers eviction via a message; the actor decides
/// whether to comply.
trait Evictable {
    fn idle_timeout(&self) -> Duration;
    fn last_activity(&self) -> Instant;
    async fn prepare_eviction(&mut self) -> EvictionResult;
}

enum EvictionResult {
    /// Safe to evict.
    Ready,
    /// Refused. Actor holds unpersisted state and the write path is broken.
    Blocked { reason: &'static str },
}
```

**Why Evictable is a trait and not just a documented pattern:** Eviction depends on persistence state. The `prepare_eviction` logic checks whether the actor has dirty state and whether the write path is healthy. Getting this wrong (forgetting the degraded check) is a data loss bug. The trait makes the contract explicit: every evictable actor must answer "can you safely die right now?"

#### Eviction Under Persistence Degradation

```rust
// Inside any Persistent + Evictable actor
async fn prepare_eviction(&mut self) -> EvictionResult {
    if self.dirty {
        if self.persistence_degraded {
            tracing::warn!(thing_id = %self.id, "Eviction blocked: unpersisted changes");
            return EvictionResult::Blocked {
                reason: "dirty state with degraded write path"
            };
        }
        self.persist().await;
        // persist() may have failed, flipping persistence_degraded
        // and leaving dirty = true. Re-check after the attempt.
        if self.dirty {
            return EvictionResult::Blocked {
                reason: "persist failed during eviction"
            };
        }
    }
    EvictionResult::Ready
}
```

`EvictionResult::Blocked` is a signal, not a veto. The supervisor respects it temporarily, then overrides it.

**Escalation chain:**

1. **Actor refuses eviction.** Returns `Blocked`, logs a warning. Stays alive, holding unpersisted data. Debounce timer continues retrying.
2. **Supervisor tracks stuck actors** and how long they've been stuck (during shutdown drain or normal idle eviction sweeps).
3. **Supervisor escalates to the platform.** Reports: "campaign X has unpersisted changes on thing Y, actor has been stuck for Z seconds." The supervisor does not notify users directly. It reports a fact.
4. **Supervisor force-kills the actor after a deadline.** Shutdown must complete, leases must be released, the server may be going down.
5. **The platform sends a transactional email to affected users:** "Some changes on [page name] in [campaign name] may not have been saved. Please review."

The boundary is strict: **campaigns report health facts to the platform, the platform decides how to tell users.** Campaign servers don't know about email infrastructure, user contact preferences, or notification templates. User communication lives in the platform, consistent across all failure modes.

#### Persistence Health Notifications

ThingActor (and any Persistent + Evictable actor) implements `Notifiable` for persistence health:

```rust
enum ThingNotification {
    PersistenceDegraded,
    PersistenceRestored,
}
```

CrdtRoom was considered and rejected for this. CrdtRoom subscribers expect loro-dev/protocol binary frames. Persistence health isn't a CRDT operation; injecting it into the CRDT stream would break the protocol contract.

Notifiable uses the same subscriber list as CrdtRoom. Both push messages through the same outbound `mpsc::Sender` per client. The write_task multiplexes CRDT frames and notification frames, distinguished by message envelope. This is one concrete consumer of the deferred "non-CRDT side channel" design.

#### Restoration as Free Functions

Restoration is not on the `Persistent` trait. Each actor type needs different inputs to reconstruct. Putting `restore()` on the trait would require an associated context type that adds machinery without enabling polymorphic restoration code. Each actor type has a free function in the persistence module:

```rust
// In persistence/restore.rs

async fn restore_thing(
    id: ThingId,
    reader: &impl CampaignReader,
    writer: ActorRef<DatabaseActor>,
) -> Result<ThingActor> {
    let snapshot = reader.restore_thing(&id).await?;
    let doc = reconstruct_loro_doc(&snapshot)?;
    Ok(ThingActor {
        id, doc, subscribers: vec![], dirty: false,
        persistence_degraded: false, last_activity: Instant::now(),
        db_writer: writer,
    })
}

async fn restore_graph(
    reader: &impl CampaignReader,
    writer: ActorRef<DatabaseActor>,
) -> Result<RelationshipGraph> {
    let snapshot = reader.restore_graph().await?;
    let graph = build_petgraph(&snapshot)?;
    Ok(RelationshipGraph { graph, db_writer: writer, .. })
}

async fn restore_vocabulary(
    reader: &impl CampaignReader,
) -> Result<CampaignVocabulary> {
    let handles = reader.list_thing_handles().await?;
    Ok(CampaignVocabulary { entries: handles, .. })
}

/// Called during the Restoring phase of campaign startup.
async fn restore_campaign_actors(
    reader: &impl CampaignReader,
    writer: &ActorRef<DatabaseActor>,
) -> Result<CampaignActors> {
    let graph = restore_graph(reader, writer.clone()).await?;
    let toc = restore_toc(reader, writer.clone()).await?;
    let vocabulary = restore_vocabulary(reader).await?;
    Ok(CampaignActors { graph, toc, vocabulary })
}
```

The function signature documents exactly what each actor needs. CampaignVocabulary doesn't need the writer -- it's derived, not independently persistent.

#### Interface Traits

These are unchanged from the original design. Each has external consumers that interact with actors through them.

```rust
trait CrdtRoom {
    fn room_id(&self) -> &str;
    fn crdt_type(&self) -> CrdtType;
    async fn on_join(&mut self, client: ClientId, auth: &[u8])
        -> Result<JoinResponse, JoinError>;
    fn apply_update(&mut self, from: ClientId, update: &[u8])
        -> Result<(Broadcast, Ack), UpdateError>;
    fn on_leave(&mut self, client: ClientId);
    fn state_vector(&self) -> Vec<u8>;
    fn full_state(&self) -> Vec<u8>;
}

trait Notifiable {
    type Notification: Serialize;
    fn subscribe(&mut self, client: ClientId);
    fn unsubscribe(&mut self, client: ClientId);
}

trait Queryable {
    type Query;
    type Response: Serialize;
    fn query(&self, q: &Self::Query) -> Self::Response;
}

trait Mutable {
    type Command;
    type Event: Serialize;
    fn apply_command(&mut self, cmd: Self::Command)
        -> Result<Self::Event, DomainError>;
}

trait SuggestionTarget {
    fn apply_suggestion(
        &mut self,
        target_blocks: Vec<BlockId>,
        proposed: Vec<Block>,
        provenance: SuggestionProvenance,
    ) -> Result<SuggestionId>;
    fn accept_suggestion(&mut self, id: SuggestionId) -> Result<()>;
    fn reject_suggestion(&mut self, id: SuggestionId) -> Result<()>;
}

trait DocumentState {
    fn content(&self) -> &PageContent;
    fn status_tree(&self) -> &StatusTree;
    fn identity(&self) -> &ThingIdentity;
}
```

**Why serialization is NOT a trait on the actor:** The serialization compiler (`f()`) needs the actor's document state AND the campaign relationship graph AND embedding results (for Tier 2 RAG) AND the user's role (for gm_only filtering). Putting serialization on the actor would require the actor to hold references to all of those services. Instead, the compiler is a stateless service that takes `&dyn DocumentState` plus context and produces markdown. The actor exposes its state. The compiler does the work. Clean separation of concerns.

#### Trait Composition by Actor

**Interface traits** (external consumers):

| Actor              | CrdtRoom | Notifiable | Queryable | Mutable | SuggestionTarget | DocumentState |
| ------------------ | -------- | ---------- | --------- | ------- | ---------------- | ------------- |
| ThingActor         | ✓        | ✓          | ✓         |         | ✓                | ✓             |
| TocActor           | ✓        | ✓          | ✓         |         |                  |               |
| RelationshipGraph  |          | ✓          | ✓         | ✓       |                  |               |
| CampaignVocabulary |          | ✓          | ✓         |         |                  |               |
| AgentConversation  | ✓        |            |           |         |                  |               |
| UserSession        |          |            |           |         |                  |               |
| CampaignSupervisor |          |            |           |         |                  |               |

**Pattern traits** (compile-time verification):

| Actor              | Persistent | Evictable |
| ------------------ | ---------- | --------- |
| ThingActor         | ✓          | ✓         |
| TocActor           | ✓          | ✓         |
| RelationshipGraph  |            |           |
| CampaignVocabulary |            |           |
| AgentConversation  | ✓          | ✓         |
| UserSession        |            | ✓         |
| CampaignSupervisor |            |           |

**Why RelationshipGraph is not Persistent:** Mutated edge-by-edge via `Mutable`, persisted as deltas through `PersistenceCommand::SnapshotGraph(GraphMutation)`. The delta-based persistence path doesn't fit the snapshot-on-debounce pattern.

**Why CampaignVocabulary is not Persistent:** Derived entirely from Thing data. No independent state to write back.

**Why UserSession is Evictable but not Persistent:** Has a lifecycle (connect, idle, disconnect) but no state worth persisting. Session state is reconstructable from the auth token.

---

### WebSocket Architecture

#### Connection Lifecycle

Each websocket connection (one per campaign per client) gets its own pair of async tasks spawned by the axum upgrade handler: a **read task** owning the websocket read half, and a **write task** owning the write half and draining an unbounded mpsc receiver.

The read task holds a **local routing table**: `HashMap<RoomId, RoomHandle>`. This table is populated as the client joins rooms and is the hot-path dispatch mechanism — the CampaignSupervisor is NOT in the hot path for DocUpdate messages.

```rust
enum RoomHandle {
    Thing  { id: ThingId,         actor: ActorRef<ThingActor> },
    Toc    {                      actor: ActorRef<TocActor> },
    LlmStream { conversation_id: ConversationId,
                                  actor: ActorRef<AgentConversation> },
}
```

**Why `RoomHandle` is an enum, not a trait object:** kameo's `ActorRef<A>` is generic over the concrete actor type. You can't have `ActorRef<dyn CrdtRoom>`. The enum does double duty: it dispatches messages to the right typed actor AND carries enough identity to request a respawn if the actor has terminated.

Each variant implements the same logical operations (send update, register subscriber, etc.) via a match. This is a small amount of boilerplate — three match arms doing the same thing through different typed refs — but it's honest about the type system's constraints and provides a natural place to diverge per-variant later if needed.

#### Message Routing

```
Client sends JoinRequest(room_id="thing:kael")
  → read_task sends JoinRoom request to CampaignSupervisor
  → Supervisor returns existing ActorRef or spawns a new ThingActor
  → read_task stashes RoomHandle in local routing table
  → read_task registers its outbound Sender with the ThingActor
  → ThingActor replies with JoinResponseOk + full state
  → Reply flows through outbound Sender → write_task → client

Client sends DocUpdate(room_id="thing:kael", ...)
  → read_task looks up "thing:kael" in local routing table
  → read_task sends DocUpdate directly to ThingActor (no supervisor)
  → ThingActor applies update, broadcasts to other subscribers, sends Ack

Client disconnects
  → read_task iterates local routing table
  → Sends Leave to each actor, actors remove this client's Sender
  → read_task notifies CampaignSupervisor of disconnect
```

**Why the supervisor is only in the JoinRequest path:** The supervisor has campaign-level responsibilities (checkout/checkin, health monitoring, actor lifecycle). Routing every DocUpdate through it would make every keystroke contend with supervisor operations. The local routing table makes the hot path a HashMap lookup and a kameo message send — no supervisor involvement.

#### Actor Termination and Reconnection

When a ThingActor evicts itself (idle timeout), subsequent messages from any read_task holding a stale `ActorRef` will fail. The read_task detects this, sends a `RoomError(RejoinSuggested)` to the client via the loro protocol, and removes the stale entry from its routing table. The client re-joins the room with a fresh `JoinRequest`, which flows through the supervisor and spawns a new ThingActor from the database.

**Why `RejoinSuggested` rather than transparent respawn:** The actor died, which means the client's state vector may not match the newly restored actor's state. A clean rejoin via JoinRequest → full state sync is safer than applying a stale update to a fresh doc. The loro protocol designed `RejoinSuggested` precisely for this scenario.

#### Outbound Channel Design

Each websocket connection has one unbounded `mpsc::Sender`. When a client joins a room, the read_task registers a clone of this sender with the room's actor. The actor broadcasts by iterating its subscriber list and sending to each subscriber's sender. The write_task drains the receiver and sends frames.

```rust
// In ThingActor
fn broadcast(&self, update: &[u8], exclude: Option<ClientId>) {
    self.subscribers
        .iter()
        .filter(|s| Some(s.client_id) != exclude)
        .for_each(|s| { let _ = s.sender.send(frame); });
}
```

If a send fails (client disconnected, receiver dropped), the subscriber is stale. Cleanup happens on the next Leave message or on a periodic sweep. Failed sends don't propagate errors — the actor doesn't care if a specific client is gone.

**Why unbounded:** The failure mode (slow client causes memory growth) requires a zombie connection. The fix when it matters is bounded channels with `RejoinSuggested` on overflow — the client needs a full resync anyway because it missed updates. This is not a design-time concern.

#### Non-CRDT Side Channel

Relationship changes, suggestion status updates, and other notifications that don't go through the loro CRDT protocol need a side channel on the same websocket. These are NOT CRDT rooms — they're server-authoritative push notifications.

**Deferred:** The exact framing for the side channel (custom message type in the loro protocol envelope, a separate binary prefix, JSON messages interleaved with binary CRDT frames) is a protocol-level design decision that depends on how the frontend parses incoming frames. The actor topology doesn't depend on this choice.

---

### AgentConversation as a CRDT Room

AgentConversation implements `CrdtRoom` because the conversation is a LoroDoc. This gives several properties for free:

1. **LLM token streaming is CRDT sync.** As the LLM generates tokens, the AgentConversation appends them to the conversation LoroDoc. The CRDT sync pushes updates to the connected client in real-time. The client uses the same rendering pipeline for "agent is typing" as for "another human is editing."
2. **Thinking tokens are a different block type** in the LoroDoc. The frontend can render them collapsed or expanded without special streaming logic.
3. **Conversation history is a document.** It persists, it's restorable, it supports hammock time.
4. **Historical suggestions are preserved as blocks in the conversation doc.** If a suggestion was accepted on the Thing page, the conversation still shows what was proposed, as immutable history.

**Human messages are POSTed, not CRDT-appended.** The human message triggers inference — it's a command, not a document edit. The flow:

1. Human POSTs message to AgentConversation via REST
2. AgentConversation appends the human message block to its LoroDoc (server-side)
3. LoroDoc update syncs to the client via CRDT (client sees their own message, confirming receipt)
4. AgentConversation builds the LLM prompt and starts inference
5. Tokens stream back, appended to the LoroDoc, synced to client in real-time

POST makes the intent unambiguous: "this is a new message, start inference." A CRDT append from the client would force the server to distinguish "new message that triggers inference" from "client catching up on sync" from "user editing a previous message" — the CRDT update carries no intent signal.

#### Conversation-Scoped Serialization

When the compiler serializes a Thing page for an AgentConversation, it includes only suggestions owned by that conversation. Other conversations' suggestions are invisible. The agent sees a clean page with only its own pending work.

**Why this matters for deconfliction:** If agent A and agent B independently target the same content, agent B's compiler doesn't see agent A's suggestion marks. It serializes the original content, the agent reasons about it, and produces a suggest_replace. The ThingActor applies the suggestion mark — now both suggestions exist as overlapping marks on the same blocks. The GM sees both and can accept either one independently.

This means agents don't need to reason about each other's proposals. They don't need deconfliction logic. They each operate against their own scoped view of the page. The deconfliction surface is the editor UI, where the GM reviews competing suggestions with full context.

---

### Suggestion Model

Suggestions are modeled as **marks on block ranges**, following the same architectural pattern as TipTap's comment threads. The key insight: a suggestion is a special type of comment that proposes replacement content for the marked blocks, rather than a discussion about them.

#### Block-Level Addressing

Every block in a LoroDoc has a UUID (branded as `BlockId`). Suggestions target a contiguous list of block IDs. The original content stays in the document tree — the suggestion is an annotation layered on top, not a structural replacement.

```rust
struct Suggestion {
    id: SuggestionId,
    target_blocks: Vec<BlockId>,
    proposed_content: Vec<Block>,
    conversation_id: ConversationId,
    author_user_id: UserId,
    created_at: i64,
    model: String,
}
```

#### Why Marks, Not Structural Replacement

The earlier design (pulling target blocks out of the document flow and wrapping them in a SuggestionBlock node) had a fatal flaw: it changed the document tree when a suggestion was created. This meant a second suggestion targeting overlapping blocks would fail — the first suggestion had restructured the tree, so the second couldn't find its target content. Every suggestion after the first operated against a different document than the original.

Marks don't modify the document tree. The content stays where it is. Multiple suggestions can mark overlapping block ranges without interfering. The blocks are stable anchors. The suggestions are metadata associated with those anchors.

#### Blocking Semantics

Blocks that have pending suggestions are **read-only to human editors** in the editor UI. The GM can accept the suggestion (replacing the marked content with the proposed content), reject it (removing the suggestion, leaving the original content editable), or edit the proposed replacement content — but not edit the original text underneath while a suggestion is pending.

**Why blocking eliminates staleness:** If the original text under a suggestion can't be changed by human editing, then the suggestion's target content is always valid. There is no drift, no staleness detection, no render-time comparison of original vs. current text. The only way the content under a suggestion changes is when a _different_ overlapping suggestion is accepted — which is a deliberate GM action, and the remaining suggestions' target blocks now reference different content. The editor can detect this trivially (the accepted suggestion removed/replaced the blocks the other suggestion was targeting) and visually flag the remaining suggestions.

**Escape hatch:** If the GM wants to edit the blocked text directly, they reject the suggestion. One action, clear intent. If multiple suggestions overlap, rejecting one doesn't affect the others — each suggestion independently references its block list.

#### Single-Suggestion Inline Diff vs. Multiple-Suggestion UI

When only one suggestion exists on a block range, the editor renders it as an inline diff — strikethrough for original, highlight for proposed, accept/reject controls on the block. This is the common case and should feel like tracked changes in a word processor.

When multiple suggestions overlap on the same blocks, the editor shifts to a UI that acknowledges competing proposals. The exact visual design (stacked diffs, tabs, sidebar) is a frontend concern. The mechanics are identical — each suggestion independently references blocks and carries proposed content.

#### Suggestion Lifecycle

1. **Created:** The compiler processes a `suggest_replace` tool call, identifies the target block IDs, and sends the compiled suggestion to the ThingActor. The ThingActor adds the suggestion mark and metadata to the LoroDoc. CRDT sync broadcasts the update to connected editors.
2. **Pending:** The suggestion is visible in the editor. Target blocks are read-only. The GM can review in context.
3. **Accepted:** The GM accepts. The ThingActor replaces the target blocks with the proposed content (new blocks get fresh UUIDs). The suggestion mark is removed. The outcome is recorded in the `suggestion_outcomes` table. Any other suggestions whose target blocks overlapped with the accepted suggestion are now referencing changed/removed blocks — the editor flags them accordingly.
4. **Rejected:** The GM rejects. The suggestion mark is removed. The original blocks become editable. The outcome is recorded in `suggestion_outcomes`. No other suggestions are affected.
5. **Superseded (same conversation only):** When the same AgentConversation produces a new suggestion targeting the same blocks, the new suggestion replaces the old one. The old suggestion is recorded as superseded in `suggestion_outcomes`. Different conversations' suggestions always coexist — they are independent proposals deserving independent review.

#### Suggestion Outcomes Table

```sql
CREATE TABLE suggestion_outcomes (
    suggestion_id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    thing_id TEXT NOT NULL,
    author_user_id TEXT NOT NULL,
    model TEXT NOT NULL,
    outcome TEXT NOT NULL,          -- 'accepted', 'rejected', 'superseded'
    resolved_by TEXT,               -- user who accepted/rejected, or conversation that superseded
    resolved_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
```

This table serves two purposes:

**For users:** When a conversation is reopened, the conversation doc shows historical suggestions ("I proposed X"). The outcomes table decorates these with resolution status ("accepted", "rejected", "still pending"). The conversation doc is self-contained history; the outcomes table is read-time enrichment.

**For evals:** Accept/reject rates per model, per workflow, per Thing type. Time-to-resolution. Supersession rates (high supersession might indicate poor first-draft quality). This is training signal for model selection and prompt tuning.

---

### The Compiler

The serialization compiler (`f()` / `f⁻¹()`) is a stateless service, not an actor. It bridges the LoroDoc world (CRDT operations, block UUIDs, Loro types) and the agent world (markdown, wiki-links, retrieval tiers).

**`f()` — LoroDoc → Agent Markdown:** Takes a `DocumentState` reference (from a ThingActor), a graph context (from the RelationshipGraph actor), a retrieval tier, a role (for gm_only filtering), and a conversation ID (for suggestion scoping). Produces the markdown format defined in the AI Serialization Format document. The conversation ID determines which suggestion marks are rendered as `<prior>/<suggestion>` pairs — only the requesting conversation's suggestions are visible.

**`f⁻¹()` — Agent Tool Call → Compiled Suggestion:** Takes a `suggest_replace` tool call (page name, old content, new content), serializes the target page via `f()` to get the current markdown, string-matches the old content to identify target block IDs, and produces a `CompiledSuggestion` ready for the ThingActor to apply.

**Why the compiler is not on the actor:** The compiler needs the actor's document state AND the relationship graph AND embedding results (Tier 2) AND role context AND conversation scoping. Putting this on the ThingActor would require the actor to hold references to all of these. The compiler is a pure function with multiple inputs. The AgentConversation orchestrates: it asks the ThingActor for DocumentState, asks the RelationshipGraph for context, calls the compiler, and routes the result back to the ThingActor.

**Why the compiler always reads from actors:** In the Hocuspocus architecture, the compiler had two read paths — Y.Doc for active pages, libSQL for inactive pages — because loading a Y.Doc on the Node.js event loop was expensive and could starve other connections. In the Rust actor model, spinning up a ThingActor to serve a Tier 1 index card costs one libSQL read and a few milliseconds of CPU. The actor evicts itself on idle. There is no event loop to starve. One read path, through the actor, always.

---

### Design Rationale: How These Decisions Emerged

The designs in this section were not planned top-down. Each one fell out of a specific problem encountered while reasoning about the one before it. This section traces the causal chain to make the reasoning auditable.

#### CampaignDatabase module: "reads are parallelizable, writes need serialization"

The starting problem was: multiple actors need to read from the campaign database concurrently (especially during restoration, when the RelationshipGraph, TocActor, and initial ThingActors all need data), but writes need to be serialized. An actor model gives serialization for free via the mailbox, but if every actor reads through the same write actor, reads become a bottleneck.

The resolution split reads from writes. Reads go through a `CampaignReader` trait — a domain-typed algebra backed by a pool of read-only libSQL connections (WAL mode allows concurrent readers). Writes go through a `DatabaseActor` that owns the single read-write connection.

This also established the persistence module boundary: all SQL lives in one place. Actors never see a connection, a query, or a row. They see domain-typed snapshots going in and out.

**Why a write actor instead of `Arc<Mutex<Connection>>`:** Not for correctness — libSQL in WAL mode with `busy_timeout` already serializes writers at the database level. The actor buys two things: the CampaignSupervisor never blocks on IO during a debounce writeback, and shutdown has a natural drain point (stop accepting new commands, flush pending writes, then release).

#### CampaignStore algebra: "local vs. hosted is not the database's concern"

While designing the CampaignDatabase module, a temptation arose to put the storage lifecycle (downloading from object storage, periodic writeback, final upload) inside the DatabaseActor. This was rejected because it conflates two responsibilities with different triggers, different error handling, and different shutdown ordering.

Per-write persistence (actor snapshots to libSQL) is identical in both local and hosted topologies. The DatabaseActor doesn't know or care where its file came from. The storage lifecycle (where the file comes from, where it goes) is a CampaignSupervisor concern.

The `CampaignStore` trait encapsulates this: local impl is mostly no-ops, hosted impl downloads/uploads from object storage. The CampaignSupervisor owns it. The DatabaseActor never sees it.

**What it gave us for free:** The `CampaignDatabase::checkout()` and `release()` methods compose the two concerns cleanly — checkout calls `CampaignStore::checkout()` to get a file path, then opens connections and spawns the write actor. `release()` drains the write actor, then calls `CampaignStore::release()`. The module owns its full lifecycle with two clean entry/exit points.

#### Non-blocking startup: "heartbeats must survive checkout"

The CampaignDatabase module's `checkout()` is an async function that may take 2-3 seconds (object storage download, connection setup). kameo actors process one message at a time — an `.await` inside a handler yields the thread but not the mailbox. If the CampaignSupervisor calls `checkout()` inside a message handler, heartbeats queue up and the platform thinks the server is dead.

This forced the supervisor into a state machine. The checkout is spawned as a background `tokio::spawn` task, the handler returns immediately, and a `CheckoutComplete` message arrives when the database is ready. A `CheckoutTimedOut` races against completion — loser is always a no-op.

The `Starting` / `Restoring` / `Ready` / `Draining` state machine fell out of this naturally. And because heartbeats always respond with the current phase, the design gives us descriptive loading for free: the platform forwards the phase to connected clients, who see "Downloading campaign data..." then "Restoring entities..." then the editor loads.

**Why interrupt-driven instead of polling:** An alternative (the "monadic" approach) was considered: hold the `JoinHandle` in state, send yourself a `PollLifecycle` message, check `is_finished()` each spin. This is elegant conceptually but adds latency (one message-processing cycle of delay on completion) and requires `std::mem::replace` gymnastics in Rust to move the handle out of the enum. The interrupt pattern (completion message + timeout message) has zero wasted work on the happy path and the timeout falls out of the same mechanism rather than needing a separate check.

#### CampaignVocabulary: "autocomplete needs freshness the database can't provide"

The original prompt was editor autocomplete for `@mentions` — when a GM types `@Jorg`, suggest "Jorgath the Beneficent." The naive implementation (query the database: `SELECT id, name FROM things WHERE name LIKE ?`) has a freshness gap: the ThingActor writes to the database through the DatabaseActor's debounce timer. A Thing created 2 seconds ago might not be in the DB yet. "I just created Jorgath, why can't I mention him?"

This motivated an in-memory actor that holds the entity list and receives domain events (`ThingCreated`, `ThingRenamed`, `ThingDeleted`) directly from the CampaignSupervisor. The actor is always immediately fresh — no polling, no DB reads on the hot path.

**Why Tantivy was deferred:** At ~500 entities, a linear scan with substring matching is sub-microsecond. The search engine question becomes relevant for fuzzy matching (STT correction needs "Yorgath" to find "Jorgath"), but the matching strategy is an implementation detail behind the query interface. Start with Levenshtein distance. Reach for Tantivy or phonetic indexing if and when matching quality becomes a bottleneck.

**Why two separate concerns, not one trait:** The original sketch had a single `TypedInputSuggestions` trait with both `suggest_thing()` and `suggest_relationship()`. These were split because they have different data sources: Thing mentions draw from the entity list (new CampaignVocabulary actor), relationship suggestions draw from distinct edge labels in the graph (existing RelationshipGraph actor, via its `Queryable` implementation). No new actor needed for the second concern.

**Why the vocabulary is Notifiable:** Fell out of tracing the rename flow. When the GM renames "Korgath" to "Kurgath," the vocabulary actor updates its entry, but connected clients also need to know — their local autocomplete cache is stale. The notification is independent of any document update. No CRDT room carries this information. The vocabulary actor pushes `VocabularyNotification::ThingRenamed` to subscribers over the websocket side channel.

#### Mention model: "the relational layer shouldn't store what it can derive"

Tracing the rename flow surfaced three options for mention storage:

- **Option A (ID only in LoroDoc):** No propagation on rename, but every renderer needs a vocabulary lookup. The document is not self-describing. The serialization compiler would need to resolve every mention.
- **Option B (ID + label in LoroDoc, ID + label in relational):** Self-describing documents, but rename requires propagation to every document that mentions the entity — including inactive Things sitting in the database.
- **Option C (ID + label in LoroDoc, ID only in relational):** Self-describing live documents, no propagation to inactive Things. The label is a rendering cache in the CRDT. The relational layer stores only the foreign key. On restoration, `restore()` resolves ThingIds to current names using the CampaignVocabulary.

Option C was chosen because it treats each storage layer according to its strengths. The LoroDoc carries the label for rendering convenience. The relational data carries the ID for structural correctness. Rename propagation only touches active ThingActors (the supervisor sends `MentionRenamed`, each actor updates its live LoroDoc). Inactive Things get the correct name for free when they're next restored.

**What it gave us for free:** Graceful recovery degradation. A hard restart from a hot LoroDoc snapshot might have stale mention labels (a rename happened after the last snapshot). If the vocabulary actor is up, a reconciliation pass can fix them. If it isn't, the page renders with slightly wrong names but structurally correct links. No recovery path requires special ceremony. No recovery path corrupts data. The label is always "correct when convenient, harmlessly stale otherwise."

#### Persistence error handling: "silent drops become silent data loss"

The original `persist()` method discarded send errors with `let _ =`. If the DatabaseActor dies, every snapshot is silently lost. Recoverable in the short term (LoroDoc is in memory, next tick retries), but if the actor evicts before the DatabaseActor recovers, data is gone.

The fix has four parts: log the error, notify connected clients via `PersistenceDegraded`, block eviction while dirty and degraded, and escalate to the platform with a deadline force-kill.

**Why ThingActor gained Notifiable:** Persistence health is server-authoritative. It can't go through CrdtRoom (wrong protocol). Notifiable pushes it over the websocket side channel.

**The escalation boundary:** Campaigns report facts, the platform notifies users. Campaign servers don't know about email infrastructure. This is a product-level decision, not just a technical one.

#### Trait system: "interface vs. pattern, not just external vs. internal"

The original trait system mixed CrdtRoom (dispatched by websocket handlers) and Persistent (consumed only by its implementor) without distinguishing them. The distinction emerged from asking "who calls this?"

This nearly led to eliminating Persistent and Evictable entirely. But Persistent was rescued by the `Into<PersistenceCommand>` bound, which connects snapshot types to the DatabaseActor's command vocabulary at compile time. Without the bound, the connection between "this actor produces ThingSnapshots" and "the DatabaseActor handles ThingSnapshots" is enforced only by convention. With the bound, the compiler checks it.

Evictable was rescued by the coupling between eviction and persistence health -- the degraded check is a correctness requirement the trait makes explicit.

The `PersistenceCommand` enum provides a third layer: every variant must be constructed (`dead_code`) and handled (exhaustive match). Together, the types prove the persistence plumbing is wired up end to end. Not that it's correct -- the SQL might be wrong, the snapshot might be incomplete. But the plumbing from actor to database is verified at compile time, and failures at runtime are visible rather than silent.

#### Restoration as free functions: "construction is not the actor's concern"

`Persistent` originally included `restore()`. Removed because each actor needs different inputs. ThingActor needs a ThingId + reader + writer. CampaignVocabulary needs a reader only. UserSession needs nothing. An associated `RestoreCtx` type would add machinery without enabling polymorphic restoration code -- nobody writes generic restoration functions.

Free functions in `persistence/restore.rs` are honest about each actor's requirements. The function signature documents exactly what's needed. The actor knows how to snapshot and persist. How it was *created* is the persistence module's concern.

---

## Consequences

### What this architecture gives us

- **One state representation per actor.** No conditional logic around "do I have a doc or not." Every code path is exercised in every scenario. The LoroDoc is always there.
- **Independent actor lifecycles.** Loading a document in one actor has zero impact on any other. No shared event loop, no memory pressure propagation, no "don't load Y.Docs for read-only access" workarounds.
- **Natural deconfliction through marks.** Multiple suggestions coexist as overlapping marks on stable blocks. No structural document modification on suggestion creation. No string-match failures from earlier suggestions changing the tree. The deconfliction surface is the editor UI, not the backend.
- **Conversation-scoped agent views.** Each agent sees only its own suggestions. Agents don't reason about each other's proposals. The system doesn't need cross-conversation deconfliction logic.
- **Blocking eliminates staleness.** Read-only blocks under pending suggestions mean the original content never drifts. No staleness detection, no render-time comparison, no stale suggestion states.
- **Provenance tracking for evals.** Every suggestion carries a conversation ID, user ID, model identifier, and timestamp. The outcomes table records resolution. This is the training signal for model quality measurement.
- **Hot-path routing bypasses the supervisor.** DocUpdate messages (99% of traffic during editing) go directly from the websocket read task to the ThingActor via the local routing table. The supervisor handles only JoinRequest and lifecycle events.
- **Compile-time persistence wiring.** The `Into<PersistenceCommand>` bound, `dead_code` lint, and exhaustive match together prove the persistence plumbing is connected end to end. Adding a new persistent actor without wiring it to the DatabaseActor is a compile error.
- **Visible persistence failures.** Write path degradation is logged, notified to clients, and blocks eviction. Data loss scenarios become stuck-actor scenarios that the supervisor can escalate. Silent data loss is architecturally prevented.
- **Descriptive loading.** The supervisor state machine reports its current phase in heartbeats. The platform forwards this to clients, turning an opaque wait into "Downloading campaign data..." then "Restoring entities..." then the editor loads.
- **Mention labels as rendering caches.** The relational layer stores only ThingIds for mentions. Rename propagation only touches active actors. Inactive Things get correct names for free on restore. Recovery degrades gracefully.

### What this architecture costs us

- **Actor-per-Thing memory.** Every active Thing has a LoroDoc in memory. At ~100KB per doc and campaign scale of ~500 entities (of which maybe 30 are active at once), this is ~3MB — negligible. But it means we rely on eviction working correctly. A bug in idle detection could keep hundreds of actors alive unnecessarily.
- **Reconstruction on every actor startup.** There is no fast "just load the relational data" path. Every ThingActor startup rebuilds a LoroDoc from relational rows. This is a few milliseconds per actor, acceptable now, but would need revisiting if LoroDoc reconstruction ever becomes expensive (very large documents, complex schema).
- **Compiler fan-out for context building.** An AI context-building pass may need to read 20+ Things at Tier 1. Each read spins up a ThingActor (if not already active), sends a query, and waits for a response. This is 20+ sequential or parallel actor interactions. Fast individually, but the fan-out pattern needs to be implemented carefully to avoid waterfall latency.
- **RoomHandle enum boilerplate.** Adding a new room-capable actor type requires updating the RoomHandle enum and adding match arms. This is a small tax on extensibility in exchange for type safety — the compiler catches missing cases.
- **Two suggestion mechanisms remain.** Document-level suggestions (marks on blocks in the LoroDoc) and graph-level suggestions (propose_relationship through the suggestion queue) use different storage, different review UIs, and different acceptance flows. This is inherited from the Hocuspocus ADR and remains a cost.
- **Blocking may frustrate GMs.** Read-only blocks under pending suggestions mean the GM must accept or reject before editing that text. For a GM who wants to ignore AI suggestions and just write, this is friction. The escape hatch (reject to unblock) is one action, but if the AI produces many suggestions across many blocks, the GM may feel they're playing whack-a-mole with reject buttons rather than writing.
- **Stuck actors during shutdown.** If the DatabaseActor dies, persistent actors refuse to evict. The supervisor must track stuck actors, wait a deadline, then force-kill them. This is the correct behavior (visible and bounded beats silent data loss), but it adds complexity to the shutdown path and requires the platform to handle escalation notifications.
- **Interrupt-driven startup complexity.** The non-blocking startup pattern (spawn task, receive completion message, race timeout) is more complex than a synchronous `checkout().await`. Each lifecycle transition is a separate message handler. The tradeoff is necessary (heartbeats must survive checkout) but it means the supervisor has more message types and more state transitions to get right.

---

## Open Questions

- **Loro's mark/annotation primitives.** The suggestion model depends on marks over block ranges. Loro's native support for this (vs. building range tracking on top of LoroText/LoroTree) needs investigation in the Loro/TipTap spike.
- **Non-CRDT side channel wire framing.** The architecture now has concrete consumers: persistence health notifications (ThingNotification) and vocabulary change notifications (VocabularyNotification) both flow through the Notifiable trait, sharing the same subscriber list as CrdtRoom and multiplexed by the write_task via message envelope discrimination. The remaining open question is the exact wire format: custom message type in the loro protocol envelope, a separate binary prefix, or JSON messages interleaved with binary CRDT frames. This depends on how the frontend parses incoming frames and needs resolution during the Loro/TipTap spike.
- **Conversation LoroDoc schema.** The exact block types for user messages, assistant messages, thinking tokens, and historical suggestion records in the conversation document. Needs design alongside the TipTap extension for the chat UI.
- **Campaign graph change notification payloads.** The delivery mechanism is decided: RelationshipGraph implements `Notifiable`, sharing the same subscriber list and write_task multiplexing as other notification consumers. The remaining question is the notification payload shape -- what information does the client need when an edge is added, removed, or when a `propose_relationship` is accepted? This determines whether the frontend can update its local graph representation incrementally or needs to re-fetch.
- **Eviction under active suggestions.** If a ThingActor has pending suggestions and evicts on idle, the suggestions must survive in the database. On restoration, the actor must reconstruct both the document content and the suggestion marks. This is a `restore()` implementation concern, not an architectural one, but it's a correctness requirement that needs explicit testing.
