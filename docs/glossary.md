# Glossary

> Terms and concepts used across familiar.systems documentation. Grouped by domain. Entries include references to the documents where they are defined in detail.

> Intended audience: Coding agents and developers

---

## Product & Business

**familiar.systems** - The product. An AI-assisted campaign world wiki for Game Masters running continuing TTRPG campaigns. Previously called Loreweaver. The name is a four-way pun: a wizard's familiar, tabletop RPG systems, a system that feels familiar, and agentic AI systems.

**GM (Game Master)** - The primary user. The person who runs the campaign, owns the campaign data, and has final authority over all content. All AI output requires GM approval.

**Continuing campaign** - A TTRPG campaign that spans multiple sessions with persistent world state. The product's exclusive target. One-shots are explicitly out of scope.

**Starter pack** - A bundled set of templates and suggested relationship labels appropriate to a game system (e.g., D&D 5e, Mothership). Ships with a campaign to provide sensible defaults. The GM can customize or ignore them.

**The Town Market** - The planned community marketplace for user-submitted starter packs. Deferred to post-multiplayer milestone.

**Notebook / Notebook + Audio** - The two pricing plans. Notebook (€5/month) is text-only. Notebook + Audio (€10/month) includes 8 session-hours of audio processing, with additional hours at €1 each.

> See [vision.md](vision.md) for the full product vision.

---

## Campaign Graph

The interconnected structure of nodes, blocks, and edges that represents the campaign world. Every campaign has its own independent graph.

The AI is chiefly responsible for assembling the graph, not the GM. The GM uploads session recordings, reviews AI proposals, and makes corrections - but the heavy lifting of entity extraction, relationship mapping, and wiki maintenance is the AI's job. The key promise is "15 minutes of review, not hours of bookkeeping." The terminology below describes the data model the AI constructs and the GM curates. We need to provide a good UI for humans to interact with this model directly when they choose to, but we are primarily concerned with providing an excellent harness for the AI to build it.

> _Vocabulary: a **page** is any node in the graph; an **entity** is the page kind that holds authored world content. The AI extracts entities; the graph is made of pages._

**Campaign** - The GM's entire body of work for one group playing in one world. The top-level container and the unit of data isolation - one database, one world, no cross-campaign queries or shared state. For GMs running a multi-year sandbox, the campaign _is_ the world. For GMs running sequential bounded stories (Curse of Strahd, then Tomb of Annihilation) in a shared setting, the campaign is the persistent world and each story is an arc within it.

**Page** - The universal node in the campaign graph. A page is one resource: URL-addressable (`/p/:id`), backed by a LoroDoc, composed of blocks, and connected to other pages by relationships. NPCs, locations, items, factions, lore entries, player characters, monsters, sessions, arcs, and tags are all pages. A page's `kind` (see **PageKind**) distinguishes the few cases that differ in document structure or need engine-level behavior - a session carries temporal data, a tag auto-generates a category listing - but everything else (NPC vs. Location, Villain vs. Quest-Giver) is expressed through tags, templates, and relationships, not the page type. Pages emerge from play - a page can start as nothing more than a name mentioned once and grow organically.

_"Page" is the supertype that reaches a URL and the API; it is not the word a GM authors with._ They create an NPC, a Location, a Session - always from a template or a kind. The creation menu reads `New → Session | Arc | Player Character | NPC | Location | ...`, not "New Page."

**PageKind** - The discriminant on a page (its `kind` field): `entity | session | skill | memory | template`. A kind exists only when pages differ in document **structure** (a different Loro schema) or need a **systemic action the engine can't infer from content** - never for editorial or semantic differences, which live in content, tags, and relationships. Today only `entity` and `template` exist in code; `session` lands with the audio pipeline, while `skill` and `memory` land with the agent system. Precedent: `TocEntryKind`. Adding a concept is adding a case, so the compiler points at every site that must handle it.

**entity** - The page kind (`kind == entity`) for authored world content: NPCs, locations, factions, items, lore, arcs, or whatever your templates define. The collective, user-facing noun for "what exists in the world" and the AI's extraction/resolution/search target end to end. The concrete label a GM picks for one entity stays specific - NPC, Location - via its tag and template; "entity" is the category, not the per-page label.

