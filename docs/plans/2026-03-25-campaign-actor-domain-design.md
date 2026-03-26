# ADR: Campaign Actor Domain Design

**Status:** Draft
**Date:** 2026-03-25
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

- **Solo developer.** Operational complexity matters, but the right abstraction is worth the upfront cost if it prevents larger problems later. Don't optimize for "easy to build first time" at the expense of "easy to reason about in six months."
- **Campaign-as-file isolation.** Each campaign is a self-contained libSQL database file. All actors for a campaign operate against the same file. Cross-campaign interaction is architecturally impossible.
- **"AI proposes, GM disposes."** The AI never modifies the campaign graph directly. All AI output is provisional until explicitly accepted.
- **EU/EEA infrastructure.** All compute and data stays in EU/EEA. LLM inference runs on Nebius (Finnish infrastructure). Claude never sees user data.

---

## Decision

### Actor Topology

A checked-out campaign has the following actor tree:

```
CampaignSupervisor (one per checked-out campaign)
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

### ThingActor Internal State

```rust
struct ThingActor {
    id: ThingId,
    doc: LoroDoc,
    subscribers: Vec<Subscriber>,
    dirty: bool,
    last_activity: Instant,
}
```

**The LoroDoc is always reconstructed on actor startup.** There is no "cold" state where the actor holds only relational data. `restore()` reads from libSQL and builds the full LoroDoc via the equivalent of `toYdoc()`. The doc is live from the moment the actor exists.

**Why no two-phase state (cold relational / hot CRDT):** The Hocuspocus architecture had two read paths (Y.Doc for active pages, libSQL for inactive pages) because loading a Y.Doc on the Node.js event loop consumed shared memory and blocked the single thread. In Rust, each actor is an independent async task. Reconstructing a LoroDoc in one actor has zero impact on any other actor. The reconstruction cost is a few milliseconds of CPU to walk relational rows and build a document tree. At campaign scale, even a context-building pass that spins up 30 ThingActors costs ~30ms of CPU and ~3MB of memory. The actors evict themselves after idle timeout.

One state representation means one read path, one write path, and no conditional logic around "do I have a doc or not." The compiler always reads from a LoroDoc. The CRDT room is always joinable. The debounce timer always has a doc to snapshot. Every code path is exercised in every scenario.

**Debounce is per-actor.** Each ThingActor manages its own persistence timer. When the timer fires, the actor snapshots its LoroDoc to relational data and writes to the campaign database. 30 active Things means 30 independent timers — they're atomic, they don't interact, and if one fires late, nothing else cares. A centralized "sweep dirty actors" tick would couple actors that have no reason to be coupled.

---

### Trait System

The trait system captures the behavioral contracts that actors implement. Traits are composed — a ThingActor implements a different set than a RelationshipGraph. The traits exist at the _design_ level (informing what messages each actor handles) rather than as Rust `dyn Trait` objects, because kameo's `ActorRef<A>` is generic over the concrete actor type.

#### Lifecycle Traits

```rust
/// Reconstructable from the campaign database. Every persistent actor
/// implements this. The LoroDoc (for CRDT actors) or in-memory graph
/// (for RelationshipGraph) is always fully built on restore.
trait Persistent {
    type Snapshot;
    async fn restore(db: &CampaignDb, id: &EntityId) -> Result<Self>;
    fn snapshot(&self) -> Self::Snapshot;
    fn is_dirty(&self) -> bool;
}
```

**Why `Persistent` is one trait, not split into "debounced" vs. "eager":** AgentConversation could justify eager persistence (write-through on every message, because losing conversation history is unacceptable). But the simplest implementation is a short debounce (e.g., 1 second) that's functionally indistinguishable from eager writes while batching rapid-fire message sequences. If message loss during the debounce window becomes a real problem — which requires a server crash during the 1-second window after the user sends a message but before the timer fires — the fix is straightforward (flush on each append). The trait doesn't need to encode this distinction.

```rust
/// Self-terminates after a period of inactivity. The actor sets its
/// own timer. The supervisor is notified on exit via kameo's
/// supervision protocol.
trait Evictable {
    fn idle_timeout(&self) -> Duration;
    fn last_activity(&self) -> Instant;
    async fn prepare_eviction(&mut self) -> Option<Self::Snapshot>
    where Self: Persistent;
}
```

#### CRDT Sync Traits

```rust
/// Anything that participates in the loro-dev/protocol as a "room."
/// Each implementor IS a room that clients can join, sync with,
/// and leave.
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
```

**CrdtRoom is implemented by ThingActor, TocActor, and AgentConversation** (for LLM response streaming — see below). It is NOT implemented by RelationshipGraph, which is server-authoritative.

```rust
/// Broadcasts non-CRDT notifications to connected clients. Used by
/// RelationshipGraph (edge changes), and potentially TocActor
/// (structural changes that affect navigation).
trait Notifiable {
    type Notification: Serialize;
    fn subscribe(&mut self, client: ClientId);
    fn unsubscribe(&mut self, client: ClientId);
}
```

#### Query and Mutation Traits

```rust
/// REST-readable. The handler asks the actor for a response rather
/// than hitting the DB, because the actor holds state that may be
/// ahead of the last writeback.
trait Queryable {
    type Query;
    type Response: Serialize;
    fn query(&self, q: &Self::Query) -> Self::Response;
}

