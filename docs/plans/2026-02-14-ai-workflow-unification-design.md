# Loreweaver — AI Workflow Unification

## Context

Loreweaver's AI layer serves three distinct use cases: processing raw session data into structured content, helping the GM build and refine their world interactively, and answering questions about the campaign. Prior discovery work explored these separately — [audio pipeline architecture](../discovery/audio_ingest/audio_overview.md) for session processing, and [interactive vs background AI](../discovery/ai_workflows/interactive-vs-background.md) for the execution model split.

This document unifies them. The core insight: **all three workflows converge on the same product primitives**. They differ in how they start (batch vs interactive, system-initiated vs user-initiated) but produce the same outputs (suggestions to the campaign graph) and are consumed through the same interface (the agent window).

---

## The Three Workflows

### SessionIngest

**What it is:** After a session, the GM uploads audio recordings and/or notes. A batch pipeline processes this raw material into a journal draft and proposes new entities, relationships, and updates to the campaign graph.

**How it works from the GM's perspective:**

1. Navigate to the session page (created during prep, or created now)
2. Upload audio, fill in metadata (date, attendees)
3. The system processes the upload — transcription, entity extraction, journal drafting, proposal generation
4. When processing completes, a conversation appears on the session page with the results: a proposed journal and a batch of suggestions (new NPCs detected, relationships inferred, updates to existing entities, contradiction flags)
5. The GM reviews the suggestions — accepting, editing, rejecting, or dismissing in bulk
6. If the GM wants to refine anything, they open the agent window and continue from there

**Key property:** SessionIngest is a **system-initiated conversation**. The batch pipeline's output is framed as something the system said to the GM, with suggestions attached. This means the GM has one mental model for all AI output — it always arrives as a conversation with proposals.

### Planning & Refinement (P&R)

**What it is:** The GM opens the agent window and collaborates with the AI to build, expand, or refine campaign content. This is interactive from the start — the GM is watching and guiding.

**Examples:**

- On a session page after ingest: "The journal missed that Kael seemed nervous. Work that in and add a relationship to the Silver Compact."
- On an NPC page: "Flesh out Grimbeard's backstory. He should have a connection to @Ashenmoor."
- On an upcoming session page: "I need three encounters for the journey to @Northport. The Silver Compact should be shadowing the party."
- From the campaign overview: "What unresolved plot threads do I have? Which ones connect to @Kael?"

**How it works:**

1. The GM opens the agent window from wherever they are in the application
2. The AI loads context based on the **focal point** — the page the GM is on
  * _Nice to have:_ be able to remove the page's context
3. The GM describes what they want, `@`-referencing specific nodes or blocks for additional context
4. The AI streams a response, producing suggestions as it goes — new things, block updates, relationships
5. Suggestions appear in real-time and are immediately durable
6. The GM reviews inline or comes back later

**Key property:** P&R is a **GM-initiated conversation** that produces the same suggestion primitives as SessionIngest. The agent window is the same; the starting context is different.

### Q&A

**What it is:** A GM or player asks a question about the campaign. The AI answers by querying the campaign graph. No suggestions are produced — this is a pure read operation.

**Examples:**

- Player: "What do we know about the Silver Compact?"
- GM: "When did we last encounter Kael? What sessions mention him?"
- Player: "What happened in the last session?" (for a player who missed it)

**How it works:**

1. The user opens the agent window
2. They ask a question
3. The AI retrieves relevant content from the campaign graph, filtered by the user's role
4. The AI streams a text answer with references to specific nodes, blocks, and sessions

**Key property:** Q&A uses the **same interface** as P&R. The difference is not a mode toggle — it's **tool availability**. Q&A has read tools only; P&R has read and write tools. The AI's behavior emerges from what it can do, not from being told which mode it's in.

**Player access:** Players use Q&A through the same agent window. The campaign graph is filtered to show only Known content — the AI cannot reveal GM-only information because it never sees it. Players do not have access to write tools, so they cannot trigger P&R behavior.

### How They Relate

```
SessionIngest ──[batch]──→ System-initiated conversation + Suggestions
                                                                    ↘
P&R ──────────[interactive]──→ GM-initiated conversation + Suggestions  → Same review model
                                                                    ↗      Same suggestion primitives
Q&A ──────────[interactive]──→ Text answer (no suggestions)                Same agent window
```

SessionIngest is batch processing that produces the same output as P&R. Q&A is the same interface without write capabilities. The product has **one AI interaction model**, not three.

---

## The Agent Window

