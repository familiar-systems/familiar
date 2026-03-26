# ADR: Document-Centric Campaign Architecture

**Status:** Superseded by [Campaign Collaboration Architecture](../../plans/2026-03-25-campaign-collaboration-architecture.md) -- all eight architectural hypotheses validated ([experiment repo](https://github.com/loreweaver-no/experiment-hocuspocus-agent-collab), [hypotheses](https://github.com/loreweaver-no/experiment-hocuspocus-agent-collab/blob/main/hocuspocus-hypotheses.md)), then implementation technology replaced (Yjs/Hocuspocus/Node.js to Loro/kameo/Rust)
**Date:** 2026-03-14
**Supersedes:** None (new decision area)
**Related decisions:** [SPA project structure](./2026-02-14-project-structure-spa-design.md), [AI workflow unification](../../plans/2026-02-14-ai-workflow-unification-design.md), [Templates as prototype pages](../../plans/2026-02-20-templates-as-prototype-pages.md)

---

## Context

Loreweaver is a SaaS platform for tabletop RPG groups. Its core workflow: multi-hour session audio recordings go in, and out come transcripts, speaker-attributed journals, entity graphs, and a persistent campaign wiki. The product is built around a rich text editor (TipTap on ProseMirror) where GMs and players view and edit campaign content, and where an AI agent proposes changes based on session processing and interactive conversations.

Three actors interact with page content:

1. **Human editors** (GM and players) via a browser-based TipTap editor connected over WebSocket
2. **The interactive AI agent** during Planning & Refinement conversations in the sidebar
3. **The batch AI pipeline** (SessionIngest) processing audio recordings into journal drafts and entity proposals

All three actors need to read page content and (with appropriate authorization) write to it. Human editors write freely. The AI agent and batch pipeline write only suggestions — tagged blocks that the GM reviews before they become permanent. The architectural question is: how does page state flow between these actors, how is it persisted, how are conflicts resolved, and how does the system scale?

### Constraints

- **Solo developer.** Operational complexity matters. Every moving part is maintained by one person.
- **Database-per-campaign on libSQL.** Each campaign is an isolated libSQL database file. No shared multi-tenant database. This was chosen for structural isolation, trivial testing (`:memory:` databases), and the self-hosting story (a campaign is a file you can copy). This isolation also turns out to be the natural sharding boundary for horizontal scaling.
- **"AI proposes, GM disposes."** The AI must never modify the campaign graph without GM approval. All AI output is provisional until explicitly accepted.
- **"Tolerant of neglect."** The system must stay useful when the GM doesn't review AI output promptly. Suggestions auto-expire. The system doesn't require per-session user input upfront.

---

## Decision

### The campaign as the atomic unit

Every campaign is a self-contained libsql database file. There are no cross-campaign queries, no shared tables, no joins between campaigns. A campaign's database contains block records, mention records, entity relationships and graph edges, search text and embeddings, state vectors for optimistic concurrency, and campaign-specific metadata. During active editing sessions, Y.Doc blobs are also present as transient CRDT plumbing (see "Y.Doc blob lifecycle" below).

This isolation was chosen for GDPR data deletion (delete a campaign = delete a file) and turns out to be the foundation of the entire scaling model. Each campaign is an independent shard. No distributed transactions, no consensus protocols.

### Object storage as source of truth, local disk as cache

Object storage (Hetzner Object Storage) is the authoritative location for all campaign database files. Local disk on each application server is a working cache.

**Campaign checkout:** When a user connects to a campaign, the routing table (a lightweight map of campaign ID → server address in the platform database, currently libSQL -- see [libSQL decision](../../discovery/2026-03-09-sqlite-over-postgres-decision.md)) is consulted. If the campaign is already checked out on a server, route there. If not, assign it to the least-loaded server. That server downloads the libsql file from object storage to local disk and opens it.

**Active session:** All reads and writes run against the local embedded libsql file at NVMe speed. No network hop in the hot path. Hocuspocus documents are lazy-loaded into memory as users open pages — reconstructed via `toYdoc()` from relational data on cold checkout, or loaded from the Y.Doc blob if one exists from a prior active session (crash recovery).

**Periodic writeback:** A debounced process (e.g. every 30 seconds) flushes the local libsql file back to object storage for durability. This bounds the worst-case data loss window.

**Campaign release:** When the last user disconnects and an idle timeout elapses, Hocuspocus evicts all the campaign's documents from memory, `onStoreDocument` fires a final time to ensure relational data is current, Y.Doc blob columns are nulled out, and the blob-free file is written back to object storage. The campaign is deregistered from the routing table.

**Lease-based ownership:** Each campaign checkout is a lease with a heartbeat. The router will not reassign a campaign until the lease expires. This prevents two servers from having the same campaign open simultaneously. If a lease-expiry race causes brief overlap, Yjs CRDTs merge gracefully — no data loss, possible minor editing artifacts.

### Co-located Hono + Hocuspocus per server

On each server, a Hono HTTP server and the Hocuspocus WebSocket server run in the same Node.js process. Because the routing model pins all users of a campaign to the same server, all Hocuspocus document state, libsql file access, and API endpoints for that campaign are co-located. The WebSocket endpoint serves both human editor collaboration and AI agent participation. The HTTP endpoints serve document status queries and a fire-and-forget write path for agent writes to inactive pages.

The SPA loads from Bunny CDN with zero campaign awareness. On login, the client resolves the correct server address from the routing table and directs all API and WebSocket connections there.

### Hocuspocus as the central authority for page state

Every page in Loreweaver (session journals, Thing pages, campaign wiki entries) is a Y.Doc managed by Hocuspocus. The Y.Doc is the authoritative representation of page content. All reads and writes to page content flow through or are derived from Yjs CRDTs.

Hocuspocus is a long-running Node.js process that holds active documents in an in-memory map. Documents are loaded from the local libsql file on first access and evicted after all clients disconnect. Persistence happens on a debounced schedule (every 2-5 seconds of quiet, with a hard maximum interval).

Human editors connect via WebSocket from the browser. The AI agent connects the same way — via a server-side `HocuspocusProvider` over WebSocket — when writing to active pages. For writes to inactive pages, an HTTP endpoint uses `DirectConnection` internally for a faster fire-and-forget path. Both connection types are first-class Hocuspocus participants; the only difference is that the agent's writes are permission-constrained to suggestions (tagged blocks).

### Relational data is the data, Y.Doc blobs are transient CRDT plumbing

The relational tables in libsql (blocks, mentions, entity references, search text, embeddings) are the application's data. The AI agent queries them. The API serves them. Search indexes them. RAG retrieves from them. Nothing outside of Hocuspocus ever touches a Y.Doc blob.

During active editing, the Y.Doc in Hocuspocus memory is the authoritative state for that page — the write-ahead log. The relational tables are a derived view, eventually consistent with the Y.Doc. The consistency window is the `onStoreDocument` debounce interval (2-5 seconds).

**Critical invariant:** During active editing, never update the relational data directly and expect the editor to reflect it. All mutations go through the Y.Doc (either via the editor UI or server-side via `DirectConnection`). The materialization path is strictly one-way while a document is active.

### Materialization pipeline in onStoreDocument

When Hocuspocus persists a Y.Doc (on the debounced schedule), the `onStoreDocument` hook does two things:

1. **Stores the Y.Doc blob** — the raw CRDT state needed for merge correctness during the active session. This is a nullable `BLOB` column in the documents table. It exists only while the campaign is actively checked out.
2. **Extracts relational data** — walks the ProseMirror JSON tree (via `TiptapTransformer.fromYdoc()`) and upserts block records, mention records, and status flags into relational tables.

H5 confirmed that `TiptapTransformer.fromYdoc()` preserves custom attributes (id, status) without needing extensions registered on the server. Only `toYdoc()` (JSON → Y.Doc) requires extensions. The field name is `'default'`. This means the collab server's extraction pipeline can run without importing the full editor extension set.

### Y.Doc blob lifecycle

The Y.Doc blob is a transient working artifact, not permanent storage.

**During active sessions:** The blob exists in the libsql file and is included in periodic writebacks to object storage. This is essential for crash recovery — if the server dies, another server checks out the file from object storage and clients reconnect with buffered local edits. The blob must be present for the CRDT merge to work correctly during reconnection (Hocuspocus issue #344).

**On clean eviction (zero connections, idle timeout passed):** The `onStoreDocument` hook fires a final time, ensuring relational data is fully current. The blob columns are then nulled out. The final writeback to object storage sends a blob-free file. This means every file at rest in object storage contains only relational data — smaller files, faster cold checkouts.

**On cold checkout (campaign not already local):** The server downloads the blob-free libsql file from object storage. When a user opens a page, `onLoadDocument` reconstructs a fresh Y.Doc via `toYdoc()` from the relational data. Both the server and the reconnecting browser have fresh Y.Docs with no CRDT history to conflict. There is nobody to merge with, because zero connections was the precondition for eviction.

**Lossless reconstruction requirement:** The `toYdoc()` reconstruction path must produce a Y.Doc whose rendered output is identical to the original. This is a testable invariant: round-trip a document through `fromYdoc()` → relational storage → `toYdoc()` and diff the editor output. No rendered information may be lost. CRDT history (tombstones, client IDs, operation log) is intentionally discarded — we don't care about page edit history.

**`toYdoc()` requires extensions.** H5 confirmed this. The reconstruction code path needs the TipTap extension set registered, unlike the `fromYdoc()` extraction path. This dependency only applies to the cold-checkout reconstruction, not to the hot-path `onStoreDocument` extraction.

### The AI agent as a collaboration participant

The AI agent interacts with Hocuspocus in the same way a human editor does — as a WebSocket participant with a live Y.Doc that stays in sync via the CRDT protocol. The only difference is a permission constraint: the agent can only write suggestion-tagged blocks, never permanent content. The GM reviews and accepts/rejects suggestions in the editor.

This symmetry is the key simplification. The agent doesn't use a separate write protocol, a separate concurrency model, or a separate API. It's a participant. The CRDT handles merge. Connected users see the agent's suggestions appear in real-time. The agent sees the users' edits in real-time.

**Two write paths, selected by page activity:**

**Active page (someone is editing, or the agent expects to write).** The agent infrastructure connects via a server-side `HocuspocusProvider` over WebSocket. The agent receives the current Y.Doc state through the normal sync protocol, reasons about it, and writes suggestion blocks directly into the live Y.Doc. All connected clients see the suggestions appear instantly. Between LLM tool calls, the agent checks whether its local Y.Doc state has changed — a local in-memory state vector comparison, no round trip. If the document changed while the LLM was thinking, the agent re-reads from its now-updated local Y.Doc and adjusts. This eliminates the H1 deletion blind spot for the WebSocket path, because deletions propagate through the CRDT sync in real-time even though they don't change the state vector.

**Inactive page (nobody is editing, fire-and-forget write).** The agent submits mutations via the HTTP write endpoint, which opens a `DirectConnection` internally, applies the update via `transact()` in ~2ms (H6), and returns. The page doesn't need to stay loaded in Hocuspocus memory afterward. This is cheaper than establishing a WebSocket connection for a single write to a page nobody is looking at.

**The routing decision is simple:** attempt the write. If the page is active in Hocuspocus (checked via `documents.get()`, confirmed by H3), connect as a participant. If not, use the HTTP path. The agent infrastructure handles this transparently — to the LLM, it's just a tool call.

### Agent reads

**Bulk context reads (RAG, entity search, graph traversal)** come from the relational data in libSQL. The agent queries materialized block records, mention records, and embeddings. This avoids loading Y.Docs into Hocuspocus memory for non-collaborative read-only access.

**Full-page reads when the agent intends to write** are handled by the connection model above. If the page is active, the agent connects as a participant and reads the live Y.Doc — the freshest possible state, including edits within the debounce window. If the page is inactive, the agent reads from libSQL, which has the canonical persisted state. The routing is hidden behind a `readPage()` abstraction.

Because Hono and Hocuspocus are co-located on the same server, all routing is an in-process map lookup followed by either a local WebSocket connection or a local libsql query. No network boundary for either path.

### Optimistic concurrency

For the **WebSocket path** (active pages), the CAS pattern from H1/H7 is largely unnecessary. The agent holds a live Y.Doc that's being kept in sync. Between tool calls, the agent compares its local state vector against what it saw before the LLM call — a local in-memory comparison. If the document changed, the agent already has the updated content in its local Y.Doc. No re-read needed, no round trip. The retry is: notice the change, feed the updated content to the next LLM call.

For the **HTTP path** (inactive pages), the CAS pattern still applies as originally designed. The agent reads content plus a state vector from libSQL, reasons, and includes the state vector in the write request. The endpoint compares vectors inside `DirectConnection.transact()` (atomic, H2 confirmed). If stale, the write is rejected and the agent re-reads. In practice, this path targets pages nobody is editing, so staleness is rare.

The proposal tagging mechanism remains the ultimate safety net regardless of path. Even if the agent writes suggestions based on slightly stale content, the GM reviews them in context before they become permanent.

### Block-level proposals for document edits, suggestion infrastructure for graph mutations

**Document-level proposals** (journal drafts, block edits to existing pages) use tagged CRDT blocks. The AI writes blocks with `agent_proposal: true` directly into the Y.Doc. The GM reviews them in-context in the editor — approval is removing the tag, rejection is deleting the block, editing is just editing. No separate review UI needed.

**Graph-level proposals** (create new Thing, create relationship, flag contradiction) use the suggestion infrastructure described in the AI workflow design. These can't be CRDT blocks because there's no Y.Doc to write them into — the Thing doesn't exist yet, or the proposal is a graph edge rather than page content. These proposals live in the suggestion queue in libsql, are reviewed in a dedicated UI, and upon acceptance create the real content.

### Materialization barrier for batch writes

SessionIngest needs all proposals materialized to libsql before creating the system conversation that references them. H8 confirmed that `storeDocumentHooks(doc, payload, true)` forces immediate persistence, bypassing the debounce. Note: `flushPendingStores()` does not exist in Hocuspocus v3 despite appearing in some v2 documentation.

**Upgrade risk:** `storeDocumentHooks` is the only hypothesis with medium upgrade risk. The method is exported in type declarations but requires manually constructing an `onStoreDocumentPayload` with 8 fields. The payload shape is an implementation detail. Mitigation: isolate payload construction into a thin wrapper with an integration test, or accept natural debounce timing if sub-100ms materialization latency isn't critical.

### Why no Redis

The original Hocuspocus scaling model uses Redis pub/sub to synchronize Y.Doc updates across multiple Hocuspocus instances. This is necessary when users editing the same document might be connected to different servers.

The campaign-pinning routing model eliminates this need entirely. All users of a campaign are routed to the same server. The Hocuspocus instance on that server holds all active Y.Docs for that campaign. There is no cross-instance document sync because there is no cross-instance document access.

### Nothing happens to a campaign without checkout

A campaign must be checked out on a server before any actor — human, AI agent, or batch pipeline — can read from or write to it. This is not a convention; it's structural. The libsql file must be on local disk before it can be opened.

This eliminates race conditions by design. If a user connects to a campaign while SessionIngest is writing journal drafts and entity proposals, both actors are on the same server, in the same Node.js process, operating against the same local libsql file, with Hocuspocus mediating all document writes through the CRDT layer. There is no window where the user sees stale data because both actors are going through the same Hocuspocus instance. There is no concurrent write hazard because Hocuspocus serializes writes through `onStoreDocument`. There is no split-brain because the routing table enforces single-server ownership.

The checkout model means the system never needs to reason about "what if two processes on different machines are modifying the same campaign simultaneously" — that situation is architecturally impossible.

---

## Why This Architecture

### Why not have the AI read and write through the database directly?

If the AI bypassed Hocuspocus and wrote block records directly to libSQL, the editor and database would diverge. The Y.Doc wouldn't contain the AI's proposals, so connected clients wouldn't see them until the next page reload. Worse, when `onStoreDocument` next fires, the Y.Doc's state would overwrite whatever the AI wrote to the database, silently dropping the proposals. The Y.Doc must be the single source of truth for page content, and all writes must go through it.

### Why not have the AI read through Hocuspocus exclusively?

The appealing argument: one system owns page state, the agent always talks to that system. Read and write go through the same channel. Conceptual symmetry, and the agent always sees the absolute latest state including edits within the debounce window.

We rejected this for four reasons:

**Memory pressure for read-only access.** When the agent opens a DirectConnection to read a page, Hocuspocus loads the entire Y.Doc into memory if it isn't already there. For a single page, that's fine — maybe 100KB. But SessionIngest processes a 3-hour recording and needs campaign context to produce good suggestions. That means reading the current state of potentially dozens of Things to understand what's already established about NPCs, locations, and relationships mentioned in the transcript. If each read goes through Hocuspocus, you're loading 20-50 Y.Docs into memory on the collab server not because anyone is editing them, but because the AI pipeline needs to read them. Those documents then sit in memory until the eviction timeout fires, consuming resources for no collaborative editing purpose. Hocuspocus is being used as a read cache for a use case it wasn't designed for.

**The staleness window is negligible.** The strongest argument for reading through Hocuspocus is freshness — you see edits that haven't been persisted yet. But that window is the debounce interval: 2-5 seconds. For an AI agent reasoning about campaign lore to produce suggestions, being 2-5 seconds stale is completely irrelevant. The GM isn't going to ask the agent to flesh out a backstory and then race to edit that same page within the next 3 seconds. The freshness guarantee you'd be paying for with the architectural coupling buys almost nothing in practice.

**The data already exists in libSQL.** The `onStoreDocument` hook already extracts ProseMirror JSON and materializes blocks, mentions, and statuses to relational tables. That data exists. It's queryable. It's indexed. Reading it is a simple SQL query with no WebSocket handshake, no Y.Doc deserialization, and no dependency on documents being loaded into Hocuspocus memory. The infrastructure is already there.

**Smart routing covers the edge case.** For full-page reads where the agent intends to write, the agent connects as a participant to active pages (getting freshness for free via the CRDT sync) and reads from libSQL for inactive pages. This gives freshness exactly when it matters without paying the cost when it doesn't.

### Why SPA, not SSR?

The long pole in rendering a campaign view is the campaign checkout — downloading the libsql file from object storage on a cold start. A SPA loads instantly from Bunny CDN, shows a loading skeleton, and handles the async checkout gracefully. SSR would block the entire page render waiting for the same operation, giving the user a blank tab instead of a responsive loading state. The SPA architecture also fully decouples static asset serving (CDN, no campaign awareness) from data serving (campaign-pinned servers).

### Why optimistic concurrency instead of just letting the CRDT merge everything?

The CRDT guarantees structural convergence but is blind to semantic coherence. If the agent reads a page, reasons about it for 15 seconds, and writes proposals based on content the GM deleted 10 seconds ago, the CRDT happily merges both — structurally valid but semantically nonsensical.

For the WebSocket path (active pages), this is handled naturally: the agent's live Y.Doc receives the deletion in real-time, so the agent can detect the change between tool calls and adjust. For the HTTP path (inactive pages), the state vector CAS check catches this case and forces the agent to re-read. The proposal tag is the ultimate fallback regardless of path — even stale proposals are visible and reviewable rather than silently applied.

### Why tagged CRDT blocks for document proposals instead of a separate suggestion system?

For proposals that ARE blocks in a known document, the editor provides a superior review experience. The GM sees proposals in context. A separate review UI would show the same blocks in isolation. The suggestion infrastructure is reserved for proposals that aren't blocks (entity creation, relationships, contradictions) where the editor-as-review-UI pattern doesn't apply.

---

## Scaling Model

### Now: Single Server

One Hetzner VPS, one Volume as local cache, object storage as source of truth. The campaign checkout/checkin pattern, object storage writeback, routing table, and lease model are all implemented from the start — the routing table just has one entry. Multiple Node processes on the same machine via k3s replicas, with campaign-to-process affinity via consistent hashing.

All code written in this phase works unchanged in the next phase. The only difference between "now" and "next" is infrastructure, not application logic.

**Transition trigger:** Sustained CPU or memory saturation on the single server during peak session hours.

### Next: Multiple Servers

Add servers. Each has its own local disk. The routing table now has multiple entries. Ingress routes campaign-bound requests to the correct server. New campaigns are assigned to the least-loaded server. Rebalancing is "copy a small file to object storage (it's already there), update the routing table."

No Redis. No distributed database. No consensus protocol. Just files on disks behind a router.

**Transition trigger:** Campaign checkout latency is a measurable UX problem (many users hitting cold checkouts simultaneously).

### Later: Prefetch on Login

When a user logs in, look up their campaign memberships. For campaigns not already checked out anywhere, preemptively pull them to a server and register ownership. For campaigns already live on another server, cache the route so the SPA can connect instantly. User clicks a campaign and it's already warm.

Eviction is natural: no connected users + idle timeout → flush to object storage, release, deregister.

---

## Key Discoveries from Hypothesis Validation

These findings were not documented anywhere in the Hocuspocus, Yjs, or TipTap ecosystems at the time of testing. They were discovered empirically during the [experiment](https://github.com/loreweaver-no/experiment-hocuspocus-agent-collab).

1. **Yjs `clientID` constructor option is ignored in v13.6.** `new Y.Doc({ clientID: 1 })` assigns a random clientID anyway. Don't rely on deterministic client IDs.
2. **Yjs deletions don't change state vectors.** `Y.Map.delete()` marks existing Items as tombstones without creating new Items. CAS misses pure-delete edits.
3. **`TiptapTransformer.fromYdoc()` doesn't need extensions.** The collab server's `onStoreDocument` hook can extract ProseMirror JSON without importing the full editor extension set. Only `toYdoc()` needs extensions.
4. **TipTap v3 `element: null` breaks UniqueID and Collaboration.** The Editor constructor needs a real DOM element. In Node.js, use jsdom and pass `document.createElement("div")`.
5. **Hocuspocus v3 provider `attach()` gotcha.** When passing a pre-built `websocketProvider`, `attach()` is NOT called automatically. Always let the provider create its own websocket layer by passing `url` + `WebSocketPolyfill`.
6. **Hocuspocus v3 API change: `Server.configure()` is gone.** Use `new Hocuspocus({...})` directly.
7. **`flushPendingStores()` does not exist in Hocuspocus v3.** Use `storeDocumentHooks(doc, payload, true)` instead.
8. **HTTP write latency is ~4x faster than WebSocket for single writes.** The HTTP path uses `DirectConnection.transact()` (~2ms). The WS path goes through the async provider sync protocol (~10ms). HTTP is preferred for fire-and-forget writes to inactive pages; WebSocket is preferred when the agent needs live sync with active editors.

---

## Consequences

### What this architecture gives us

- **Single authority for page state during editing** (the Y.Doc in Hocuspocus), eliminating divergence between what the editor shows and what the database stores. At rest, relational data is the sole representation.
- **Real-time AI collaboration.** The agent is a participant in the same CRDT collaboration session as human editors. Suggestions appear in real-time. Edits by humans are visible to the agent in real-time. No polling, no refresh, no separate protocol.
- **Symmetric interaction model.** Humans and the AI agent connect to Hocuspocus the same way, through the same protocol. The only difference is a permission constraint (suggestions only). This reduces the number of code paths and the conceptual surface area of the system.
- **Natural conflict resolution.** The CRDT handles structural merging. The proposal tag handles semantic review.
- **Offline resilience for human editors.** The browser holds a full Y.Doc replica. Edits during network interruptions are buffered and merged on reconnection.
- **Trivial horizontal scaling.** Campaign-per-file isolation means adding servers is "add a box, update the routing table." No Redis, no distributed database, no consensus protocol.
- **Blob-free files at rest.** Campaign files in object storage contain only relational data — no CRDT history accumulation. Smaller backups, faster cold checkouts.
- **The test harness is the production code.** The spike harness (Hono + Hocuspocus + libSQL + Vitest) is structurally identical to the real implementation.

### What this architecture costs us

- **Two proposal mechanisms.** Document-level proposals (tagged CRDT blocks) and graph-level proposals (suggestion queue) use different storage, different review UIs, and different acceptance flows.
- **Materialization lag.** The libSQL read model lags behind the Y.Doc by 2-5 seconds.
- **Lossless reconstruction requirement.** The `toYdoc()` cold-checkout path must produce rendered output identical to the original. Any attribute, node type, or nesting structure that the `fromYdoc()` extraction doesn't capture into relational tables will be lost on eviction. This is testable and must be tested for every schema change.
- **`toYdoc()` extension dependency.** The cold-checkout reconstruction path requires TipTap extensions registered on the collab server. The hot-path `fromYdoc()` extraction does not.
- **Campaign checkout latency on cold start.** Downloading the libsql file from object storage adds latency for the first user to access a campaign that isn't already local. Mitigated in the "Later" phase by prefetching on login.
- **`storeDocumentHooks` upgrade risk.** The forced-persistence API is semi-internal and its payload shape could change between Hocuspocus minor versions. Requires an integration test and thin wrapper.

---

## Key Invariants

- **Object storage is always authoritative.** Local libsql files are a cache.
- **During active editing, the Y.Doc is authoritative.** Relational data is derived from it via `onStoreDocument`. Never write directly to relational tables while a document is active in Hocuspocus.
- **At rest, relational data is the data.** Y.Doc blobs are nulled on clean eviction. Cold checkout reconstructs Y.Docs from relational data via `toYdoc()`.
- **Lossless reconstruction.** The `fromYdoc()` → relational → `toYdoc()` round-trip must preserve all rendered content. Tested on every schema change.
- **Nothing happens to a campaign without checkout.** All reads and writes require the libsql file to be on local disk, checked out from object storage. This eliminates cross-server race conditions by design.
- **A campaign has at most one owning server at any time** (enforced by lease).
- **No cross-campaign state or queries.** The campaign file is the complete unit.
- **All mutations flow through the Y.Doc** during active editing. Humans and AI agents are both Hocuspocus participants. The agent is permission-constrained to suggestion-tagged blocks only. Never write directly to materialized tables.
- **The platform database is the only centralized stateful system** and does not need to scale beyond a single instance for the foreseeable future.

---

## References

### Hocuspocus / Yjs

- [Hocuspocus GitHub](https://github.com/ueberdosis/hocuspocus) — MIT license, source code
- [Hocuspocus overview](https://tiptap.dev/docs/hocuspocus/getting-started/overview)
- [Hocuspocus hooks](https://tiptap.dev/docs/hocuspocus/server/hooks) — lifecycle API
- [Hocuspocus server examples](https://tiptap.dev/docs/hocuspocus/server/examples) — Hono integration, DirectConnection usage
- [Hocuspocus persistence guide](https://tiptap.dev/docs/hocuspocus/guides/persistence)
- [Hocuspocus.ts source](https://github.com/ueberdosis/hocuspocus/blob/main/packages/server/src/Hocuspocus.ts)
- [Yjs documentation](https://docs.yjs.dev/)
- [Yjs document updates API](https://docs.yjs.dev/api/document-updates) — encodeStateVector, encodeStateAsUpdate, applyUpdate
- [y-protocols sync protocol](https://github.com/yjs/y-protocols/blob/master/PROTOCOL.md)
- [@hocuspocus/transformer](https://www.npmjs.com/package/@hocuspocus/transformer)
- [@hocuspocus/provider](https://www.npmjs.com/package/@hocuspocus/provider)

### TipTap / ProseMirror

- [TipTap Editor documentation](https://tiptap.dev/docs)
- [TipTap Collaboration extension](https://tiptap.dev/docs/editor/extensions/functionality/collaboration)
- [ProseMirror](https://prosemirror.net/)

### Validation

- [Hypothesis experiment repo](https://github.com/loreweaver-no/experiment-hocuspocus-agent-collab) — 42 tests, all 8 hypotheses confirmed
- [Hypothesis testing plan](https://github.com/loreweaver-no/experiment-hocuspocus-agent-collab/blob/main/hocuspocus-hypotheses.md) -- claims, references, spikes, and test harness design

### Loreweaver design documents

- [AI workflow unification](../../plans/2026-02-14-ai-workflow-unification-design.md)
- [SPA project structure](./2026-02-14-project-structure-spa-design.md)
- [Templates as prototype pages](../../plans/2026-02-20-templates-as-prototype-pages.md)
- [Storage overview](../discovery/2026-02-14-storage-overview.md)
