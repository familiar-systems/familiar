# Loreweaver — Vision Document

## The Problem

Running a tabletop RPG campaign generates an enormous amount of information: NPCs improvised on the fly, locations described in passing, plot threads introduced and forgotten, lore that contradicts what was established three sessions ago. The GM is expected to track all of it.

Today, GMs cobble together solutions from tools that weren't designed for this. Google Docs for session notes. Notion or wikis for world-building. Spreadsheets for tracking NPCs. None of these tools talk to each other, and none of them understand the structure of a campaign. The result is that most GMs either burn out on bookkeeping or let details slip — and the game suffers either way.

World-building tools like WorldAnvil and Kanka exist, but they treat the wiki as the primary artifact. The GM is expected to author and maintain a knowledge base as a separate activity from actually running the game. For most GMs, that's unsustainable.

## The Insight

The primary artifact of a TTRPG campaign is not a wiki. It's the **session** — what happened at the table. Everything else (the NPCs, the locations, the factions, the lore) is derived from that lived experience.

If we can capture what happens at the table — through audio recording, transcription, and the GM's own notes — then the knowledge base should **assemble itself** from that activity. The GM's job shifts from *authoring a wiki* to *running their game and reviewing what the AI extracted*.

## The Product

A specialized, non-linear, AI-assisted campaign notebook. Two interlocking systems:

1. **The Journal** — captures what happened (sessions, recordings, narrative)
2. **The Things** — captures what exists in the world (NPCs, locations, items, factions, lore)

The AI is the connective tissue. It processes journal content, proposes new things and relationships, and keeps the campaign knowledge base growing with minimal GM effort. The AI layer connects to external language models — the hosted instance manages this; self-hosters configure their own provider.

The underlying structure is a **graph**: every entity is a node, every relationship is an edge, and content is composed of blocks that can be referenced and embedded across the graph.

### Distribution

Loreweaver is a **web application**. The GM opens a browser, logs in, and works. No installation, no local setup, no file management. Players access the same application with their own accounts and see only what the GM has made visible.

**Two deployment modes, one codebase:**

- **Hosted (primary)** — We operate a multi-tenant instance for paying customers. This is the default experience and the path with the lowest barrier to entry. A GM who isn't technical should be able to sign up and start capturing their first session in minutes.