/// Accepts mutations through the REST API (as opposed to through
/// CRDT sync). Relationships and ToC reordering go here.
trait Mutable {
    type Command;
    type Event: Serialize;
    fn apply_command(&mut self, cmd: Self::Command)
        -> Result<Self::Event, DomainError>;
}
```

#### Suggestion Trait

```rust
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
```

**Why `SuggestionTarget` is a separate trait from `CrdtRoom`:** Suggestions are not raw CRDT operations. They are semantically meaningful proposals that go through the compiler, carry provenance, and have a lifecycle (pending → accepted/rejected). The trait makes it explicit that accepting a suggestion is a domain operation (replace target blocks, record outcome, clean up the suggestion mark), not just "apply some bytes to the doc."

#### Serialization Traits

```rust
/// The raw material the compiler needs to serialize a Thing into
/// the agent-facing markdown format. The ThingActor implements this.
/// The compiler consumes it.
trait DocumentState {
    fn content(&self) -> &PageContent;
    fn status_tree(&self) -> &StatusTree;
    fn identity(&self) -> &ThingIdentity;
}
```

**Why serialization is NOT a trait on the actor:** The serialization compiler (`f()`) needs the actor's document state AND the campaign relationship graph AND embedding results (for Tier 2 RAG) AND the user's role (for gm_only filtering). Putting serialization on the actor would require the actor to hold references to all of those services. Instead, the compiler is a stateless service that takes `&dyn DocumentState` plus context and produces markdown. The actor exposes its state. The compiler does the work. Clean separation of concerns.

#### Trait Composition by Actor

| Actor              | Persistent | Evictable | CrdtRoom | Notifiable | Queryable | Mutable | SuggestionTarget | DocumentState |
| ------------------ | ---------- | --------- | -------- | ---------- | --------- | ------- | ---------------- | ------------- |
| ThingActor         | ✓          | ✓         | ✓        |            | ✓         |         | ✓                | ✓             |
| TocActor           | ✓          | ✓         | ✓        | ✓          | ✓         |         |                  |               |
| RelationshipGraph  | ✓          |           |          | ✓          | ✓         | ✓       |                  |               |
| AgentConversation  | ✓          | ✓         | ✓        |            |           |         |                  |               |
| UserSession        |            | ✓         |          |            |           |         |                  |               |
| CampaignSupervisor |            |           |          |            |           |         |                  |               |

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

## Consequences

### What this architecture gives us

- **One state representation per actor.** No conditional logic around "do I have a doc or not." Every code path is exercised in every scenario. The LoroDoc is always there.
- **Independent actor lifecycles.** Loading a document in one actor has zero impact on any other. No shared event loop, no memory pressure propagation, no "don't load Y.Docs for read-only access" workarounds.
- **Natural deconfliction through marks.** Multiple suggestions coexist as overlapping marks on stable blocks. No structural document modification on suggestion creation. No string-match failures from earlier suggestions changing the tree. The deconfliction surface is the editor UI, not the backend.
- **Conversation-scoped agent views.** Each agent sees only its own suggestions. Agents don't reason about each other's proposals. The system doesn't need cross-conversation deconfliction logic.
- **Blocking eliminates staleness.** Read-only blocks under pending suggestions mean the original content never drifts. No staleness detection, no render-time comparison, no stale suggestion states.
- **Provenance tracking for evals.** Every suggestion carries a conversation ID, user ID, model identifier, and timestamp. The outcomes table records resolution. This is the training signal for model quality measurement.
- **Hot-path routing bypasses the supervisor.** DocUpdate messages (99% of traffic during editing) go directly from the websocket read task to the ThingActor via the local routing table. The supervisor handles only JoinRequest and lifecycle events.

### What this architecture costs us

- **Actor-per-Thing memory.** Every active Thing has a LoroDoc in memory. At ~100KB per doc and campaign scale of ~500 entities (of which maybe 30 are active at once), this is ~3MB — negligible. But it means we rely on eviction working correctly. A bug in idle detection could keep hundreds of actors alive unnecessarily.
- **Reconstruction on every actor startup.** There is no fast "just load the relational data" path. Every ThingActor startup rebuilds a LoroDoc from relational rows. This is a few milliseconds per actor, acceptable now, but would need revisiting if LoroDoc reconstruction ever becomes expensive (very large documents, complex schema).
- **Compiler fan-out for context building.** An AI context-building pass may need to read 20+ Things at Tier 1. Each read spins up a ThingActor (if not already active), sends a query, and waits for a response. This is 20+ sequential or parallel actor interactions. Fast individually, but the fan-out pattern needs to be implemented carefully to avoid waterfall latency.
- **RoomHandle enum boilerplate.** Adding a new room-capable actor type requires updating the RoomHandle enum and adding match arms. This is a small tax on extensibility in exchange for type safety — the compiler catches missing cases.
- **Two suggestion mechanisms remain.** Document-level suggestions (marks on blocks in the LoroDoc) and graph-level suggestions (propose_relationship through the suggestion queue) use different storage, different review UIs, and different acceptance flows. This is inherited from the Hocuspocus ADR and remains a cost.
- **Blocking may frustrate GMs.** Read-only blocks under pending suggestions mean the GM must accept or reject before editing that text. For a GM who wants to ignore AI suggestions and just write, this is friction. The escape hatch (reject to unblock) is one action, but if the AI produces many suggestions across many blocks, the GM may feel they're playing whack-a-mole with reject buttons rather than writing.

---

## Open Questions

- **Loro's mark/annotation primitives.** The suggestion model depends on marks over block ranges. Loro's native support for this (vs. building range tracking on top of LoroText/LoroTree) needs investigation in the Loro/TipTap spike.
- **Non-CRDT side channel framing.** How relationship change notifications and other server-authoritative push messages share the websocket with loro-protocol binary frames. Deferred to implementation.
- **Conversation LoroDoc schema.** The exact block types for user messages, assistant messages, thinking tokens, and historical suggestion records in the conversation document. Needs design alongside the TipTap extension for the chat UI.
- **Campaign graph change notifications.** When the RelationshipGraph actor processes an accepted `propose_relationship`, how do connected clients learn about it? The graph actor implements `Notifiable`, but the delivery mechanism (which websocket channel, what message format) is unspecified.
- **Eviction under active suggestions.** If a ThingActor has pending suggestions and evicts on idle, the suggestions must survive in the database. On restoration, the actor must reconstruct both the document content and the suggestion marks. This is a `restore()` implementation concern, not an architectural one, but it's a correctness requirement that needs explicit testing.
