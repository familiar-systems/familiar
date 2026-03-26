# ADR: Campaign Collaboration Architecture

**Status:** Draft
**Date:** 2026-03-25
**Supersedes:** [Document-Centric Campaign Architecture (Hocuspocus ADR)](../archive/plans/2026-03-14-hocuspocus-architecture.md) — validated its hypotheses, then replaced the implementation technology (Yjs/Hocuspocus/Node.js → Loro/kameo/Rust). The campaign model, scaling model, and operational invariants carry forward. The collaboration layer, persistence hooks, and AI interaction model change.
**Related decisions:** [Campaign Actor Domain Design](./2026-03-25-campaign-actor-domain-design.md), [AI Serialization Format v2](./2026-03-25-ai-serialization-format-v2.md), [Suggestion Marks Spike](./2026-03-25-loro-tiptap-spike.md), [Project structure](./2026-03-26-project-structure-design.md), [AI workflow unification](./2026-02-14-ai-workflow-unification-design.md), [Templates as prototype pages](./2026-02-20-templates-as-prototype-pages.md)

---

## Context

Loreweaver is a SaaS platform for tabletop RPG groups. Multi-hour session audio recordings go in; transcripts, speaker-attributed journals, entity graphs, and a persistent campaign wiki come out. The product is built around a rich text editor (TipTap on ProseMirror) where GMs and players view and edit campaign content, and where AI agents propose changes based on session processing and interactive conversations.

Three actor types interact with page content:

1. **Human editors** (GM and players) via a browser-based TipTap editor connected over WebSocket
2. **The interactive AI agent** during Planning & Refinement and Q&A conversations
3. **The batch AI pipeline** (SessionIngest) processing audio recordings into journal drafts and entity proposals

The superseded Hocuspocus ADR validated eight hypotheses about how page state flows between these actors, how it's persisted, and how conflicts are resolved. All hypotheses held. What changed is the implementation technology: the Hocuspocus experiment revealed that Node.js event loop constraints — single-threaded execution, shared memory pressure across all documents, Y.Doc lifecycle coupled to Hocuspocus hooks — forced architectural workarounds (two read paths, two write paths, memory pressure management) that don't exist in a Rust actor model. Moving to Rust/kameo/Loro eliminates these constraints and simplifies the design while preserving every principle the Hocuspocus ADR established.

### What carries forward unchanged

- **Campaign-as-file isolation.** Each campaign is a self-contained libSQL database file.
- **Object storage as source of truth, local disk as cache.** Campaign checkout/checkin lifecycle.
- **Routing table for campaign → server assignment.** Single-server ownership, lease-based.
- **Blob-free files at rest.** CRDT state is transient. Relational data is the data at rest.
- **Lossless reconstruction.** The cold-checkout path must produce rendered output identical to the original.
- **"AI proposes, GM disposes."** All AI output is provisional until explicitly accepted.
- **"Tolerant of neglect."** The system stays useful when the GM doesn't review promptly.
- **No Redis.** Campaign-pinning eliminates cross-instance sync.
- **Nothing happens without checkout.** The libSQL file must be on local disk.
- **SPA architecture.** Loads from CDN, handles async checkout gracefully.
- **The scaling model.** Single server now, multiple servers later. Same code, different infrastructure.

### What changes