- **Self-hosted** — The same application can be deployed by anyone on their own infrastructure. This serves enthusiasts who want control over their data, organizations with compliance requirements, and the open-source community. The [FSL-1.1-ALv2](https://fsl.software/) license makes this explicit: non-competing use is permitted immediately, and the code converts to Apache 2.0 after two years.

**What this constrains:**

- The application is a single deployable artifact that works in both modes. No hard dependency on proprietary cloud services that a self-hoster can't replace.
- AI integration must be pluggable — the hosted instance uses managed API keys; self-hosters bring their own.
- Storage must support both multi-tenant (hosted, many GMs on one instance) and single-tenant (self-hosted, one group's campaigns) without architectural divergence.

---

## Core Concepts

### Campaign

The top-level container. A campaign holds everything: arcs, sessions, things, and the relationship graph that connects them. A GM might run multiple campaigns. Each campaign has its own graph, its own templates, and its own emergent vocabulary of relationships.

A campaign can ship with a **starter pack** — a set of node templates and suggested relationship labels appropriate to the game system (D&D 5e, Mothership, Blades in the Dark, etc.). These are defaults, not constraints. The GM can customize or ignore them.

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

Things are the entities that make up the campaign world: NPCs, locations, items, factions, lore, monsters, player characters, and anything else the GM cares to track. Each thing is a node in the graph, defined by a **template** that gives it structure (an NPC has different fields than a location), and populated with **blocks** of content.

Things are not authored in isolation. They emerge from play:

- The AI detects a new NPC mentioned in a journal entry and proposes creating a node for them
- The GM confirms, and the thing is created with whatever context the journal provides
- Over subsequent sessions, the thing accumulates more references, more detail, and more relationships

Things can also be created manually — the GM might want to pre-build a city before the party arrives. But the system should never *require* upfront authoring. A thing can start as nothing more than a name and a single journal reference, and grow organically.

### Block

The atomic content unit. Everything inside a node — text, headings, stat blocks, images, AI suggestions — is a block. Blocks are the grain at which content is referenced, embedded, and transcluded.

Key behaviors:

- **Block references**: Any block can be referenced from anywhere in the graph, like Notion or Logseq. "See the description of Grimhollow" can link to a specific paragraph on the Grimhollow page, not just the page itself.
- **Transclusion**: A block from one node can be embedded live in another. The goblin stat block defined on the Goblin monster page can be transcluded into the NPC page for "Mr. Foo Bard" (who is, apparently, a goblin). Edit it in one place, it updates everywhere.
- **Source linking**: Blocks derived from audio transcription carry a reference back to the timestamp in the original recording. The GM can always trace a claim back to "what was actually said at the table."
- **AI suggestions**: A suggested block is visually distinct — clearly marked as unvetted. The GM can accept, edit, or reject it inline.

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

**Mentions are the raw signal; relationships are the semantic interpretation.** When the AI processes "Jormag and Linnea went to Northport," the three mentions are automatic. The AI then *proposes* relationships from that context: "Jormag → traveled to → Northport", "Linnea → traveled to → Northport." Those proposals land in the review queue. The GM accepts, edits, or ignores them. Mentions are exhaustive; relationships are curated.

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

---

## Workflows

### Post-Session Workflow (Primary)

This is the core loop. After every session, the GM goes through roughly this process:

```
Record/capture session
        ↓
Raw sources (audio + notes) land in the session
        ↓
AI transcribes audio, merges with notes into raw journal
        ↓
AI drafts journal entry using campaign graph for context
  - Resolves entity references ("the blacksmith" → Tormund, the NPC)
  - Suggests new entities detected in the narrative
  - Flags potential contradictions with established canon
        ↓
GM reviews and edits the journal draft
        ↓
Journal is finalized
        ↓
AI generates a review queue:
  - New things to create (3 new NPCs detected)
  - New relationships to add (Kael → Rusty Anchor: "frequents")
  - Updates to existing things (Tormund's status: now deceased)
        ↓
GM reviews the queue (accept / edit / reject)
  - One-click accept for obvious ones
  - Inline edit for things that need adjustment
  - Skipping is fine — unreviewed items stay as suggestions
        ↓
Campaign graph is updated
```

The entire post-session process should take **15–30 minutes**, not hours. The AI does the heavy lifting; the GM does the judgment calls.

### Pre-Session Workflow (Prep)

Before a session, the GM needs to prepare. The system supports this by:

1. **Surfacing relevant context**: Based on where the last session ended, the AI pulls together relevant things — NPCs the party is likely to encounter, locations they're heading toward, unresolved plot threads
2. **Highlighting gaps**: "You've established that the party is traveling to Grimhollow, but you haven't defined what's there yet. Want to flesh it out?"
3. **Prep notes**: The GM writes plans, encounter ideas, secrets to reveal, and NPC motivations in a prep note attached to the upcoming session
4. **Post-session diff**: After the session, the AI can compare prep notes to the actual journal — what happened vs. what was planned. The delta is where the most interesting world-state updates live (improvised NPCs, unexpected alliances, plans that went sideways)

### Ongoing World-Building Workflow

Between sessions, the GM might want to build out parts of the world that haven't come up in play yet. This is traditional wiki-style authoring:

- Create a new thing from a template
- Fill in details, embed references to other things
- Everything starts GM-only by default; the GM publishes nodes, blocks, and edges as they're revealed in play

This workflow is fully supported but never *required*. The system works even if the GM only ever interacts through the post-session loop.

### Player-Facing Workflow

Player access is part of the core architecture. The features below are prioritized after the GM-facing workflows are solid.

Players interact with the campaign knowledge base differently:

- **Published journal**: Players see only published blocks of the session journal. The GM controls exactly which parts of the narrative are player-visible — entire entries can be published, or specific blocks can be withheld (e.g., a GM-only block noting "the party didn't notice the assassin watching from the rafters").
- **Character ownership**: Players can edit their own character nodes — inventory, backstory, personal notes.
- **Player recollections**: Players can submit their own notes or memories of a session, which feed into the session as an additional source (multiple perspectives help the AI resolve ambiguity).
  - Player recollections aren't just AI input - they're part of the session's record. The final journal should include player-contributed blocks alongside the GM's own narrative, giving the session multiple voices.
- **Filtered graph view**: Players see only published nodes, published edges, and published blocks. GM-only content is invisible — not redacted, not hinted at, simply absent. The player's view of an NPC page shows only what their characters would know, with no indication that hidden content exists.

---

## Design Philosophy

### The graph assembles itself

The GM's primary activity is running the game and writing about it. The knowledge base grows as a side effect of that activity. Maintaining the graph should never feel like a separate chore.

### AI proposes, the GM disposes

The AI is an editorial assistant, not an author. It suggests; the GM approves. Every suggestion links back to the source material that triggered it. The GM always has the final word.

### Tolerant of neglect

Not every GM will review every suggestion. The system stays useful even with unvetted AI suggestions in the graph. Suggestions are visually distinct and carry a confidence tier (confirmed > accepted > suggested), but they're never invisible — they still power AI context retrieval and surface in searches.

### Structure is discovered, not imposed

The GM doesn't design an ontology before session 1. Node templates provide sensible defaults for page structure, but the relationship vocabulary between things is freeform and emerges over time. The AI clusters and normalizes labels as the campaign grows.

### The journal is the source of truth

When in doubt, what happened at the table is canonical. Things and relationships are derived from journal content. If the graph contradicts the journal, the journal wins. Retconning is always explicit — a deliberate act that preserves what was originally established while marking it as no longer active.