The agent window is the single interface for all AI interaction. It is a conversational surface — the GM (or player) talks, the AI responds, and when appropriate, the AI produces structured suggestions alongside its conversational output.

### Focal Point

When the agent window opens, its context is determined by **where the user opened it**:

| Focal point | Context the AI starts with |
|---|---|
| Session page (post-ingest) | Session transcript, extracted entities, journal draft, existing suggestions from ingest |
| Session page (pre-session) | Recent session summaries, active plot threads, prep notes, upcoming session metadata |
| Thing page (NPC, location, etc.) | The thing's blocks, relationships, all mentions across sessions, connected entities |
| Campaign overview | High-level: arcs, major entities, open contradictions, unresolved threads |

The focal point determines the AI's **initial context retrieval**, not its capabilities. The GM can always pull in additional context with `@`-references: "Flesh out @Grimhollow, connecting it to @SilverCompact and @Kael."

### Tool Availability Determines Behavior

The agent window does not have modes. Instead, the user's role determines what tools the AI has access to:

**Read tools** (available to all users):
- Search entities by name, type, or description
- Get full entity details (blocks, relationships)
- Semantic search across content blocks
- Get session summaries and journal entries
- Get relationship context for entities

**Write tools** (GM only):
- Propose creating a new thing
- Propose updating blocks on an existing thing
- Propose creating a relationship between things
- Propose a journal draft or journal block edits
- Flag a contradiction between content

When a player opens the agent window, the AI has only read tools and answers questions. When a GM opens it, the AI has read and write tools and can produce suggestions when the conversation warrants it. There is no explicit "switch to P&R mode" — if the GM says "tell me about Kael," the AI answers (Q&A behavior). If the GM says "flesh out Kael's backstory," the AI produces suggestions (P&R behavior). The tool set, not a mode flag, drives this.

In the future, we may want to add scoping by user to pages or block sections to enable players to update their own context. However, this is NICE TO HAVE and not a requirement or priority.

### Status Filtering

The campaign graph is filtered **at the retrieval layer** based on the user's role:

- **GM:** Sees GM-only + Known content.
  - _Nice to have_: Retconned content is excluded from active context but can be retrieved when explicitly asked ("what did we originally say about Kael's backstory?").
- **Player:** Sees Known content only. GM-only content is invisible — not redacted, simply absent.

This means the AI structurally cannot reveal GM-only information to players. The filter is applied before the AI sees any content, not after.

### Visual Model

The agent window takes inspiration from Zed's ACP connector and multibuffer model. When the AI produces suggestions that touch multiple nodes (e.g., "flesh out Grimhollow" creates the location update + 3 NPCs + 5 relationships), the GM sees all affected content simultaneously — not just a chat transcript, but a multi-pane view of the actual nodes being modified with suggestions highlighted inline.

The specifics of this visual model are deferred to frontend design. The product requirement is: **the GM must be able to see, in one view, everything the AI is proposing to change across the graph**.

---

## Suggestions

### The Universal Primitive

A **Suggestion** is a proposed mutation to the campaign graph. Every AI output that modifies the world — whether from SessionIngest or P&R — materializes as a suggestion. Suggestions are never applied automatically. The GM reviews and acts on each one.

**Suggestion types:**

| Type | What it proposes |
|---|---|
| `create_thing` | A new node (NPC, location, item, faction, etc.) with a template and initial blocks |
| `update_blocks` | New or modified blocks on an existing node |
| `create_relationship` | A new edge between two nodes, with label and optional inverse label |
| `update_relationship` | A modification to an existing relationship |
| `journal_draft` | Proposed journal entry blocks for a session |
| `contradiction` | A flag: "this content conflicts with established canon," with references to both sides |

**Suggestion properties:**

- **Type-specific payload**: Each suggestion type carries different data. A `create_thing` has a template and initial blocks. A `create_relationship` has source, target, label, and optional inverse. The payload is structured per type, not a generic blob.
- **Target**: Which existing node this affects (null for `create_thing`).
- **Source references**: Which content blocks triggered this suggestion — the AI's evidence for why it's proposing this.
- **Status**: `pending` → `accepted` | `rejected` | `dismissed`.
- **Provenance**: Which conversation produced this suggestion.

This section is not set in stone. It would be beneficial to have evals drive tool evolution.

### Suggestion Batches

Related suggestions are grouped into a **SuggestionBatch**. A batch represents a cohesive set of proposals that emerged from the same context — "everything the AI proposed about the tavern scene in Session 13" or "the three NPCs and their relationships from a P&R session about Grimhollow."