**Template** - A page of kind `template`: the page that `entity` pages clone from. Defines the default section structure and block layout for new entities of that type (an NPC template, a Location template). Creating an entity from a template clones its section structure; `templateId` tracks the lineage. Templates render a subset of an entity's sections (no derived sections like an AI "sounds like" block), carry OnCreate directives (e.g., `OnCreate: tag as #NPC`) and an AI Instructions block, are excluded from RAG/embedding, and are surfaced to the agent as a `create_page` template. `kind == entity` listings exclude them.

> See [templates-as-pages](plans/2026-02-20-templates-as-pages.md) for the full design.

**Skill** _(future kind)_ - A page of kind `skill`: a GM-authored, campaign-specific instruction the agent loads to do its work ("how to draft a journal for this table", "how we name our taverns"). Loaded by the agent as instruction, not world content; excluded from `kind == entity` listings. Distinct from a **Memory** by provenance (the GM writes skills; the AI writes memories), and from the shipped **Global skills** of the Agent Instruction Stack, which are product-level instruction files rather than pages in the campaign graph. Lands with the agent system.

**Memory** _(future kind)_ - A page of kind `memory`: the AI's durable, long-term notes about the campaign, accumulated across sessions and carried forward - learned patterns and standing facts about how this table plays. AI-authored, GM-curated (the GM can read, edit, and prune them). Like a **Skill**, it is loaded by the agent as instruction, not world content, and excluded from `kind == entity` listings; the difference is provenance. Lands with the agent system.

**Block** - The atomic content unit. Everything inside a page - text, headings, stat blocks, images - is a block. Blocks are the grain at which content is referenced, embedded, and targeted by suggestions. Each block has a UUID (`BlockId`).

**Embed** - A block from one page embedded live in another. Edit it in one place, it updates everywhere. An embed is both a rendering behavior (show content inline) and an edge in the graph (a block-to-block mention), making the dependency queryable.

**Arc** - A page representing a narrative grouping across sessions. "The Siege of Grimhollow" spanning sessions 7-12. Optional - not every campaign uses them. For GMs who run sequential bounded stories in a persistent world, each "campaign" in their parlance maps to an arc in the product's model.

> See [vision.md](vision.md) for the core concepts (Campaign, Page, Block, Arc).

### Sessions

**Session** - A page with special behavior: the campaign's central unit and the event everything else falls out of. The fundamental temporal unit and the unit of knowledge time, representing a single gathering at the table. The temporal coordinate for the entire campaign's state history - "show me the world as of session 10" means "after all relationship mutations from session 10 have been applied." Sessions are ordered chronologically and form the campaign timeline's spine.

A new session automatically links to the previous session by date. For West Marches or interleaved play, the GM can change this - what matters narratively is the last session _these characters_ were in, not the most recent calendar entry. Attendance records are load-bearing for narrative continuity, not just for tracking "who was there."

The sub-entities below are data _on_ the session page, not pages themselves:

**Session Prep** - Freeform text with @mentions, written by the GM before the session. Plans, contingencies, dramatic questions. No relationships are created. No structured data. The @mentions give the AI signal about which entities are relevant to the upcoming session. The diff between prep and journal is valuable signal ("what happened vs. what was planned").

**Session Sources** - The raw inputs that feed the journal pipeline, on one of two paths. **With audio**: the recording, the GM summary, and (optionally) player notes; the recording is transcribed into speaker-attributed words - note that diarization only attributes _who spoke_, while resolving those words against the campaign's entities is a later AI stage. **Without audio**: the GM's or players' written recap and notes become the primary input, functionally the journal's raw material. Either way, consumed by SessionIngest: inputs to processing, not artifacts the system reasons about independently after the journal is produced.

**GM Summary** - The GM's 4-5 bullet points or sentences about what happened. Lives within session sources. Has a precise role in the AI pipeline: it transforms extraction from open-ended ("what happened?") to guided ("find where _these things_ happened and fill in the gaps"). This is the oracle input - the segmentation prior and verification scaffold. _(When the GM doesn't upload audio, their notes become the primary input to journal drafting - functionally, they are writing the journal's raw material directly.)_

**Session Journal** - The cleaned, GM-approved narrative of a session. The primary written artifact of the campaign and the canonical source of truth for what happened - for both the AI and players. AI-drafted from session sources, then GM-reviewed. Composed of blocks containing references to pages.

