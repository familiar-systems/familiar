# familiar.systems — Vision Document

## The Problem

Running a tabletop RPG campaign generates an enormous amount of information: NPCs improvised on the fly, locations described in passing, plot threads introduced and forgotten, lore that contradicts what was established three sessions ago. The GM is expected to track all of it.

Today, GMs cobble together solutions from tools that weren't designed for this. Google Docs for session notes. Notion or wikis for world-building. Spreadsheets for tracking NPCs. None of these tools talk to each other, and none of them understand the structure of a campaign. The result is that most GMs either burn out on bookkeeping or let details slip — and the game suffers either way.

World-building tools like WorldAnvil and Kanka exist, but they treat the wiki as the primary artifact. The GM is expected to author and maintain a knowledge base as a separate activity from actually running the game. For most GMs, that's unsustainable.

## The Insight

The primary artifact of a TTRPG campaign is not a wiki. It's the **session** — what happened at the table. Everything else (the NPCs, the locations, the factions, the lore) is derived from that lived experience.

If we can capture what happens at the table — through audio recording, transcription, and the GM's own notes — then the knowledge base should **assemble itself** from that activity. The GM's job shifts from _authoring a wiki_ to _running their game and reviewing what the AI extracted_.

## The Product

A specialized, non-linear, AI-assisted campaign notebook. Two interlocking systems:

1. **The Journal** — captures what happened (sessions, recordings, narrative)
2. **The Things** — captures what exists in the world (NPCs, locations, items, factions, lore)

The AI is the connective tissue. It processes journal content, proposes new things and relationships, and keeps the campaign knowledge base growing with minimal GM effort. The AI layer connects to external language models — the hosted instance manages this; self-hosters configure their own provider.

The underlying structure is a **graph**: every entity is a node, every relationship is an edge, and content is composed of blocks that can be referenced and embedded across the graph.

### Distribution

familiar.systems is a **web application**. The GM opens a browser, logs in, and works. No installation, no local setup, no file management. Players access the same application with their own accounts and see only what the GM has made visible.

**Two deployment modes, one codebase:**

- **Hosted (primary)** — We operate a multi-tenant instance for paying customers. This is the default experience and the path with the lowest barrier to entry. A GM who isn't technical should be able to sign up and start capturing their first session in minutes.