| Concern | Hocuspocus ADR | This ADR |
|---------|---------------|----------|
| CRDT library | Yjs (Y.Doc) | Loro (LoroDoc) |
| Sync protocol | Yjs sync protocol via Hocuspocus | loro-dev/protocol (room-based multiplexing) |
| Server runtime | Node.js (single-threaded event loop) | Rust + kameo (actor-per-document, independent async tasks) |
| Collaboration server | Hocuspocus (lifecycle hooks) | Axum WebSocket + kameo actors |
| ProseMirror binding | y-prosemirror | loro-prosemirror |
| Persistence hooks | `onLoadDocument` / `onStoreDocument` | Actor `restore()` / `snapshot()` via `Persistent` trait |
| AI read paths | Two: Hocuspocus for active pages, libSQL for inactive | One: always through actors. Every actor holds a full LoroDoc. |
| AI write paths | Two: WebSocket for active pages, HTTP/DirectConnection for inactive | One: through the compiler + actor messaging. All suggestions are marks on blocks. |
| Document proposals | Tagged CRDT blocks (`agent_proposal_prior/suggestion`) | Suggestion marks on block UUID ranges |
| AI agent connection model | WebSocket participant speaking Yjs sync protocol | Compiler-mediated: agent calls tools, compiler produces LoroDoc operations |
| Memory management | Manual (don't load Y.Docs for read-only, eviction timeout) | Automatic (actors are independent, eviction is per-actor idle timeout) |

### Constraints

- **Database-per-campaign on libSQL.** Each campaign is an isolated file. No shared multi-tenant database. Turso Database is the identified upgrade path.
- **"AI proposes, GM disposes."** Structural at every write path. The suggestion mark model and the graph-level suggestion queue both enforce this.
- **"Tolerant of neglect."** Suggestions may expire. The system doesn't block on GM review.
- **EU/EEA infrastructure.** All compute and data in EU/EEA. LLM inference on Nebius (Finnish infrastructure).

---

## Decision

### The campaign as the atomic unit

Every campaign is a self-contained libSQL database file. There are no cross-campaign queries, no shared tables, no joins between campaigns. A campaign's database contains block records, entity data, relationships and graph edges, search text and embeddings, suggestion outcomes, agent conversation history, and campaign-specific metadata. During active editing sessions, LoroDoc state is held in actor memory as transient CRDT plumbing.

This isolation was chosen for branch deployment — preview environments are `cp` or `cp -r`, depending on the database layout. GDPR deletion is a happy side effect (delete a campaign = delete a file). The isolation also turns out to be the foundation of the scaling model. Each campaign is an independent shard. No distributed transactions, no consensus protocols.

### Object storage as source of truth, local disk as cache

Object storage (Hetzner Object Storage) is the authoritative location for all campaign database files. Local disk on each application server is a working cache.

**Campaign checkout:** When a user connects to a campaign, the routing table (a lightweight map of campaign ID → server address in the central database instance) is consulted. If the campaign is already checked out on a server, route there. If not, assign it to the least-loaded server. That server downloads the libSQL file from object storage to local disk, opens it, and spawns a CampaignSupervisor actor that owns the database connection and all child actors for that campaign.

**Active session:** All reads and writes run against the local libSQL file at NVMe speed. No network hop in the hot path. ThingActors are spawned on demand as users open pages or the AI agent needs context — each actor reconstructs a full LoroDoc from relational data on startup.

**Periodic writeback:** Each actor manages its own debounce timer. When the timer fires, the actor snapshots its LoroDoc to relational data and writes to the campaign database. A campaign-level writeback flushes the local libSQL file to object storage periodically (~30 seconds) for durability. This bounds the worst-case data loss window.

**Campaign release:** When the last user disconnects and an idle timeout elapses, the CampaignSupervisor evicts all child actors. Each actor snapshots and writes back before terminating. The final campaign database file — containing only relational data, no CRDT blobs — is written to object storage. The campaign is deregistered from the routing table.

**Lease-based ownership:** Each campaign checkout is a lease with a heartbeat. The router will not reassign a campaign until the lease expires. This prevents two servers from having the same campaign open simultaneously.

### The actor topology replaces Hocuspocus

The superseded ADR used Hocuspocus as the collaboration layer — a Node.js WebSocket server with lifecycle hooks (`onLoadDocument`, `onStoreDocument`, `onChange`, `onAuthenticate`). These hooks were the integration points for persistence, validation, and AI interaction.

The new architecture replaces Hocuspocus with kameo actors, each owning a LoroDoc and handling CRDT sync via the loro-dev/protocol:

```
CampaignSupervisor (one per checked-out campaign)
├── ThingActor (per active Thing — wiki pages)
├── TocActor (one per campaign — organizational structure)
├── RelationshipGraph (one per campaign — full entity graph in memory)
├── UserSession (per connected user)
│   ├── AgentConversation (per conversation — P&R, Q&A)
│   └── ...
```

Each ThingActor is the equivalent of "one document in Hocuspocus" — it holds a LoroDoc, syncs with connected clients via the loro-dev/protocol, and persists to the campaign database on a debounce timer. The critical difference: **there is no shared event loop.** Each actor is an independent async task. Loading a document in one actor has zero impact on any other. This eliminates the memory pressure and event loop contention that drove the Hocuspocus ADR's "two read paths" and "two write paths" design.

The full actor topology, trait system, and interaction patterns are defined in the [Campaign Actor Domain Design](./2026-03-25-campaign-actor-domain-design.md).

### LoroDoc lifecycle replaces Y.Doc lifecycle

**The Hocuspocus lifecycle:**

1. `onLoadDocument`: client opens a page → Hocuspocus calls hook → hook reads relational data from libSQL → `toYdoc()` reconstructs a Y.Doc → Y.Doc is held in Hocuspocus memory → clients sync via Yjs protocol
2. `onStoreDocument` (debounced): Y.Doc changed → hook fires → `fromYdoc()` extracts ProseMirror JSON → writes blocks/mentions/statuses to relational tables → Y.Doc blob optionally persisted for crash recovery
3. Eviction: last client disconnects → idle timeout → `onStoreDocument` fires one final time → Y.Doc blob column nulled → clean eviction, blob-free file

**The actor lifecycle (replaces the above):**

1. **Spawn:** Something needs a Thing (user opens page, AI needs context, another actor queries it) → CampaignSupervisor spawns a ThingActor → `restore()` reads relational data from libSQL → constructs a full LoroDoc → actor is live, CRDT room is joinable
2. **Active:** Clients join the actor's CRDT room via the loro-dev/protocol → edits sync bidirectionally → debounce timer fires → actor snapshots LoroDoc to relational data → writes to campaign DB
3. **Eviction:** No subscribers, idle timeout fires → final snapshot and writeback → actor terminates → LoroDoc is dropped (blob-free, relational data is the data at rest)

**What's simpler:** No distinction between "active page" and "inactive page." No `DirectConnection` for HTTP writes. No "don't load Y.Docs for read-only access" optimization. Every actor always holds a full LoroDoc. One read path, one write path, one lifecycle.

**What's preserved:** The fundamental pattern — transient CRDT state in memory during editing, relational data as the sole representation at rest, lossless reconstruction on cold checkout — is identical. The hooks are replaced by actor trait methods. The eviction model is the same. Blob-free files at rest is the same.

### One read path replaces two

**The Hocuspocus design had two read paths because of Node.js constraints:**

For the AI agent, reading through Hocuspocus loaded Y.Docs into memory on the single-threaded event loop. A SessionIngest pass needing context from 20+ Things would load 20+ Y.Docs, consuming shared memory and potentially starving editor connections. The solution was "smart routing": read from Hocuspocus for active pages (getting freshness via CRDT sync), read from libSQL for inactive pages (avoiding memory pressure).

**The actor design has one read path:**

Every read goes through an actor. If the actor exists (Thing is active), the read hits the live LoroDoc. If the actor doesn't exist, the CampaignSupervisor spawns it — one libSQL read, a few milliseconds of reconstruction, and the actor is live with a full LoroDoc. At campaign scale (~500 entities, of which maybe 30 are active and another 20 are transiently spawned for AI context), this is ~5MB of memory across all actors. They evict themselves on idle timeout.

There is no shared event loop to starve. Each actor is an independent async task. The "memory pressure for read-only access" concern from the Hocuspocus ADR does not exist.

### One write path replaces two

**The Hocuspocus design had two write paths:**

- **WebSocket (active pages):** The AI agent connects as a Hocuspocus participant via `HocuspocusProvider`, receives live Y.Doc state, writes suggestion-tagged blocks directly into the Y.Doc. Real-time sync to editors.
- **HTTP (inactive pages):** The AI agent writes via `DirectConnection.transact()`, a server-side shortcut that opens a Y.Doc, applies changes, and closes it without a WebSocket connection. ~2ms per write.

**The actor design has one write path:**

The AI agent never speaks the CRDT protocol. It calls tools (`suggest_replace`, `create_page`, `propose_relationship`). The serialization compiler (`f⁻¹()`) translates tool calls into compiled suggestions. The AgentConversation actor routes compiled suggestions to the appropriate ThingActor. The ThingActor applies suggestion marks to its LoroDoc. CRDT sync broadcasts the change to connected editors.

This is functionally equivalent to the Hocuspocus WebSocket path — suggestions appear in real-time — but the agent doesn't need to be a protocol participant. The compiler is the bridge. Whether the page is "active" (has human editors) or "inactive" (nobody editing) doesn't matter. The ThingActor always exists when someone needs to write to it, and the write path is the same.

### Suggestion model replaces tagged CRDT blocks

**The Hocuspocus design used tagged CRDT blocks:**

The agent's writes inserted `<prior>/<suggestion>` block pairs into the Y.Doc, tagged with `agent_proposal_prior: true` / `agent_proposal_suggestion: true`. The editor rendered these as inline diffs. Accepting removed the prior block and cleared the tag on the suggestion. Rejecting removed the suggestion and cleared the tag on the prior.

**The new design uses suggestion marks on block UUID ranges:**

Every block in a LoroDoc has a UUID (`BlockId`). Suggestions target a contiguous list of block IDs and store proposed replacement content as metadata. The original blocks stay in the document tree, unchanged. Suggestions are annotations layered on top — marks, not structural modifications.

This solves a structural problem the tagged-block approach had: creating a suggestion modified the document tree (pulling original content into a SuggestionBlock node). A second suggestion targeting overlapping content couldn't find its target because the first suggestion had restructured the tree. With marks, the tree is stable. Multiple suggestions coexist on stable blocks.

The full suggestion model — marks, blocking semantics, conversation-scoped visibility, supersession rules, outcomes table — is defined in the [AI Serialization Format v2](./2026-03-25-ai-serialization-format-v2.md).

### AI agent interaction model

**The Hocuspocus design:** The agent is "a collaboration participant identical in kind to a human editor." It connects via WebSocket, speaks the Yjs sync protocol, and writes directly to the Y.Doc. The only difference from a human editor is a permission constraint (suggestion-tagged blocks only).

**The new design:** The agent is a participant in spirit but not in protocol. It doesn't speak the loro CRDT protocol. Instead:

1. The AgentConversation actor asks ThingActors for document state
2. The serialization compiler (`f()`) produces agent-readable markdown at the appropriate retrieval tier, scoped to the conversation's own pending suggestions
3. The agent reasons about the markdown and produces tool calls
4. The compiler (`f⁻¹()`) translates tool calls into compiled suggestions (target block IDs + proposed content)
5. The AgentConversation routes compiled suggestions to ThingActors
6. ThingActors apply suggestion marks and broadcast via CRDT sync

The agent's experience is the same — it reads content, proposes changes, and those changes appear in real-time in the editor. But the implementation replaces "agent joins a WebSocket room and writes Yjs operations" with "agent calls tools, compiler produces Loro operations, actors apply them." This is both simpler (the agent doesn't need CRDT protocol support) and more powerful (the compiler can validate, scope, and mediate suggestions).