The journal records _events_. The graph records _state_. "The party killed the baron" is a journal fact. "The baroness is a widow" is a graph mutation - a relationship change proposed by the AI as a _consequence_ of that journal fact, accepted by the GM. The journal is the ledger; the graph is the derived world state.

> See [entity-relationship-temporal-model](plans/2026-06-23-entity-relationship-temporal-model.md) for how sessions serve as the temporal coordinate for the relationship graph.

### Edges

**Mention** - A link from a block to a page (or to another block). Created by typing `@` followed by a name in any block on any page - `@Kael` creates a clickable reference to Kael's page. The editor resolves the name against the campaign graph via autocomplete.

Mentions are derived, not authored - created automatically when the AI detects entity references in text or when the GM writes an `@`-reference. They carry no label (the connection is always "mentions") and inherit status from their parent block. Mentions power backlinks ("where is this page mentioned?"), context retrieval for the AI, and clickable references throughout the wiki. Embeds are a special case of block-to-block mention that renders its target inline.

**Relationship** - A node-to-node semantic connection. Bidirectional: carries a forward predicate and a reverse predicate (e.g., "is a resident of" / "is the home of") in a single row. Two pages can have multiple concurrent relationships - the Duke and the Duchess are both "married to" and "rivals with" each other, each a separate row. Predicates are immutable - when a relationship evolves, the old row is ended (superseded) and a new one replaces it, or a new row coexists alongside it. The GM decides which.

The primary way relationships enter the graph is through the AI: the GM uploads session sources, the AI proposes relationship changes based on what happened, and the GM reviews and accepts. Manual tools exist for direct manipulation, but the point is to let the AI handle the bookkeeping. Relationships have an immutable, non-nullable origin: either `prior` (true before the campaign started) or `session(FK)` (became true in the context of that session).