Batches are the **unit of review** in the suggestion queue. The GM can:
- Expand a batch and act on individual suggestions
- Accept or reject a batch in bulk
- Dismiss an entire batch ("that P&R conversation went nowhere")

Both SessionIngest and P&R produce batches. SessionIngest may produce multiple batches per run (one for the journal draft, one per narrative scene's entity proposals). P&R produces one batch per conversation (or per logical turn, if a single conversation covers multiple topics).

### Durability

All suggestions are **always durable** — persisted to the database the moment they're generated. There is no ephemeral suggestion state. If the GM closes the browser mid-conversation, every suggestion the AI has produced so far is still waiting when they come back.

This is a deliberate choice. The alternative — ephemeral suggestions that the GM must explicitly "save" — creates a failure mode where the AI produces valuable proposals and the GM loses them by accident. Durable-by-default means the system is safe to abandon at any point.

### Auto-Rejection Window

Suggestions that are not acted on within a configurable window (default: ~7 days) are automatically rejected. This prevents the suggestion queue from growing unboundedly when the GM ignores proposals.

The auto-rejection window is a product of the "tolerant of neglect" design principle: the system should stay useful even when the GM doesn't review everything. Stale suggestions that sit forever would undermine that — they'd clutter the queue and make the system feel burdensome. Auto-rejection keeps the queue fresh.

Auto-rejected suggestions are still part of their conversation's history. The GM can review past conversations and see what was proposed and auto-rejected.

### Accepting a Suggestion

When the GM accepts a suggestion, the system creates the corresponding real content in the campaign graph:

- `create_thing` → a new node is created with `gm_only` status (the default for all new content)
- `update_blocks` → new blocks are added to the target node with `gm_only` status
- `create_relationship` → a new edge is created with `gm_only` status
- `journal_draft` → journal blocks are created on the session

The suggestion's status is updated to `accepted`. The real content it created exists independently — editing or deleting the content later does not affect the suggestion record.

---

## Conversations

### Conversations as Provenance

Every AI interaction is an **AgentConversation** — a persisted record of what was said, what was proposed, and what decisions were made.

**Why persist conversations?** Suggestions without their originating conversation are context-free. A suggestion saying "Create NPC: Mysterious Figure" is meaningless without the conversation that explains *why* — what the GM was exploring, what context the AI was working with, what alternatives were considered. The conversation IS the provenance.

**Conversation properties:**

- **Campaign scope**: Every conversation belongs to a campaign.
- **Focal point**: What the user was looking at when they opened the agent window (session, node, campaign overview).
- **Role**: `gm`, `player`, or `system` (for SessionIngest-generated conversations).
- **Messages**: The conversation history (user messages + AI responses).
- **Suggestion references**: Which suggestions were produced during this conversation.

### Conversation Lifecycle

**Active → Resolved → Archived**

- **Active**: The conversation has unresolved suggestions (pending status). Active conversations surface prominently in the UI — they're visually demanding ("bright and annoying") because they represent outstanding decisions.
- **Resolved**: All suggestions have been acted on (accepted, rejected, dismissed, or auto-rejected). The conversation fades from prominence but remains accessible in history.
- **Archived**: Old resolved conversations. Still browsable but not surfaced.

### SessionIngest as a System Conversation

When the SessionIngest pipeline completes, the system creates a conversation with role `system`:

- **Messages** contain a summary of what was processed (audio duration, sources merged, entities detected) and what was proposed
- **Suggestions** are attached as one or more batches
- **Focal point** is the session that was processed

This conversation shows up on the session page as "the system processed your session and has proposals for you." The GM's experience is: navigate to the session, see the system's conversation, review the suggestions.

If the GM wants to refine the results, they open the agent window (a new GM-initiated conversation) with the session as focal point. The AI has access to the previous system conversation's suggestions as context — "continue from here."

### "Continue From Here"

A GM can always start a new conversation from any context, including a context where previous conversations and their suggestions already exist. The new conversation's AI has access to:

- The focal point's content (the session, the node, etc.)
- Previous conversations on this focal point and their suggestion history
- The broader campaign graph via the standard retrieval tools

This is the **escape hatch** for conversations that go nowhere. The suggestions from a dead-end conversation are still durable (dismissable in bulk), and the GM starts fresh without losing anything.

---

## Key Design Decisions

### Unified Suggestion Pipeline over Single-Executor Model

**Decision:** The API server handles interactive AI directly (P&R, Q&A). The worker handles SessionIngest batch processing. Both produce the same suggestion primitives.

**Alternative considered:** Route all AI work through the worker — interactive requests get priority queuing, the worker streams results back. This centralizes all AI execution (easier rate limiting, cost control) but adds latency to interactive requests. When a GM clicks "flesh out Grimhollow," queuing overhead is felt directly. The latency constraint for P&R ruled this out.

**Why this is right:** The worker exists because SessionIngest jobs are long-running (10+ minutes) and must survive deploys. P&R has neither constraint — it's short-lived and latency-sensitive. Forcing it through the worker solves a problem P&R doesn't have while creating one it does (latency). The shared `CampaignContext` interface and tool definitions ensure consistency without requiring a shared execution path.

### Suggestion Layer over Live Edits

**Decision:** AI changes are always proposals (suggestions). Nothing is "real" until the GM accepts it.

**Alternative considered:** AI writes directly into documents like a collaborator, with undo/revert. Faster flow, but the GM must catch and undo unwanted changes rather than proactively approving wanted ones.

**Why this is right:** The vision doc establishes "AI proposes, GM disposes" as a core principle. The suggestion layer makes this structural — the AI literally cannot modify the campaign graph without GM approval. This is especially important for SessionIngest, where the AI processes a 3-hour recording unsupervised. Live edits from a batch process would be chaotic.

### Conversations Persisted over Ephemeral

**Decision:** Agent conversations are persisted as the provenance for suggestions.

**Original position:** Conversations are ephemeral; only suggestions survive.

**Why we changed:** A durable suggestion without its originating conversation loses its "why." The GM comes back tomorrow, sees "Create NPC: Mysterious Figure" in the queue, and has no memory of the context. The conversation — what the GM was exploring, what the AI reasoned, what alternatives were discussed — is essential context for reviewing suggestions. Provenance requires persistence.

**Practical implication:** Conversations with unresolved suggestions are visually prominent. Resolved conversations fade. Auto-rejection ensures conversations don't demand attention forever.

### Q&A and P&R as Tool Availability, Not Modes

**Decision:** The agent window has no mode toggle. The AI's behavior emerges from its tool set (read-only for Q&A, read+write for P&R) and from what the user asks.

**Why this is right:** A mode toggle creates a UX burden — the GM has to decide "am I asking a question or generating content?" before they start. In practice, conversations flow naturally between the two. "Tell me about Kael" (Q&A) → "Actually, flesh out his backstory" (P&R) should be seamless, not require switching modes.

### SessionIngest as System-Initiated Conversation

**Decision:** The batch pipeline's output is framed as a conversation — as if the system said "I processed your session, here's what I found" with suggestions attached.

**Why this is right:** This gives the GM one mental model for all AI output. Whether the AI was triggered by the system (SessionIngest) or by the GM (P&R), the output arrives the same way: a conversation with proposals. The batch pipeline is effectively a "command" the system runs on the GM's behalf — analogous to a tool call in an AI coding assistant.

---

## Open Questions

These are explicitly deferred — noted for future design work, not silently assumed.

- **Multibuffer-style UI for suggestion review.** The product requires that the GM see all affected nodes simultaneously when the AI proposes cross-graph changes. The visual design and interaction model for this is a frontend design problem.

- **MCP exposure of CampaignContext.** The [audio pipeline doc](../discovery/audio_ingest/audio_overview.md) describes exposing the campaign graph as an MCP server for external AI tools. This is compatible with the design here — the same CampaignContext interface, exposed via MCP, with status filtering. Not designed in detail yet.

- **Conversation export.** The GM should be able to export a conversation to a GM-only thing for future reference — capturing reasoning, decisions, and context as a permanent part of the campaign knowledge base. The mechanism (one-click export? selective export?) is not designed yet.

- **SessionIngest pipeline stages.** The [audio pipeline doc](../discovery/audio_ingest/audio_overview.md) defines a 6-stage pipeline. This design treats the pipeline as an implementation detail behind the suggestion contract — the pipeline can be refined independently as long as its output is suggestions attached to a system conversation. The pipeline internals are not constrained by this design.

- **Auto-rejection window tuning.** The ~7 day default is a starting point. Whether this should be configurable per campaign, per suggestion type, or globally is not decided.

- **Suggestion dependencies.** When the AI proposes "Create NPC Kael" and "Kael frequents Rusty Anchor," rejecting the first makes the second invalid. Whether to model this as explicit dependencies within a batch or handle it at acceptance time (flag the conflict when the GM tries to accept the orphaned relationship) is not decided.