The staleness concern from the Hocuspocus ADR ("what if the agent reads stale content?") is handled differently:

- **In the Hocuspocus design:** The WebSocket path gave the agent live sync. The HTTP path used state vector CAS checks. Two mechanisms.
- **In the actor design:** The agent reads from actors, which always hold the latest state. The string-match mechanism in `suggest_replace` detects stale reads — if the content the agent based its reasoning on has changed, the string match fails, and the agent re-reads. One mechanism.

### WebSocket architecture

The Hocuspocus ADR didn't detail WebSocket architecture because Hocuspocus handled it. The new design requires explicit WebSocket management, defined in the [Campaign Actor Domain Design](./2026-03-25-campaign-actor-domain-design.md):

- One WebSocket per campaign per client, upgraded by axum
- Read/write task pair per connection
- loro-dev/protocol messages parsed by the read task
- Local routing table (`HashMap<RoomId, RoomHandle>`) for hot-path dispatch
- CampaignSupervisor only in the path for JoinRequest and disconnect
- DocUpdate (99% of traffic) routes directly to ThingActor via the routing table

The loro-dev/protocol provides room-based multiplexing natively. Multiple rooms (Thing pages, ToC, agent conversation streams) share one WebSocket connection. Each room has a CRDT type discriminator and a room ID.