- **Self-hosted** — The same application can be deployed by anyone on their own infrastructure. This serves enthusiasts who want control over their data, organizations with compliance requirements, and the open-source community. The [AGPL-3.0](https://www.gnu.org/licenses/agpl-3.0.html) license makes this explicit: the code is free to use, modify, and redistribute, provided modifications are shared under the same license.

**What this constrains:**

- The application is a single deployable artifact that works in both modes. No hard dependency on proprietary cloud services that a self-hoster can't replace.
- AI integration must be pluggable — the hosted instance uses managed API keys; self-hosters bring their own.
- Storage must support both multi-tenant (hosted, many GMs on one instance) and single-tenant (self-hosted, one group's campaigns) without architectural divergence.

---

## Core Concepts

### Campaign

The top-level container. A campaign holds everything: arcs, sessions, things, and the relationship graph that connects them. A GM might run multiple campaigns. Each campaign has its own graph, its own prototype things (templates), and its own emergent vocabulary of relationships.

A campaign can ship with a **starter pack** — a set of prototype things (NPC, Location, Item, Faction, etc.) and suggested relationship labels appropriate to the game system (D&D 5e, Mothership, Blades in the Dark, etc.). Prototypes are themselves things — editable pages that define what a new NPC or location looks like when created. These are defaults, not constraints. The GM can customize or ignore them.

### Arc

An optional narrative grouping across sessions. Arcs give structure to long-running campaigns — "The Siege of Grimhollow" might span sessions 7–12. Not every campaign uses arcs, and that's fine. They exist for GMs who think in terms of chapters or story beats.

Arcs are nodes in the graph. They can have their own content (theme notes, planned revelations, dramatic questions) and they connect to sessions via edges.

### Session

The fundamental temporal unit of a campaign. A session represents a single gathering at the table (or online). It carries:

- **Date and attendees** — which players were present (this matters for "what does this character know?")
- **Arc membership** — which arc(s) this session belongs to, if any
- **Raw sources** — audio recordings, transcription output, player-submitted notes
- **Journal entry** — the cleaned, canonical narrative of what happened
- **Prep notes** — what the GM planned before the session (optional but valuable for diffing plan vs. reality)

Sessions are ordered chronologically and form the spine of the campaign timeline.

### Journal Entry

The cleaned, GM-approved narrative of a session. This is the primary written artifact of the campaign — the record of what happened, in the GM's voice.

The workflow for producing a journal entry:

1. **Capture**: The GM records the session (audio) and/or takes rough notes during play
2. **Raw journal**: Recordings are transcribed and combined with any typed notes into a raw, unstructured dump
3. **AI draft**: The AI processes the raw journal against the campaign graph, producing a structured draft with entity references, suggested highlights, and narrative cleanup
4. **GM review**: The GM edits the draft — correcting errors, adjusting tone, adding context the AI missed
5. **Publication**: The final journal entry is saved. The AI extracts suggested things and relationships for the review queue.

The journal entry is composed of **blocks** (see below), and those blocks contain references to things in the campaign graph. When the GM writes "the party met Kael at the Rusty Anchor," both "Kael" and "Rusty Anchor" become clickable references to their respective nodes.

### Things

Things are the entities that make up the campaign world: NPCs, locations, items, factions, lore, monsters, player characters, and anything else the GM cares to track. Each thing is a node in the graph, populated with **blocks** of content. A thing can be created from a **prototype** — another thing marked as a template — which provides its initial page layout and block structure (an NPC page looks different from a location page because they were cloned from different prototypes).

Things are not authored in isolation. They emerge from play:

- The AI detects a new NPC mentioned in a journal entry and proposes creating a node for them
- The GM confirms, and the thing is created with whatever context the journal provides
- Over subsequent sessions, the thing accumulates more references, more detail, and more relationships

Things can also be created manually — the GM might want to pre-build a city before the party arrives. But the system should never _require_ upfront authoring. A thing can start as nothing more than a name and a single journal reference, and grow organically.

### Block

The atomic content unit. Everything inside a node — text, headings, stat blocks, images — is a block. Blocks are the grain at which content is referenced, embedded, and transcluded.

Key behaviors:

- **Block references**: Any block can be referenced from anywhere in the graph, like Notion or Logseq. "See the description of Grimhollow" can link to a specific paragraph on the Grimhollow page, not just the page itself.
- **Transclusion**: A block from one node can be embedded live in another. The goblin stat block defined on the Goblin monster page can be transcluded into the NPC page for "Mr. Foo Bard" (who is, apparently, a goblin). Edit it in one place, it updates everywhere.
- **Source linking**: Blocks derived from audio transcription carry a reference back to the timestamp in the original recording. The GM can always trace a claim back to "what was actually said at the table."

### Edges

The graph has two kinds of connections. Both are edges, but they serve different purposes, connect different things, and behave differently with respect to status.

**Mentions** are **block-to-node or block-to-block** links. They're referential — "this content points to that entity or that content." The source is always a block (since blocks are the atomic content unit), but the target can be a node or another block.

A block-to-node mention is an entity reference: a journal block says "Jormag and Linnea went to [The Wet Beer]," creating mentions to three things. A block-to-block mention is a content reference: a player character's page links to [a specific moment in Session 3's journal entry](block reference). Block-to-block mentions are what make transclusion work — a transcluded block is a mention that renders its target inline.

Mentions are derived, not authored. They're created automatically when the AI detects entity references in text, or when the GM writes an inline reference. They carry no label (the connection is always "mentions"), no meaningful direction, and no independent status — a mention inherits status from the block it lives in. If the block is GM-only, its mentions are too.

Mentions power backlinks ("where is this entity mentioned?"), context retrieval for the AI, and the clickable references throughout the graph.

**Relationships** are **node-to-node** links. They're semantic — they describe how two entities in the campaign world are connected. "Clericman the Good" worships "Murdergod." Kael frequents the Rusty Anchor. The Silver Compact is allied with the Crown of Ashenmoor.

Relationships are authored, not derived. The GM creates them directly, or the AI proposes them for review. They carry:

- **A label** — freeform, semantic: "worships", "frequents", "rules over", "is allied with"
- **An optional inverse label** — "worships" from one direction is "is worshipped by" from the other
- **Direction** — relationships point from source to target; the inverse label lets both directions read naturally
- **Independent status** — a relationship can be GM-only even when both nodes it connects are Known

The relationship vocabulary is freeform and emerges over time. The GM doesn't predefine an ontology of allowed labels. The AI clusters and normalizes labels as the campaign grows — suggesting that "works for" and "employed by" might be the same relationship.

**Mentions are the raw signal; relationships are the semantic interpretation.** When the AI processes "Jormag and Linnea went to Northport," the three mentions are automatic. The AI then _proposes_ relationships from that context: "Jormag → traveled to → Northport", "Linnea → traveled to → Northport." Those proposals land in the review queue. The GM accepts, edits, or ignores them. Mentions are exhaustive; relationships are curated.

### Status

A single field on every primitive — nodes, relationships, and blocks — that captures both visibility and canonicity in one concept. Mentions don't carry independent status; they inherit from the block they belong to. Three states, one mental model.

**The states:**

- **GM-only** — true and secret. Only the GM can see it. The AI uses it actively for context retrieval, suggestions, and consistency checking. This is the default for all new content.
- **Known** — true and public. Visible to everyone. The AI uses it actively. This is the standard state for anything established in play and shared with the table.
- **Retconned** — no longer true, but visible to everyone. The table established this in play and has since decided it didn't happen. The AI ignores it for connectivity and active world-state queries, but can reference it when asked ("what did we originally say about Kael's backstory?").

**The lifecycle:** Content starts GM-only, gets promoted to Known when revealed in play, and can be moved to Retconned if the table decides it didn't happen. If something is GM-only and the GM decides it never needed to exist, they just delete it — there's no history to preserve for something that never left the GM's head.

**Visual design:** The three states map directly to distinct visual treatments that are immediately learnable:

- **GM-only** → dimmed, in a tinted/colored container (a "secret" feel)
- **Known** → normal rendering, no adornment
- **Retconned** → struck through or crossed out (clearly "this was here, but it's done")

No icons, no badges, no tooltips needed. The visual language is the same across nodes, edges, and blocks, so a GM scanning a page instantly knows the status of every piece of content.

**How status applies across primitives:**

- **Nodes**: A GM-only node is completely invisible to players — it doesn't appear in searches, references, or the published journal. A Known node is visible, but individual blocks and edges within it can still be GM-only.
- **Relationships**: A relationship can be GM-only even if both nodes it connects are Known. "Clericman the Good" and "Murdergod" can both be Known entities, but the relationship between them is GM-only. Players viewing either node's relationship panel simply don't see that relationship.
- **Blocks**: Individual blocks within a Known node can be GM-only. Clericman's page shows his public description, his role in the temple, his stat block — but the paragraph about his secret allegiance is hidden from the player view.
- **Retconned** works at any level: a whole node can be retconned (a character who never existed), a single edge (a relationship that was undone), or a single block (a detail that was walked back).

**Design principles:**

- **Default to GM-only.** New content — whether created manually or AI-suggested — starts as GM-only. Promoting to Known is a deliberate act. It's always safer to hide something the players should see than to reveal something they shouldn't.
- **The GM always sees everything.** The GM's view shows all three states with clear visual differentiation. A toggle lets the GM preview "what do my players see?" without switching accounts.
- **Status cascades down, not up.** If a node is GM-only, all its blocks and edges are implicitly GM-only. If a node is Known, its blocks and edges can be independently set to GM-only or Known. A block cannot be Known if its parent node is GM-only.
- **Revealing is a narrative moment.** When the GM promotes a previously GM-only edge or block to Known, the system can optionally note when it was revealed — "revealed in Session 14." Over time, this creates a record of the campaign's dramatic arc of discovery.
- **Retconning preserves history.** The campaign's history includes its contradictions and course corrections. Retconned content stays visible as a record of what was once established, but the AI treats it as inert.

### Suggestion

A **suggestion** is a proposed mutation to the campaign graph. Every AI output that would modify the world — whether from session processing, interactive planning, or any other source — materializes as a suggestion. Suggestions are never applied automatically. The GM reviews and acts on each one.

**Suggestion types:**

- **Create thing** — a new node (NPC, location, item, etc.) cloned from a prototype thing, with initial blocks
- **Update blocks** — new or modified blocks on an existing node
- **Create relationship** — a new edge between two nodes, with label and optional inverse
- **Journal draft** — proposed journal entry blocks for a session
- **Contradiction** — a flag: "this content conflicts with established canon," with references to both sides

**Key properties:**

- **Always durable.** Suggestions are persisted the moment they're generated. If the GM closes the browser mid-session, every suggestion the AI has produced is still waiting when they come back.
- **Grouped into batches.** Related suggestions (e.g., "the three NPCs and their relationships from a tavern scene") are grouped into a **SuggestionBatch** — the unit of review. The GM can act on a batch in bulk or expand it and review individual suggestions.
- **Provenance.** Every suggestion links back to the conversation that produced it — the reasoning, the context, the GM's instructions. A suggestion is never context-free.
- **Auto-rejection.** Suggestions not acted on within a configurable window (~14 days) are automatically rejected. This keeps the suggestion queue fresh and prevents unbounded accumulation. Auto-rejected suggestions remain visible in their conversation's history.
- **Accepting creates real content.** When the GM accepts a suggestion, the system creates the corresponding node, blocks, or relationship with `gm_only` status (the default for all new content).

### Agent Conversation

An **agent conversation** is a persisted record of an AI interaction — what was said, what was proposed, and what decisions were made. Conversations are the provenance for suggestions.

**Why conversations are persisted:** A suggestion without its originating conversation is context-free. "Create NPC: Mysterious Figure" means nothing without the conversation that explains why the AI proposed it. The conversation IS the provenance.

**Conversation types:**

- **GM-initiated** — the GM opens the agent window and collaborates with the AI (planning, refinement, world-building)
- **Player-initiated** — a player opens the agent window and asks questions about the campaign
- **System-initiated** — the session processing pipeline completes and creates a conversation with its proposals attached

**Lifecycle:** Conversations with unresolved suggestions (pending status) surface prominently in the UI — they represent outstanding decisions. Once all suggestions are resolved (accepted, rejected, dismissed, or auto-rejected), the conversation fades from prominence but remains browsable in history.

---

## The Agent Window

The agent window is the single interface for all AI interaction in familiar.systems. It is a conversational surface — the user talks, the AI responds, and when appropriate, the AI produces structured suggestions alongside its conversational output.

### Focal Point

When the agent window opens, its context is determined by **where the user opened it**:

| Focal point                      | Context the AI starts with                                                  |
| -------------------------------- | --------------------------------------------------------------------------- |
| Session page (post-processing)   | Session transcript, extracted entities, journal draft, existing suggestions |
| Session page (pre-session)       | Recent session summaries, active plot threads, prep notes                   |
| Thing page (NPC, location, etc.) | The thing's blocks, relationships, all mentions across sessions             |
| Campaign overview                | High-level: arcs, major entities, open contradictions                       |

The focal point determines the AI's initial context retrieval. The GM can always pull in additional context with `@`-references to specific nodes or blocks.

### Tool Availability Determines Behavior

The agent window does not have modes. Instead, the user's role determines what tools the AI has access to:

- **Read tools** (all users): search entities, get entity details, get relationships, semantic search across content, session summaries
- **Write tools** (GM only): propose creating things, propose block updates, propose relationships, flag contradictions

When a player opens the agent window, the AI has only read tools and answers questions. When a GM opens it, the AI has both read and write tools. If the GM asks "tell me about Kael," the AI answers (Q&A). If the GM asks "flesh out Kael's backstory," the AI produces suggestions (planning & refinement). The tool set drives the behavior, not a mode flag.

### Three Workflows, One Interface

The agent window serves three use cases:

1. **Session processing review** — the system processes uploaded audio/notes and presents its proposals as a system-initiated conversation. The GM reviews suggestions, then can "continue from here" to refine interactively.
2. **Planning & refinement** — the GM opens the agent window and collaborates with the AI to build, expand, or refine campaign content. This works from any page — a session, an NPC, a location, the campaign overview.
3. **Q&A** — a GM or player asks questions about the campaign. The AI queries the graph (filtered by role) and answers. No suggestions produced.

These are not separate features. They are the same interface with different starting contexts and tool availability. See the [AI workflow unification design](plans/2026-02-14-ai-workflow-unification-design.md) for the full design.

---

## Workflows

### Post-Session Workflow (Primary)

This is the core loop. After every session, the GM goes through roughly this process:

```
Record/capture session
        ↓
Upload audio + notes to the session page, fill in metadata
        ↓
System processes the upload (transcription, entity extraction,
journal drafting, proposal generation)
        ↓
A system-initiated conversation appears on the session page:
  - Journal draft as suggested blocks
  - New things to create (3 new NPCs detected)
  - New relationships to add (Kael → Rusty Anchor: "frequents")
  - Updates to existing things (Tormund's status: now deceased)
  - Contradiction flags
        ↓
GM reviews suggestions (accept / edit / reject / dismiss)
  - One-click accept for obvious ones
  - Inline edit for things that need adjustment
  - "Continue from here" to refine interactively via the agent window
  - Skipping is fine — unreviewed suggestions auto-reject after ~14 days
```

The entire post-session process should take **15–30 minutes**, not hours. The AI does the heavy lifting; the GM does the judgment calls.

### Pre-Session Workflow (Prep)

Before a session, the GM needs to prepare. The GM opens the agent window from the upcoming session page, where the AI has session-relevant context loaded:

1. **Surfacing relevant context**: Based on where the last session ended, the AI pulls together relevant things — NPCs the party is likely to encounter, locations they're heading toward, unresolved plot threads
2. **Highlighting gaps**: "You've established that the party is traveling to Grimhollow, but you haven't defined what's there yet. Want to flesh it out?"
3. **Prep notes**: The GM writes plans, encounter ideas, secrets to reveal, and NPC motivations in a prep note attached to the upcoming session
4. **Interactive planning**: The GM collaborates with the AI to flesh out locations, build encounters, develop NPCs — all through the agent window, with suggestions landing as durable proposals
5. **Post-session diff**: After the session, the AI can compare prep notes to the actual journal — what happened vs. what was planned. The delta is where the most interesting world-state updates live (improvised NPCs, unexpected alliances, plans that went sideways)

### Ongoing World-Building Workflow

Between sessions, the GM might want to build out parts of the world that haven't come up in play yet. The GM can author manually or open the agent window from any thing page to collaborate with the AI:

- Create a new thing from a prototype (cloning its page layout and block structure)
- Fill in details, embed references to other things
- Open the agent window: "Flesh out @Grimhollow — it should be a ruined fortress connected to @SilverCompact"
- AI produces suggestions (new blocks, related entities, relationships) that the GM reviews
- Everything starts GM-only by default; the GM publishes nodes, blocks, and edges as they're revealed in play

This workflow is fully supported but never _required_. The system works even if the GM only ever interacts through the post-session loop.

### Player-Facing Workflow

Player access is part of the core architecture. The features below are prioritized after the GM-facing workflows are solid.

Players interact with the campaign knowledge base differently:

- **Published journal**: Players see only published blocks of the session journal. The GM controls exactly which parts of the narrative are player-visible — entire entries can be published, or specific blocks can be withheld (e.g., a GM-only block noting "the party didn't notice the assassin watching from the rafters").
- **Character ownership**: Players can edit their own character nodes — inventory, backstory, personal notes.
- **Player recollections**: Players can submit their own notes or memories of a session, which feed into the session as an additional source (multiple perspectives help the AI resolve ambiguity).
    - Player recollections aren't just AI input - they're part of the session's record. The final journal should include player-contributed blocks alongside the GM's own narrative, giving the session multiple voices.
- **Q&A via the agent window**: Players can open the agent window and ask questions about the campaign — "What do we know about the Silver Compact?", "When did we last visit Grimhollow?", "What happened last session?" The AI answers using only Known content; GM-only information is structurally invisible to the player's agent.
- **Filtered graph view**: Players see only published nodes, published edges, and published blocks. GM-only content is invisible — not redacted, not hinted at, simply absent. The player's view of an NPC page shows only what their characters would know, with no indication that hidden content exists.

---

## Design Philosophy

### The graph assembles itself

The GM's primary activity is running the game and writing about it. The knowledge base grows as a side effect of that activity. Maintaining the graph should never feel like a separate chore.

### AI proposes, the GM disposes

The AI is an editorial assistant, not an author. It produces suggestions — proposed mutations to the campaign graph — that the GM reviews and acts on. The AI cannot modify the graph directly; every change requires GM approval. Every suggestion links back to the conversation that produced it, so the GM can always trace why something was proposed.

### Tolerant of neglect

Not every GM will review every suggestion. The system stays useful even when the GM doesn't review everything. Suggestions that go unreviewed auto-reject after a configurable window (~14 days), keeping the suggestion queue fresh without punishing the GM for a busy fortnight. The system never piles up infinite unreviewed proposals — it assumes silence is "no" and moves on.

### Structure is discovered, not imposed

The GM doesn't design an ontology before session 1. Prototype things provide sensible defaults for page layout, but the relationship vocabulary between things is freeform and emerges over time. The AI clusters and normalizes labels as the campaign grows.

### The journal is the source of truth

When in doubt, what happened at the table is canonical. Things and relationships are derived from journal content. If the graph contradicts the journal, the journal wins. Retconning is always explicit — a deliberate act that preserves what was originally established while marking it as no longer active.