**Tag** - A page representing a classification (e.g., `#NPC`, `#Human`). Tags are never created explicitly - tagging a page with `#Villain` auto-creates the Villain tag page if it doesn't exist. Tagging is a relationship with the label `tagged`. A tag's page auto-generates a listing of everything tagged with it, exactly like a [Wikipedia category page](https://en.wikipedia.org/wiki/Category:2001_establishments_in_the_United_States). The GM can add content to a tag's page - notes like "NPCs in this campaign tend to be untrustworthy" become context the AI uses when working with tagged entities.

> See [entity-relationship-temporal-model](plans/2026-06-23-entity-relationship-temporal-model.md) for the relationship schema and temporal model.
> See [ai-serialization-format-v2](plans/2026-03-25-ai-serialization-format-v2.md) for how mentions and relationships appear in the agent's markdown format.

### Relationship Lifecycle

A relationship moves along **two orthogonal, authored, session-stamped axes**. **Factuality** is when it was true in the fiction (`[origin, superseded)`, plus a terminal retcon); **Knowledge** is when the players learned it. Neither is inferred - the GM (or an AI suggestion the GM accepts) stamps the session each event happened in. Both axes' corrections are reversible while the row lives.

**Origin** - The factuality start: where a relationship fact came from. Always present, never nullable, immutable. Either `prior` (primordial world state) or `session(n)` (became true in the context of session n).

**Superseded** - The factuality end: a relationship that was true and is no longer because the fiction moved forward. `superseded_session_id` records the session it ended (NULL = still true). Remains visible in snapshots before that session because it was true at the time. Reversible (un-end clears the stamp).

**Retconned** - A relationship the GM declares was never true in the fiction, even if it was established in play. `retcon_session_id` records the session the correction was made (for the timeline and diff), but the row is excluded from *every* snapshot regardless of T. The row is kept because retcons are part of the tapestry of the game; it strikes factuality but preserves knowledge. Reversible (un-retcon). GM-only operation.

**Deleted** - Hard delete, no audit trail. For relationships that should never have existed: GM changed their mind about a never-established relationship, or the AI proposed something incorrect and the GM accidentally accepted it. Not an axis stamp. GM-only operation.

**Knowledge** - The knowledge axis, replacing the old timeless `visibility` flag: a relationship is **Public** (known to the players, `is_secret = false`), **Hidden** (secret, GM-only, not yet revealed), or **Revealed** at a session (secret, learned by the players then). The axis is **freely mutable** - the GM reveals, conceals (`Public → Hidden`), or re-publicizes a fact, set wholesale to any state. Unlike the old flag it is session-stamped, so "what did the players know at session T" is answerable; concealing a public fact is lossy (it keeps no record the fact was public), acceptable for a correction tool. Per-player visibility is a future expansion.

**Reveal** - The session the players learned a secret fact (`reveal_session_id`, NULL = not yet revealed). Revealing in the same session a fact became true reads as plain public (no hidden interval). Reversible (re-hiding or re-publicizing clears it). A retcon does not touch this axis: if the players were told a thing and it was later retconned, the record that they believed it survives.

> See [entity-relationship-temporal-model](plans/2026-06-23-entity-relationship-temporal-model.md) for the full two-axis lifecycle and GM manual tools.

### Status

A single field on pages and blocks, capturing both visibility and canonicity. (Relationships do not carry this `Status`; they move along the two temporal axes above - Knowledge and Factuality - instead.) Status applies at two levels: a whole page can be GM-only (the secret villain the players don't know exists yet), or individual blocks within a Known page can be GM-only (the NPC the players have met, but they don't know he's secretly a vampire).

**GM-only** - True and secret. Only the GM can see it. The AI uses it for context retrieval and consistency checking. Default for all new content.

**Known** - True and public. Visible to everyone. Standard state for anything established in play and shared with the table.

**Retconned** - No longer true, but visible to everyone. The table established this in play and has since decided it didn't happen. The AI ignores it for active world-state queries but can reference it on explicit request.

**Status tightening** - Internal implementation constraint: in page content, status can only tighten as you descend the heading hierarchy, never loosen. A `[known]` block inside a `[gm_only]` section is a parse error. Not user-facing - enforced by the serialization compiler.

> A relationship's player-visibility is **not** a `Status` value; it is the freely-mutable, session-stamped **Knowledge** axis (Public / Hidden / Revealed) - see Relationship Lifecycle above.

> See [vision.md](vision.md) for the status design philosophy.

---

## AI System

**"AI proposes, GM disposes"** - The core contract. Every AI output that would change the campaign graph materializes as a durable suggestion. The AI has no write path that bypasses the suggestion layer. This is a hard architectural boundary, not a guideline.

**"Tolerant of neglect"** - Design principle. The system stays useful when the GM doesn't review promptly. Unreviewed suggestions auto-reject after ~14 days. The system never piles up infinite homework.

**Oracle quality** - The insight that the binding constraint on AI pipeline performance is the quality of the GM's input (the GM summary, timeline review), not the capability of the underlying model. The GM's review is the primary error firewall.

### Workflows

**SessionIngest** - The batch processing workflow. Triggered when the GM uploads session sources. Runs the AI pipeline: audio processing on GPU workers, entity extraction, journal drafting, and proposal generation. Output is a system-initiated conversation with suggestion batches. Long-running (minutes to tens of minutes), must survive deploys.

**P&R (Planning & Refinement)** - Interactive workflow. The GM opens the agent window and collaborates with the AI to plan, refine, and expand campaign content. The AI has read and write tools. Low-latency, streaming responses.

**Q&A (Question & Answer)** - Interactive, read-only workflow. Same agent window as P&R, but the AI has only read tools. Players use Q&A through a status-filtered view that structurally cannot reveal GM-only content.

**Focal point** - The context anchor when the agent window opens. Determined by where the user opened it (a session page, an entity page, campaign overview). Determines initial context retrieval, not capabilities.

**"Continue from here"** - The escape hatch for dead-end conversations. The GM starts a new conversation from the same focal point. The new conversation's AI has access to previous conversations and their suggestion history. Suggestions from the abandoned conversation remain durable and dismissable.

> See [ai-workflow-unification-design](plans/2026-02-14-ai-workflow-unification-design.md) for how the three workflows share one interface and suggestion model.
> See [ai-prd](plans/2026-02-22-ai-prd.md) for full AI system requirements.

### Suggestions

**Suggestion** - A durable, reviewable proposal from the AI. The universal output primitive for all AI write operations. Persisted the moment it's generated. Types: `create_page`, `update_blocks`, `create_relationship`, `journal_draft`, `contradiction`.

**Suggestion mark** - The underlying representation of a content suggestion. A mark on block UUID ranges with proposed replacement content as metadata. The original blocks remain in the document tree unchanged. Follows TipTap's comment-mark pattern.

**Blocking semantics** - Implementation detail: blocks under pending suggestion marks are read-only in the CRDT editor. The GM can edit the AI's proposed replacement before accepting it, but cannot edit the original text underneath while the suggestion is pending. To edit the original, reject the suggestion first. _(User-facing: "You can't edit this text while there's an active suggestion.")_

**Suggestion batch** - A group of related suggestions from a single context. The unit of review. The GM can act on the batch in bulk or expand it and act on individual suggestions.

**Supersession** - Within the same conversation, a new suggestion targeting the same blocks replaces the old one. Across conversations, suggestions always coexist - independent proposals deserving independent review.

**Auto-rejection** - Suggestions not acted on within ~14 days are automatically rejected. Keeps the queue fresh. Auto-rejected suggestions remain visible in conversation history. Ideally, expiry would be user-configurable and/or default to expire on the start of the _next_ session but that's an implementation detail.

**Suggestion outcomes table** - Records the resolution of every suggestion: accepted, rejected, superseded, or expired. Serves both UX (decorating conversation history) and evals (accept/reject rates per model).

**Contradiction** - A special suggestion type that proposes no graph mutation. Flags an inconsistency between new content and established canon, with references to both sides.

> See [ai-serialization-format-v2](plans/2026-03-25-ai-serialization-format-v2.md) for the suggestion mark model and compiler pipeline.

### Conversations

**Agent conversation** - A persisted interaction between the GM and the AI. The provenance for suggestions - every suggestion links back to the conversation that produced it. Conversations are durable; closing the browser mid-conversation loses nothing.

**Hammock time** - The ability to close a conversation, step away for days, and resume with full history. Enabled by conversation persistence.

**System conversation** - A conversation initiated by SessionIngest (not the GM). Framed as if the system said "I processed your session, here's what I found."

### Serialization & Retrieval

**Serialization compiler** - The stateless service that transforms between LoroDoc state and the agent's markdown format. Two directions: `f()` (LoroDoc → markdown) and `f⁻¹()` (agent tool call → compiled suggestion). Not an actor - a pure function with multiple inputs.

**Compiled suggestion** - The output of `f⁻¹()`. Contains target block IDs, proposed content, and provenance. Ready for the PageActor to apply as a mark.

**The linker** - The component that resolves `{Name}` mentions to graph nodes using fuzzy/alias matching. Shared between serialization and compilation. Handles renames via alias matching. Flags ambiguity for GM review.

**Retrieval tiers** - Progressive disclosure levels for the serialization format:

- **Tier 1 (Index Card)** - Preamble + tags + relationships + TOC. ~100-150 tokens. Used when packing many pages into context.
- **Tier 2** - Index card + selected section content. Used for related entities that need more context.
- **Tier 3 (Full Page)** - Complete serialized page with all content. Used when the agent is actively editing.

**Preamble** - The content between the H1 and the first structural element. The most important text on the page for retrieval - the index card. Dense with identity, role, affiliations. No explicit tag marks it; position defines it.

**TOC (Table of Contents)** - A computed summary of page structure with word counts per section. Not editable content. Appears in tier 1 and 2 to let the agent estimate context cost before requesting the full page.

> See [ai-serialization-format-v2](plans/2026-03-25-ai-serialization-format-v2.md) for the full format specification, retrieval tiers, and agent write tools.

### AI Pipeline (Audio Processing)

Audio goes in, structured session data comes out. Processing runs on GPU workers decoupled from the application cluster. The GM summary guides extraction, transforming it from open-ended transcription to guided search. The pipeline's detailed phase design is still in progress.

**The Jorm problem** - The reason the pipeline must be pipelined, not parallel. Fixed-segment parallel processing fails on entity handoff across segment boundaries - an entity introduced in segment N is unknown to segment N+1 if they're processed in parallel.

> See [ai-prd](plans/2026-02-22-ai-prd.md) for SessionIngest requirements.
> Pipeline phase design: TBD (future ADR).

### Agent Instruction Stack

Three layers, most specific wins:

1. **Global skills** - Shipped with the product. General capabilities like `create-or-edit-preamble.md`, `draft-journal-entry.md`.
2. **Template AI instructions** - Campaign-specific, per-template, GM-editable. Define what a specific template needs.
3. **(Future) System-specific skills** - Game-system knowledge from starter packs. Currently part of the global layer for Milestone 1.

> See [ai-serialization-format-v2](plans/2026-03-25-ai-serialization-format-v2.md) for how the instruction stack composes.

---

## Architecture

### Two-Binary Split

**Platform (app server)** - Auth (Hanko), campaign CRUD, membership/access control, routing table, shard registry, heartbeat/lease management, billing authority. Pricing formulas always live here. Talks to `platform.db`.

**Campaign server (shard)** - Real-time collaborative editing via Loro CRDTs and TipTap. Campaign-scoped REST, WebSocket collaboration, actor lifecycle, AI agent conversations. Stateful, campaign-pinned, long-lived connections. Talks to per-campaign `*.db` files.

**The network boundary** - The platform and campaign server always communicate over HTTP, even in development. No `Local*` implementations. The boundary enforces architectural discipline that willpower alone won't maintain.

> See [deployment-architecture](plans/2026-03-30-deployment-architecture.md) for the full service topology, graceful restart protocol, and preview environment design.
> See [project-structure-design](plans/2026-03-26-project-structure-design.md) for the Cargo workspace, TypeScript packages, and dependency graph.

### Campaign Lifecycle

**Checkout** - Downloading a campaign's libSQL file from object storage to local disk, opening connections, and spawning the actor tree. Triggered when a user connects to a campaign that isn't already checked out. The checkout-first invariant: nothing happens to a campaign without checkout, including deletion.

**Writeback** - Periodic upload (~30 seconds) of the local campaign file to object storage for durability during active use.

**Release** - Final upload to object storage followed by local file deletion. Consumes `self` - use after release is a compile error.

**Lease** - The mechanism ensuring a campaign has at most one owning server at any time. Lease-based routing through the platform's routing table. Concurrent lease acquisitions resolve atomically.

**Discover endpoint** - `GET /api/campaigns/:id/connect`. The SPA calls this to find its campaign server. If the campaign isn't checked out, the platform assigns it to the least-loaded server.

**Cold checkout** - A campaign not previously on this server. Requires downloading the libSQL file from object storage (seconds). Warm checkout (file cached on local disk) is sub-millisecond.

**Graceful restart** - SIGTERM triggers per-campaign drain: snapshot actors, writeback to object storage, release lease. Heartbeat continues throughout. ~30 second budget.

> See [campaign-collaboration-architecture](plans/2026-03-25-campaign-collaboration-architecture.md) for the checkout/checkin lifecycle and scaling model.

### Actor System (kameo)

**CampaignSupervisor** - Root actor per campaign. Handles checkout/checkin, spawns and tracks child actors, routes WebSocket messages, manages the database connection. Pure orchestration - implements no domain traits.

**PageActor** - One per active page (any kind). Holds a LoroDoc, implements CrdtRoom (collaborative editing) and Persistent (snapshots to libSQL). Evictable on idle timeout.

**TocActor** - Manages the campaign's table of contents. CrdtRoom + Persistent.

**RelationshipGraph** - In-memory graph of page-to-page relationships using `petgraph`. Queryable (REST, AI context) + Persistent. Not a CrdtRoom - server-authoritative.

**CampaignVocabulary** - In-memory projection of all page names. Powers editor autocomplete and STT correction fuzzy matching. Notifiable (pushes changes to clients) + Queryable. Derived from page data, so not independently Persistent. Lives for the campaign's lifetime - too cheap and too depended-on to evict.

**UserSession** - Per-user-per-campaign. Tracks which rooms the user has joined, manages WebSocket message routing.

**AgentConversation** - Per conversation. Holds conversation state for P&R or Q&A. Carries a conversation ID that stamps provenance onto suggestions.

**DatabaseActor** - Owns the single read-write connection to the campaign's libSQL file. Serializes all writes. Exists for non-blocking IO and clean shutdown drain, not for correctness.

> See [campaign-actor-domain-design](plans/2026-05-04-campaign-actor-domain-design.md) for actor traits, message patterns, persistence, and eviction. The trait taxonomy (interface traits vs. pattern traits) draws from the mindset in _Functional and Reactive Domain Modeling_ by Debasish Ghosh.

### Persistence Model

**Relational data is the data.** At rest, the libSQL tables are the source of truth. LoroDoc blobs are transient CRDT plumbing, not a second source of truth.

**Lossless reconstruction** - The `snapshot()` → relational → `restore()` round-trip must preserve all rendered content. Tested on every schema change.

**`CampaignReader`** - A trait providing a domain-typed read algebra over a pool of read-only libSQL connections (WAL mode). Actors never see connections, queries, or rows.

**`CampaignStore`** - A trait encapsulating storage lifecycle (local vs. object storage). Local impl is no-ops. Hosted impl downloads/uploads. Owned by the CampaignSupervisor, not the DatabaseActor.

**Snapshot** - An actor writing its current LoroDoc state to relational data via the DatabaseActor.

**Restore** - Reconstructing a LoroDoc from relational data when an actor starts. Free functions, not actor methods, because each actor needs different inputs.

**Campaign-as-file isolation** - Each campaign is a self-contained libSQL database file. Enables branch deployment (`cp`), trivial GDPR deletion, and the scaling model.

> See [campaign-collaboration-architecture](plans/2026-03-25-campaign-collaboration-architecture.md) for the persistence invariants.
> See [sqlite-over-postgres-decision](discovery/2026-03-09-sqlite-over-postgres-decision.md) for the libSQL choice.

### Temporal Queries

**Snapshot query** - "Show me the world as of session N." Returns all relationships true at N: `origin` is `prior` or ≤ N, `superseded` is NULL or > N, and not retconned. A player-facing snapshot additionally keeps only `Public` or `Revealed(s ≤ N)` rows (the knowledge post-filter). The mechanism that makes the relationship graph rewindable through time.

**Diff query** - "What changed in session N." Returns all relationships with any axis event in N: origin, superseded, reveal, or retcon. A reveal is a first-class diff event (the party learned something), distinct from a factuality change. The basis for session-level change summaries.

> See [entity-relationship-temporal-model](plans/2026-06-23-entity-relationship-temporal-model.md) for the query semantics and in-memory representation.

---

## Infrastructure

**libSQL** - SQLite-compatible database. Used directly (no ORM). Each campaign is a libSQL file. `platform.db` is the platform's single database.

**Loro** - CRDT library. LoroDoc is the in-memory collaborative document state. Synced via `loro-dev/protocol` (room-based multiplexing). Replaced Yjs.

**kameo** - Rust actor framework. One actor per document/concern. Independent async tasks with per-actor persistence and eviction.

**Hanko** - Authentication provider. JWT verification happens independently on both platform and campaign server via shared code in `crates/app-shared/`.

**`@familiar-systems/editor`** - The TypeScript package containing the TipTap extension list. The single source of truth for document structure that both browser and campaign server must agree on.

**ML workers** - Stateless GPU jobs for transcription and diarization. Deploy as k8s Jobs, decoupled from the application cluster. Receive audio file references, return structured transcripts.

> See [infrastructure](plans/2026-05-23-infrastructure.md) for the full infrastructure stack (Hetzner, k3s, OpenTofu, Bunny.net, Nebius, etc.).
> See [deployment-architecture](plans/2026-03-30-deployment-architecture.md) for service topology, worker deployment, and job dispatch.
> See [project-structure-design](plans/2026-03-26-project-structure-design.md) for the Cargo workspace, TypeScript packages, and dependency graph.

---

## Design Principles (Named)

**"AI proposes, GM disposes"** - See AI System section.

**"Tolerant of neglect"** - See AI System section.

**"The journal is the source of truth"** - When graph and journal conflict, the journal wins. The journal records events; the graph records derived state. Entities and relationships emerge from journal content.

**"Structure is discovered, not imposed"** - The GM doesn't design an ontology before session 1. Relationship vocabulary is freeform and emerges over time. The AI clusters and normalizes.

**"The graph assembles itself"** - The knowledge base grows as a side effect of running the game and writing about it. Maintaining the graph should never feel like a separate chore.

**"Nothing happens without checkout"** - The checkout-first invariant. All reads and writes require the libSQL file to be on local disk. Universally preserved, including for deletion of inactive campaigns.

**"Oracle quality, not model capability"** - See AI System section.

**"Pipelined over parallel"** - The AI audio pipeline processes sequentially, not in parallel segments. See "The Jorm problem."

**"One topology everywhere"** - Local dev, preview, production, and self-hosting all run the same split binaries communicating over HTTP.