---

## Scaling Model

### Now: Single Server

One Hetzner VPS (CX22, hel1), one Volume as local NVMe cache, object storage as source of truth. The Rust binary runs under k3s. The routing table has one entry. The campaign checkout/checkin pattern, object storage writeback, routing table, and lease model are all implemented from the start.

All code written in this phase works unchanged in the next phase. The only difference is infrastructure.

**Transition trigger:** Sustained CPU or memory saturation on the single server during peak session hours.

### Next: Multiple Servers

Add servers. Each has its own local disk. The routing table has multiple entries. Ingress routes campaign-bound requests to the correct server. New campaigns are assigned to the least-loaded server. Rebalancing is "copy a small file to object storage (it's already there), update the routing table."

No Redis. No distributed database. No consensus protocol. Just files on disks behind a router.

### Why no Redis

The campaign-pinning routing model eliminates cross-instance sync. All users of a campaign are routed to the same server. All actors for a campaign run on the same server. There is no cross-instance document sync because there is no cross-instance document access. This reasoning is unchanged from the superseded ADR.

### Nothing happens to a campaign without checkout

A campaign must be checked out on a server before any actor — human, AI agent, or batch pipeline — can read from or write to it. This is not a convention; it's structural. The libSQL file must be on local disk before it can be opened.

This eliminates race conditions by design. If a user connects to a campaign while SessionIngest is writing journal drafts and entity proposals, both are on the same server, operating against the same local libSQL file, with actor isolation preventing concurrent write hazards. There is no window where the user sees stale data because both go through actors that hold live LoroDocs. There is no concurrent write hazard because each actor manages its own document. There is no split-brain because the routing table enforces single-server ownership.

---

## Consequences

### What this architecture gives us

- **One state representation per document.** Every ThingActor holds a full LoroDoc. No conditional logic around "do I have a doc or not." No two-phase cold/hot state. Every code path exercised in every scenario.
- **One read path.** Always through actors. No "Hocuspocus for active, libSQL for inactive" branching.
- **One write path for AI.** Tool calls → compiler → actor. No "WebSocket for active, HTTP for inactive" branching.
- **Independent actor lifecycles.** No shared event loop. No memory pressure propagation. Loading a document in one actor has zero impact on any other.
- **Real-time AI collaboration.** Suggestion marks applied to LoroDocs propagate via CRDT sync to connected editors. Same real-time experience as the Hocuspocus design, different mechanism.
- **Non-overlapping suggestions coexist.** Multiple agents can propose changes to different parts of the same page simultaneously without interference.
- **Overlapping suggestions coexist.** Multiple agents can propose changes to the same blocks. Suggestions are marks on stable content, not structural modifications to the document tree.
- **Blocking eliminates staleness.** Blocks under pending suggestions are read-only. No drift, no race conditions, no render-time staleness detection.
- **Conversation-scoped agent views.** Each agent sees only its own suggestions. No cross-conversation deconfliction logic needed.
- **Trivial horizontal scaling.** Unchanged from the superseded ADR. Campaign-per-file isolation means adding servers is "add a box, update the routing table."
- **Blob-free files at rest.** Unchanged. LoroDoc state is transient. Relational data is the sole representation at rest.
- **Offline resilience.** The browser holds a full LoroDoc replica via `loro-prosemirror`. Edits during network interruptions are buffered and merged on reconnection.
- **Provenance tracking for evals.** Every suggestion carries conversation ID, user ID, model identifier, and timestamp. The outcomes table records resolution.

### What this architecture costs us

- **Two proposal mechanisms remain.** Document-level proposals (suggestion marks on blocks) and graph-level proposals (`propose_relationship` through the suggestion queue) use different storage, different review UIs, and different acceptance flows. The Hocuspocus ADR had the same cost.
- **Materialization lag.** The libSQL read model lags behind the LoroDoc by the debounce interval. Unchanged from the superseded ADR.
- **Lossless reconstruction requirement.** The cold-checkout path must produce rendered output identical to the original. Unchanged. Must be tested for every schema change.
- **New technology stack.** Loro is younger than Yjs. The loro-prosemirror binding is younger than y-prosemirror. The loro-dev/protocol is younger than Hocuspocus. Less community knowledge, fewer battle-tested deployments. This is a real risk traded against the architectural simplicity gains.
- **Actor-per-Thing memory.** At ~100KB per LoroDoc and ~30 active actors, this is ~3MB — negligible. But eviction must work correctly. A bug in idle detection could keep hundreds of actors alive.
- **Reconstruction on every actor startup.** No fast "just load relational data" path. Every ThingActor rebuilds a LoroDoc. A few milliseconds per actor now, but would need revisiting if reconstruction ever becomes expensive.
- **Compiler fan-out for context building.** AI context passes may spin up 20+ actors. Fast individually, but the fan-out pattern needs careful implementation to avoid waterfall latency.
- **Blocking may frustrate GMs.** Read-only blocks under pending suggestions mean the GM must accept or reject before editing. If SessionIngest produces many suggestions, the GM encounters many blocked regions. The escape hatch is one action (reject), but volume could create friction.

---

## Key Invariants

These carry forward from the superseded ADR, updated for the new implementation:

- **Object storage is always authoritative.** Local libSQL files are a cache.
- **During active editing, the LoroDoc is authoritative.** Relational data is derived from it via the actor's `snapshot()`. Never write directly to relational tables while an actor holds a live LoroDoc for that Thing.
- **At rest, relational data is the data.** LoroDoc state is dropped on actor eviction. Cold checkout reconstructs LoroDocs from relational data via `restore()`.
- **Lossless reconstruction.** The `snapshot()` → relational → `restore()` round-trip must preserve all rendered content. Tested on every schema change.
- **Nothing happens to a campaign without checkout.** All reads and writes require the libSQL file to be on local disk, checked out from object storage.
- **A campaign has at most one owning server at any time** (enforced by lease).
- **No cross-campaign state or queries.** The campaign file is the complete unit.
- **All document mutations flow through actors.** Humans edit via CRDT sync to the ThingActor. AI suggestions flow through the compiler and are applied by the ThingActor. Never write directly to relational tables while an actor is live.
- **The central database instance is the only centralized stateful system** and does not need to scale beyond a single instance for the foreseeable future. It holds the routing table (campaign → server) and platform-level data (users, subscriptions). The technology choice for this instance (Postgres, libSQL, or otherwise) is not yet decided.

---

## Why SPA, Not SSR

Unchanged from the superseded ADR. The long pole in rendering a campaign view is the campaign checkout — downloading the libSQL file from object storage on a cold start. A SPA loads instantly from Bunny CDN, shows a loading skeleton, and handles the async checkout gracefully. SSR would block the entire page render waiting for the same operation. The SPA architecture decouples static asset serving (CDN) from data serving (campaign-pinned servers).

---

## Decisions Deferred to Implementation

- **Non-CRDT side channel framing.** How relationship change notifications and other server-authoritative push messages share the WebSocket with loro-protocol binary frames.
- **Campaign checkout prefetching.** Downloading the libSQL file on login for campaigns the user is likely to access, reducing cold-start latency.
- **Suggestion expiry mechanism.** TTL on suggestion marks, checked at render time or swept by the ThingActor.
- **Bulk suggestion review UX.** Post-SessionIngest review mode for pages with many suggestions.
- **Cursor awareness.** `loro-prosemirror` provides `LoroEphemeralCursorPlugin` and `CursorEphemeralStore`. Integration is straightforward but not yet specified.
