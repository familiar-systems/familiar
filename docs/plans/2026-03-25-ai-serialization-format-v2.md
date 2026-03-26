# Loreweaver — AI Serialization Format & Agent Editing Model (v2)

**Status:** Draft
**Date:** 2026-03-25
**Supersedes:** AI Serialization Format v1 (undated)
**Related decisions:** [Campaign Actor Domain Design](./2026-03-25-campaign-actor-domain-design.md), [Hocuspocus Architecture ADR](../archive/plans/2026-03-14-hocuspocus-architecture.md), [AI Workflow Unification](./2026-02-14-ai-workflow-unification-design.md), [AI PRD](./2026-02-22-ai-prd.md), [Templates as Prototype Pages](./2026-02-20-templates-as-prototype-pages.md)

---

## Context

Loreweaver's AI agent operates across three workflows (SessionIngest, Planning & Refinement, Q&A) that all require reading campaign page content and — for the write workflows — modifying it. The editing medium is a rich text editor (TipTap on ProseMirror) backed by Loro CRDTs synced via the loro-dev/protocol. The campaign structure is a graph: Things (nodes), relationships (edges), and blocks (atomic content units within pages).

The agent cannot work directly with ProseMirror JSON or Loro CRDT operations — it needs a human-readable format it can reason about and edit. This document defines that format, the tool surface the agent uses to make changes, the suggestion model that governs how AI proposals interact with each other and with human editors, and the compilation pipeline that bridges agent edits back to the CRDT layer.

The core insight: **the serialization format is markdown**. The page tree is the heading hierarchy. References are wiki-links. Status annotations and graph context are the only non-markdown additions. The agent reads and writes a format that is nearly indistinguishable from what a human would write in Obsidian or Logseq.

---

## The Serialization Format

### Design Principles

1. **Markdown is the format.** The heading hierarchy (`#`, `##`, `###`) defines the page tree. Content is standard markdown. LLMs are most fluent in markdown — it is the format they are best trained to read and produce.
2. **Headings are the tree.** `## History` is a section. `### Session 3` nested inside it is a child. The tree structure users create by dragging blocks in the editor and nesting headings is the same tree the agent sees and addresses.
3. **References are wiki-links.** `{Silver Compact}` in the text is a reference to the Thing named "Silver Compact." The linker resolves names to graph nodes. The agent never sees or writes IDs.
4. **Non-markdown annotations exist only for data that isn't page content.** Graph-derived relationships, computed TOCs, and visibility status need markup. Everything else is markdown.
5. **Status tightens downward, never loosens.** A `[gm_only]` annotation on a heading applies to the entire subtree. A block inside a `[gm_only]` section cannot be `[known]`. The only valid override direction is toward restriction.

### Full Page Example (Tier 3)

```md
# Kael [known]

#NPC #Human #Villain [gm_only]

Kael is a former {Silver Compact} operative turned informant,
working out of the {Rusty Anchor} in {Northport}. He knows more
than he lets on and trusts no one.

He's still reporting to the {Silver Compact}. His defection was
staged. [gm_only]

<relationships>
@Silver Compact — formerly affiliated with | former operative
@Rusty Anchor — frequents | frequented by
@Tormund — distrusts | distrusted by
</relationships>

## Appearance

Wiry build, dark eyes that never settle. A scar runs from
his left ear to his jaw — he says it's from a bar fight.
It isn't.

## Personality

Deflects with humor. Answers questions with questions. Loyal
to whoever is paying — or so he wants people to think.

## Secrets [gm_only]

Kael is still reporting to the {Silver Compact}. His "betrayal"
was staged to place him as a mole inside {Northport}'s
intelligence network.

<prior>His handler is {Whisper}, who operates out of {Ashenmoor}.</prior>
<suggestion>His handler is {Whisper}, who operates out of {Ashenmoor},
but he's begun feeding {Whisper} false intelligence.</suggestion>

## History

### Session 0

Kael was introduced to the party by {Tormund} at the
{Rusty Anchor}.

### Session 3

Kael revealed his former ties to the {Silver Compact}
during the ambush at {Northport} docks.

### Session 7

The party discovered Kael had been feeding information
to both sides.

## GM Notes [gm_only]

Planning to have Kael betray the party in session 9. The reveal
should come through {Whisper} showing up at the {Rusty Anchor}.
```

**Note on `<prior>/<suggestion>` rendering:** The agent sees its own pending suggestions inline as `<prior>/<suggestion>` pairs. This is a serialization-time projection — the underlying representation is a mark on block UUIDs (see "Suggestion Model" below). The agent doesn't manage marks or UUIDs. It sees readable diffs of its own work.

### Format Elements

#### Page Title and Type

```md
# Kael [known]
```

The H1 is the page title. The bracketed annotation is the page's visibility status. `[known]` is the default and could be omitted, but including it makes the status unambiguous.

#### Tags

```md
#NPC #Human #Villain [gm_only]
```

Hashtag syntax, immediately after the title. Tags are graph relationships (`Kael -[tagged]-> NPC`) rendered in a compact form. Tags inherit the page's visibility status by default; individual tags can tighten with `[gm_only]`. In this example, players see `#NPC #Human` but not `#Villain`.

Tags are Things in the graph — the "NPC" tag is itself a page. Tagging is a relationship with the label `tagged`.

#### Preamble

The content between the H1 and the first structural element (`<relationships>`, `<toc>`, or the first `##` heading) is the preamble. It has no explicit tag — its position defines it.

The preamble is the most important text on the page for retrieval. It is the index card: dense with identity, role, affiliations, and what makes this entity interesting. It is the text returned at the cheapest retrieval tier. When the agent packs 20 entities into a context window for entity resolution, it packs 20 preambles.

Preamble blocks inherit the page's visibility status. Individual paragraphs can tighten:

```md
He's still reporting to the {Silver Compact}. His defection was
staged. [gm_only]
```

#### References (Wiki-Links)

```md
{Silver Compact}
```

A reference to another Thing in the campaign graph, resolved by name. The linker resolves display names to graph nodes using fuzzy/alias matching against the current graph state. If ambiguous (two Things with the same name), the linker flags it for GM review.

The agent always writes bare names. It does not see or manage IDs. Name changes are handled by the linker's alias matching — if "Yurgath Tribe" is renamed to "Yurgath Clan," the linker resolves the old name to the renamed entity.

References appear in running prose, not in a separate structure. They serve the same function as wiki-links in Obsidian or Logseq.

#### Relationships

```md
<relationships>
@Silver Compact — formerly affiliated with | former operative
@Rusty Anchor — frequents | frequented by
@Tormund — distrusts | distrusted by
</relationships>
```

Graph-derived, read-only context. Relationships are edges in the campaign graph, not page content. They appear in the serialization format so the agent can reason about the Thing's connections, but they are **not editable through the document**. The agent mutates relationships via the `propose_relationship` tool call.

The format is: `@Target — outgoing label | incoming label`. The `@` prefix distinguishes relationship targets from inline wiki-link references.

Relationships with `[gm_only]` status are excluded when serializing for a player-facing context.

#### Visibility Status

```md
## Secrets [gm_only]
```

Status annotations appear on headings (applying to the entire subtree) or on individual paragraphs (applying to that block). The inheritance rule: **status can only tighten as you descend the tree, never loosen.**

```
Page (known)
├── Preamble paragraph (known) ✓ — inherits
├── Preamble paragraph (gm_only) ✓ — tighter than parent
├── History (known)
│   ├── Session 0 (known) ✓ — inherits
│   └── Session 3 paragraph (gm_only) ✓ — tighter than parent
└── Secrets (gm_only)
    └── paragraph (gm_only) ✓ — inherits (no loosening possible)
```

A `[known]` block inside a `[gm_only]` section is a **parse error**. If the agent produces it, the compiler rejects it. If the GM wants to make one piece of a `gm_only` section visible to players, it belongs in a `known` section instead.

The `[gm_only]` annotation is the only status marker that appears in the format. `[known]` is the default and is only written on the page title for explicitness. `[retconned]` content is excluded from the serialization entirely (retrievable only on explicit request).

#### Sections

```md
## Appearance

...

## Secrets [gm_only]

...

## History

### Session 0

...

### Session 3

...
```

Sections are markdown headings. The heading hierarchy defines the page tree. Sections can nest (`### Session 0` inside `## History`). Section names must be unique within their parent — this enables path-based addressing (`History/Session 0`).

Sections are defined by the prototype page. When a Thing is created from a prototype, it clones the prototype's section structure. The GM can add, remove, or rename sections. The agent addresses sections by the heading text.

#### Pending Suggestions (Conversation-Scoped)

```md
<prior>His handler is {Whisper}, who operates out of {Ashenmoor}.</prior>
<suggestion>His handler is {Whisper}, who operates out of {Ashenmoor},
but he's begun feeding {Whisper} false intelligence.</suggestion>
```

When the serialization compiler produces markdown for a specific AgentConversation, it renders that conversation's pending suggestions as `<prior>/<suggestion>` pairs inline. The `<prior>` shows the current content of the target blocks. The `<suggestion>` shows the proposed replacement.

**Conversation scoping:** Each agent sees only its own pending suggestions. Other conversations' suggestions are invisible in the serialization — the underlying content is rendered as normal text. This means agents don't reason about each other's proposals. The deconfliction surface is the editor UI, not the agent.

**Underlying representation:** The `<prior>/<suggestion>` rendering is a serialization-time projection. The actual representation in the LoroDoc is a mark on block UUIDs with proposed replacement content as metadata (see "Suggestion Model" below). The agent never manages marks, UUIDs, or CRDT operations.

Three cases from one primitive:

- **Replace:** `<prior>old text</prior><suggestion>new text</suggestion>` — the agent proposes changing content
- **Insert:** `<prior>anchor text</prior><suggestion>anchor text\n\nnew inserted text</suggestion>` — the agent proposes adding content after (or before) existing content. The anchor text appears unchanged in both prior and suggestion; the diff reveals only the insertion.
- **Delete:** `<prior>old text</prior><suggestion></suggestion>` — the agent proposes removing content

#### Table of Contents

```md
<toc>
Appearance (120 words)
Personality (85 words)
Secrets [gm_only] (200 words)
History (350 words)
  Session 0 (80 words)
  Session 3 (120 words)
  Session 7 (150 words)
GM Notes [gm_only] (150 words)
</toc>
```

A computed summary of the page structure with word counts per section. Not editable content — generated by the serializer from the page's heading hierarchy. Appears in tier 1 and tier 2 retrievals to give the agent a `stat` of the page before deciding whether to `cat` the full content.

The indentation mirrors the heading hierarchy. Status annotations on sections are included. Word counts help the agent estimate how much context a full-page read would consume.

---

## Progressive Disclosure (Retrieval Tiers)

The serialization format supports multiple retrieval tiers. The tier selected depends on how many Things the agent needs to know about and how deeply.

### Tier 1: Index Card

Preamble + tags + relationships + TOC. Enough to understand who/what this is, how it connects, and what's on its page. ~100-150 tokens per entity.

```md
# Kael [known]

#NPC #Human #Villain [gm_only]

Kael is a former {Silver Compact} operative turned informant,
working out of the {Rusty Anchor} in {Northport}. He knows more
than he lets on and trusts no one.

He's still reporting to the {Silver Compact}. His defection was
staged. [gm_only]

<relationships>
@Silver Compact — formerly affiliated with | former operative
@Rusty Anchor — frequents | frequented by
@Tormund — distrusts | distrusted by
</relationships>

<toc>
Appearance (120 words)
Personality (85 words)
Secrets [gm_only] (200 words)
History (350 words)
  Session 0 (80 words)
  Session 3 (120 words)
  Session 7 (150 words)
GM Notes [gm_only] (150 words)
</toc>
```

**Used when:** Resolving entity references during SessionIngest (pack 12+ entities). Graph traversal at each hop. Any time the agent needs breadth over depth.

### Tier 2: Index Card + RAG Blocks

Tier 1 plus embedding-selected blocks relevant to the current query. The TOC provides structural context; the RAG blocks provide specific content without loading the full page.

```md
# Kael [known]

#NPC #Human #Villain [gm_only]

Kael is a former {Silver Compact} operative turned informant,
working out of the {Rusty Anchor} in {Northport}. He knows more
than he lets on and trusts no one.

He's still reporting to the {Silver Compact}. His defection was
staged. [gm_only]

<relationships>
@Silver Compact — formerly affiliated with | former operative
@Rusty Anchor — frequents | frequented by
@Tormund — distrusts | distrusted by
</relationships>

<toc>
Appearance (120 words)
Personality (85 words)
Secrets [gm_only] (200 words)
History (350 words)
  Session 0 (80 words)
  Session 3 (120 words)
  Session 7 (150 words)
GM Notes [gm_only] (150 words)
</toc>

> From: Secrets
> Kael is still reporting to the {Silver Compact}. His "betrayal"
> was staged to place him as a mole inside {Northport}'s
> intelligence network.

> From: History / Session 3
> Kael revealed his former ties to the {Silver Compact}
> during the ambush at {Northport} docks.
```

**Used when:** Interactive P&R where the agent needs specific context about related entities. GM asks "flesh out Kael's backstory connecting him to the Silver Compact" — agent gets tier 3 for Kael (the edit target), tier 2 for Silver Compact (relevant context).

### Tier 3: Full Page

The complete serialized page — all sections expanded, all content present. The format shown in the full page example above. Includes the requesting conversation's pending suggestions as inline `<prior>/<suggestion>` pairs.

**Used when:** The agent is actively editing a page. The agent needs deep reasoning about a single entity.

### Tier Selection Heuristics

| Scenario                            | Focal entity           | Related entities                 |
| ----------------------------------- | ---------------------- | -------------------------------- |
| SessionIngest entity resolution     | —                      | Tier 1 for all candidates        |
| SessionIngest journal drafting      | Tier 3 for the session | Tier 1-2 for referenced entities |
| P&R: "flesh out this NPC"           | Tier 3 for the target  | Tier 2 for @-referenced entities |
| P&R: "connect these two things"     | Tier 2 for both        | Tier 1 for surrounding context   |
| Q&A: "tell me about Kael"           | Tier 2-3 for Kael      | Tier 1 for connected entities    |
| Q&A: "what happened in session 14?" | Tier 3 for session 14  | Tier 1 for referenced entities   |

---

## Agent Write Tools

The agent has three write tools. **All writes produce proposals — the agent never modifies the campaign graph or page content directly.** For page content, the agent's edits become suggestion marks on blocks that the GM reviews in the editor. For graph structure, the agent's proposals go through the suggestion queue. This is the "AI proposes, GM disposes" principle made structural at every write path.

### `create_page`

Create a new proposed page from a prototype.

```
create_page(
  prototype: string,       // prototype name, e.g. "NPC"
  content: string,         // full page in serialization format
  relationships?: [{       // initial relationships, batched
    target: string,        // target Thing name
    label: string,         // outgoing label
    inverse?: string       // incoming label
  }]
)
```

The content is the full markdown for the new page, including title, tags, preamble, and sections. The prototype determines the OnCreate tag (e.g., prototype "NPC" auto-tags the new Thing as `#NPC`) and provides the section structure as a starting point.

Relationships are bundled with page creation so the agent can express "create Pip, and Pip pickpocketed Tormund" in one coherent proposal. The page and its relationships form a single reviewable unit — rejecting the page cascades to its relationships.

**Used by:** SessionIngest (proposing new entities), P&R ("create me a tavern").

### `suggest_replace`

Propose an inline edit to existing page content via string replacement.

```
suggest_replace(
  page: string,           // page name
  old_content: string,    // content to find (must be unique)
  new_content: string     // proposed replacement content
)
```

Directly inspired by Claude Code's `str_replace` tool ([open source](https://github.com/anthropics/claude-code)) — the `old_content` must match exactly and appear exactly once on the page. If not found or not unique, the tool fails and the agent retries with more context. Claude Code is a standard harness for ML evals, which means agents are already trained on this interaction pattern. We get good tool-calling behavior for free.

**This does not apply the edit.** The compiler identifies which block UUIDs contain the matched content, creates a suggestion mark on those blocks, and stores the proposed replacement as metadata. The GM sees the suggestion in the editor and accepts or rejects.

**All three mutation types — replace, insert, and delete — are handled by one tool.** No separate `suggest_insert` or `suggest_delete` is needed. The agent includes surrounding content as context in `old_content` and the full result in `new_content`:

**Replace** (content changes):

```
old_content: "His handler is Whisper, who operates out of Ashenmoor."
new_content: "His handler is Whisper, who operates out of Ashenmoor, but he's begun feeding Whisper false intelligence."
```

**Insert after existing content** (proposed is a superset of original):

```
old_content: "Kael was introduced to the party by Tormund."
new_content: "Kael was introduced to the party by Tormund.\n\nHe seemed nervous, glancing toward the door every few minutes."
```

**Insert before existing content:**

```
old_content: "## Secrets"
new_content: "## Secrets\n\nThe following is known only to the GM."
```

**Delete** (proposed is empty or a subset):

```
old_content: "This paragraph is no longer relevant to the narrative."
new_content: ""
```

The anchor content (the part of `old_content` that appears unchanged in `new_content`) is included in the suggestion's `target_blocks`. The editor's inline diff rendering compares target blocks against proposed blocks at the block level and classifies each as unchanged, modified, inserted, or deleted — showing the GM exactly what's changing and what's just context.

**Start-of-document insertion:** The page title heading (`# Kael [known]`) always exists (every page has at least a title from the prototype), so matching the title and proposing `title + new content` handles this case. A completely empty page is not a meaningful edge case.

**Used by:** P&R ("flesh out the backstory", "add a section about his childhood"), SessionIngest (proposing journal drafts, inserting new session entries into History).

### `propose_relationship`

Propose a graph edge between two existing Things.

```
propose_relationship(
  source: string,          // source Thing name
  target: string,          // target Thing name
  label: string,           // outgoing label
  inverse?: string         // incoming label
)
```

Accepts arrays for batching multiple relationships in one call. Relationships are graph-level proposals — they go through the suggestion queue, not the document editing path.

**Used by:** All write workflows. SessionIngest proposing connections between entities. P&R wiring up entities. The agent recognizing an implicit relationship in narrative content.

---

## Suggestion Model

### Suggestions are marks on block ranges

Every block in a LoroDoc has a UUID (branded as `BlockId`). A suggestion targets a contiguous list of block IDs and proposes replacement content. The original blocks remain in the document tree, unchanged. The suggestion is metadata associated with those blocks — a mark layered on top, not a structural modification.

```rust
struct Suggestion {
    id: SuggestionId,                    // branded UUID
    target_blocks: Vec<BlockId>,         // contiguous block UUIDs
    proposed_content: Vec<Block>,        // replacement blocks
    conversation_id: ConversationId,     // which agent conversation produced this
    author_user_id: UserId,              // which user's agent
    created_at: i64,
    model: String,                       // which LLM model
}
```

The suggestion metadata lives in the LoroDoc alongside the document content. The document content itself is untouched by suggestion creation.

#### Why marks, not structural replacement

The v1 design used `<prior>/<suggestion>` tagged CRDT blocks inserted into the document tree — the original content was pulled out and wrapped in a SuggestionBlock node. This had a fundamental structural problem: **creating a suggestion modified the document tree.** If suggestion A targeted paragraph P2, the compiler wrapped P2 in a SuggestionBlock. P2 no longer existed as a standalone paragraph. If suggestion B then targeted P2+P3, the string match against "P2 text\nP3 text" failed because P2 was inside a SuggestionBlock. Each suggestion restructured the tree, creating a serialization order dependency where none should exist.

The deeper issue: suggestions are annotations about content, not modifications to content. This is exactly how TipTap's comment system works — comments are marks on ranges, not structural modifications. Multiple comments can mark the same text, overlapping freely. The content stays where it is. The marks layer on top.

With marks:

- **The document tree is stable.** Creating a suggestion doesn't modify anything. A second suggestion targeting overlapping blocks finds exactly the same content.
- **Multiple suggestions coexist.** Two agents can independently propose changes to the same paragraph, or to overlapping ranges (one targets P2, another targets P2+P3), and both suggestions exist as marks on stable blocks.
- **No serialization order dependency.** It doesn't matter which suggestion was created first. Neither affects the other's ability to find its target.

### Blocking semantics

Blocks with pending suggestion marks are **read-only to human editors** in the editor UI. The GM can:

- **Accept** the suggestion — target blocks are replaced with the proposed content (new blocks get fresh UUIDs). The suggestion mark is removed.
- **Reject** the suggestion — the mark is removed. The original blocks become editable.
- **Edit the proposed replacement** — the GM can modify the suggestion's proposed content before accepting. The original blocks remain read-only.

To edit the original text, the GM rejects the suggestion. One action, clear intent.

#### Why blocking

**Blocking eliminates staleness as a concept.** Without blocking, a human could edit the text underneath a suggestion, causing the suggestion's target content to drift from what the agent reasoned about. The v1 design addressed this with render-time staleness detection — comparing `original_text` to current content. With blocking, the text under a suggestion cannot change via human editing. There is no drift. There is no race condition. There is no `original_text` field to store or compare.

The only way content under a suggestion changes is when a _different_ overlapping suggestion is accepted — which is a deliberate GM action with clear, visible consequences.

**Blocking respects "AI proposes, GM disposes."** The suggestion is a visible, active proposal that demands a decision: accept, reject, or edit the proposal. This is the right interaction model for AI-assisted creative writing where the GM is the authority.

### Conversation-scoped visibility

When the serialization compiler produces markdown for a specific AgentConversation, it includes only that conversation's pending suggestions as `<prior>/<suggestion>` pairs. Other conversations' suggestions are invisible — the underlying content is serialized as normal text.

**Agents don't reason about each other's proposals.** Agent A doesn't see agent B's suggestion. Each agent operates against a clean view of the page with only its own pending work visible.

**The deconfliction surface is the editor, not the backend.** When two agents independently target the same blocks, both marks exist. The GM sees both and reviews them independently. The backend needs no cross-conversation deconfliction logic.

**String matching operates against stable content.** Because the agent's view shows original content (not other conversations' suggestions), and because blocking prevents human edits to suggested blocks, `old_content` in `suggest_replace` will find its target reliably. The only failure case is if the agent's own earlier suggestion was accepted or rejected since the last read — correct behavior that triggers a re-read.

### Supersession rules

**Same conversation, same target blocks: supersede.** When the same AgentConversation produces a new suggestion targeting the same blocks, the new suggestion replaces the old one. The old suggestion is recorded as `superseded` in the outcomes table. Within a conversation, the user is iterating toward their intent — the latest attempt is the one that matters.

**Different conversations, same target blocks: coexist.** Proposals from different conversations are independent ideas. Both deserve review. Neither silently replaces the other. The GM sees both in the editor and accepts either one independently. Accepting one doesn't automatically reject the other, but the other's target blocks may now reference changed content — the editor flags this accordingly.

### Editor rendering

**Single suggestion on a block range:** Inline diff — strikethrough (or dim/red) for original content, highlight (or green) for proposed replacement. Accept/reject controls on the block. This is the common case and should feel like tracked changes in a word processor.

**Multiple overlapping suggestions:** The editor shifts to a UI that acknowledges competing proposals — the exact visual design (stacked diffs, tabs, sidebar) is a frontend concern. The mechanics are identical underneath: each suggestion independently marks blocks and carries proposed content.

### Suggestion lifecycle

1. **Created:** The compiler processes a `suggest_replace` tool call, identifies target block UUIDs, and sends a compiled suggestion to the ThingActor. The ThingActor applies the mark and metadata. CRDT sync broadcasts to connected editors.
2. **Pending:** Visible in the editor. Target blocks are read-only. The GM can review in context.
3. **Accepted:** Target blocks replaced with proposed content (new block UUIDs). Mark removed. Outcome recorded in `suggestion_outcomes`. Other suggestions whose target blocks overlapped with the accepted suggestion are now referencing changed/removed blocks — the editor flags them.
4. **Rejected:** Mark removed. Original blocks become editable. Outcome recorded.
5. **Superseded (same conversation only):** New suggestion from the same conversation replaces the old one on the same target blocks. Old suggestion recorded as superseded.

### Suggestion outcomes

```sql
CREATE TABLE suggestion_outcomes (
    suggestion_id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    thing_id TEXT NOT NULL,
    author_user_id TEXT NOT NULL,
    model TEXT NOT NULL,
    outcome TEXT NOT NULL,          -- 'accepted', 'rejected', 'superseded'
    resolved_by TEXT,               -- user who acted, or conversation that superseded
    resolved_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
```

**For users:** When a conversation is reopened, the conversation doc shows historical suggestions ("I proposed X"). The outcomes table decorates these with resolution status ("accepted 3 days ago by GM B"). The conversation doc is self-contained history; the outcomes table is read-time enrichment.

**For evals:** Accept/reject rates per model, per workflow, per Thing type. Time-to-resolution. Supersession rates. This is the primary signal for model selection and prompt tuning.

### Suggestions in conversation history

When an AgentConversation produces a suggestion, the full content (target blocks' current text and proposed replacement) is written into the **conversation LoroDoc** as a historical record AND sent to the ThingActor as a live suggestion. These are independent artifacts.

The conversation record is immutable history — "I proposed X." It never changes after creation. The page suggestion is a living proposal that can be accepted, rejected, or superseded. A conversation should be entirely portable and self-contained. Reopening it after hammock time should not require resolving references to find out what was suggested.

---

## The Compiler

The serialization format requires bidirectional transformation between the agent's markdown and the LoroDoc document model.

### `f()` — LoroDoc → Agent Markdown (Serialization)

Produces the agent-readable format from the page's current state.

**Inputs:** A `DocumentState` reference from the ThingActor, graph context from the RelationshipGraph actor, a retrieval tier, a role (GM vs player), and optionally a conversation ID for suggestion scoping.

```
f(
    document_state: &dyn DocumentState,
    graph_context: &GraphContext,
    tier: RetrievalTier,
    role: Role,
    conversation_id: Option<ConversationId>,
) -> String
```

**Process:**

1. Walk the LoroDoc tree, emitting markdown with heading hierarchy
2. Extract status attributes from nodes, emit `[gm_only]` annotations where status tightens
3. Resolve Thing references to display names, emit `{Name}` wiki-links
4. Query the graph for the Thing's relationships and tags, emit `<relationships>` and hashtag blocks
5. Compute TOC with word counts from the heading structure
6. Filter by role — exclude `gm_only` subtrees for player-facing serialization
7. If `conversation_id` is provided: find suggestion marks owned by that conversation, render as `<prior>/<suggestion>` pairs inline. Ignore all other conversations' suggestion marks.
8. If `conversation_id` is `None`: ignore all suggestion marks, serialize pure content. Used for non-agent contexts (export, preview).

**The compiler is a stateless service, not an actor.** It needs the ThingActor's document state AND the relationship graph AND embedding results (Tier 2) AND role context AND conversation scoping. Putting serialization on the actor would require the actor to hold references to all of these services. The compiler is a pure function with multiple inputs. The AgentConversation orchestrates: asks the ThingActor for DocumentState, asks the RelationshipGraph for context, calls the compiler.

**The compiler always reads from actors.** There is no "read from DB for cold entities" path. Every ThingActor always holds a full LoroDoc, reconstructed from relational data on startup. Spinning up a ThingActor to serve a Tier 1 index card costs one libSQL read and a few milliseconds of CPU. The actor evicts itself on idle. One read path, through actors, always.

### `f⁻¹()` — Agent Tool Call → Compiled Suggestion (Compilation)

Processes a `suggest_replace` call into a compiled suggestion ready for the ThingActor to apply.

**Inputs:** The tool call, the current serialized markdown (from `f()` with conversation scoping), and the ThingActor's document state (for block UUID resolution).

```
f_inverse(
    tool_call: SuggestReplace,
    current_page_markdown: &str,
    document_state: &dyn DocumentState,
    conversation_id: ConversationId,
) -> Result<CompiledSuggestion, CompileError>
```

**Process:**

1. Find `old_content` in `current_page_markdown` (exact match, must be unique)
2. Map the matched text range back to block UUIDs via `document_state`
3. Parse `new_content` into proposed replacement blocks
4. Return a `CompiledSuggestion` with target block IDs, proposed content, and provenance

```rust
struct CompiledSuggestion {
    target_blocks: Vec<BlockId>,
    proposed_content: Vec<Block>,
    provenance: SuggestionProvenance,
}

struct SuggestionProvenance {
    conversation_id: ConversationId,
    author_user_id: UserId,
    model: String,
}
```

The ThingActor receives this and applies it — adding the mark, storing the metadata, broadcasting via CRDT sync. The compiler produces the suggestion. The actor applies it. Clean separation.

**String match failure is the feedback mechanism.** If the content has changed since the agent last read the page (because the agent's own earlier suggestion was accepted or rejected), the string match fails. The agent gets an error, re-reads via `f()`, sees the current state, and adapts. This is the same mechanism from the v1 design — what changed is that other conversations' suggestions and human edits to non-suggested blocks no longer cause failures.

**Superseding proposals from the same conversation:** When the compiler identifies that the target blocks already have a pending suggestion from the same conversation, the `CompiledSuggestion` includes a supersession flag. The ThingActor removes the old suggestion mark (recording `superseded` in the outcomes table) and applies the new one.

**Validation:** The compiler rejects invalid status inheritance (a `[known]` block inside a `[gm_only]` section), unresolvable references (logged for GM review, not a hard failure), and structural violations (headings that don't match the prototype's expected sections — warning, not error).

### Reference Resolution (The Linker)

Wiki-link references `{Name}` are resolved by the linker at both serialization and compilation time.

**On serialization (`f()`):** Thing references in LoroDoc nodes (stored with node IDs internally) are projected to display names. The agent sees `{Silver Compact}`, not an internal ID.

**On compilation (`f⁻¹()`):** Display names in agent output are resolved back to graph nodes.

**Resolution strategy:**

1. Exact name match against the campaign graph
2. Alias/fuzzy match (handles renames — "Yurgath Tribe" resolves to the renamed "Yurgath Clan")
3. Match against proposed pages in the current batch (handles forward references to pages being created in the same SessionIngest run)
4. If ambiguous or unresolvable: flag for GM review, insert as unlinked text

This is the same entity resolution capability required for SessionIngest transcript processing. The linker is a shared component.

---

## Prototype Pages and Agent Instructions

### Prototypes Define Structure

A prototype (template) page defines the section layout, status defaults, and placeholder content for a Thing type. When a Thing is created from a prototype, the system clones the prototype's structure.

Example NPC prototype:

```md
# {Name} [known]

#NPC

{Who is this person? What's their role in the campaign?
What would you need to know if they came up unexpectedly?}

## Appearance

{What do people notice first?}

## Personality

{How do they behave? What do they want?}

## Secrets [gm_only]

{What the players don't know.}

## History

{How they got here.}

## GM Notes [gm_only]

{Running notes, plans, future hooks.}
```

### OnCreate Directives

Prototypes specify tags that are automatically applied to Things created from them. The NPC prototype has `OnCreate: tag as #NPC`. This creates a `tagged` relationship to the NPC tag-Thing at creation time.

The prototype itself is not tagged NPC — it _creates things that are tagged NPC_. This prevents prototypes from appearing in tag queries alongside actual campaign entities.

### AI Instructions Block

Prototypes include an AI instructions block that tells the agent how to work with this Thing type:

```md
## AI Instructions

OnCreate: tag as #NPC
When writing the preamble, cover identity, role, and affiliations.
The Secrets section should include motivations the players haven't
discovered. Default to gm_only for any content involving hidden
allegiances or future plot plans.
```

The AI instructions block is visible to the GM and editable. The GM controls how the agent behaves with their templates. The block is excluded from the player-facing serialization.

### Agent Instruction Stack

When the agent works with a Thing, it composes instructions from two layers:

1. **Global skills** — shipped with the product, define general capabilities. `create-or-edit-preamble.md` knows what makes a good preamble. `draft-journal-entry.md` knows how to compress a transcript into a session narrative. `propose-relationships.md` knows how to infer relationships from narrative content.

2. **Prototype AI instructions** — campaign-specific, per-Thing-type, editable by the GM. Define what this specific template needs, what sections to prioritize, what tone to use.

For Milestone 1 (single starter pack: Fantasy Generic), game-system-specific skills (D&D knowledge, Daggerheart knowledge) are part of the global skill layer. When the starter pack marketplace exists, system-specific skills become a third layer between global and prototype instructions.

The most specific layer (prototype instructions) overrides the most general (global skills) when they conflict.

---

## Session Pages

Session pages follow the same serialization format but serve a different purpose. They are temporal (one per session) and are both the input to and output of SessionIngest.

### Session Template

```md
# Session 14: The Northport Docks [known]

{What happened this session in 2-3 sentences. Written by AI
after processing, editable by GM. This becomes the session's
index card for future retrieval.}

<relationships>
@The Silver Compact (arc)
</relationships>

<toc>
...
</toc>

## Journal

{The narrative of what happened. AI-drafted from transcript,
GM-reviewed. The canonical record.}

## Prep Notes [gm_only]

{What the GM planned before the session. Valuable for
post-session diffing — what happened vs. what was planned.}

## Sources [gm_only]

{Transcript segments, player recollections, GM notes.
The raw material the journal was derived from.}
```

### Session Preamble Quality

The session preamble is the single most important piece of text for retrieval across the campaign timeline. When the agent needs to understand what happened across 20 sessions, it pulls 20 session preambles. The global `draft-journal-entry.md` skill should include specific instructions for session preamble quality: cover key events, name entities involved, note major state changes (deaths, alliances, revelations), two to three sentences, dense and factual.

---

## Key Design Decisions

### Markdown over Custom DSL

**Decision:** The serialization format is standard markdown with minimal annotations, not a custom DSL with XML-like block wrappers.

**Why:** LLMs are most fluent in markdown. Users already think in markdown. The heading hierarchy provides tree structure, path-based addressing, and splice anchoring without any custom syntax. The fewer non-markdown elements in the format, the less parsing the compiler needs and the fewer opportunities for the LLM to produce invalid output.

### Wiki-Links without IDs

**Decision:** References are bare display names (`{Silver Compact}`), resolved by a linker. The agent never sees or manages entity IDs.

**Why:** IDs are noise in the agent's context window and a source of transcription errors in LLM output. Name-based resolution is already a required capability (SessionIngest entity resolution). The linker handles renames via alias/fuzzy matching. Ambiguity (two Things with the same name) is rare and flagged for GM review — a data quality signal, not a system failure.

### Relationships as Read Context, Mutated via Tools

**Decision:** Relationships appear in the serialization format as read context. The agent modifies relationships through `propose_relationship` tool calls, not by editing the document.

**Why:** Relationships are graph structure, not page content. They live in the graph database and go through the suggestion queue. Embedding them in the editable document surface would require the compiler to parse relationship changes out of the markdown and route them through a different write path than content changes. Keeping them read-only in the format and editable via tool calls respects the architectural boundary between page content (LoroDoc/CRDT) and graph structure (libSQL/suggestion queue).

### Marks on Blocks, Not Structural Replacement

**Decision:** Suggestions are marks on block UUID ranges with proposed content as metadata. The original blocks remain in the document tree unchanged.

**Why:** Structural replacement (the v1 design) modified the document tree when a suggestion was created. This created serialization order dependencies — the first suggestion changed the tree so subsequent suggestions targeting overlapping content couldn't find their target. Marks don't modify the tree. Multiple suggestions coexist on stable blocks. The pattern follows TipTap's comment thread model, which is proven at scale.

### Blocking over Staleness Detection

**Decision:** Blocks under pending suggestions are read-only to human editors. No staleness concept exists.

**Why:** Staleness detection (comparing original text to current text at render time) was necessary in the v1 design because humans could edit text underneath suggestions. With blocking, the text under a suggestion cannot change. There is no drift. No race conditions. No `original_text` field to store or compare. The escape hatch is one action: reject the suggestion.

### Conversation-Scoped Agent Views

**Decision:** Each agent sees only its own pending suggestions. Other conversations' suggestions are invisible in the serialization.

**Why:** Agents don't need to reason about each other's proposals — that's the GM's job. Conversation scoping means the string-match target is always the original content (for blocks without this conversation's suggestions) or the agent's own prior proposal (for blocks with this conversation's suggestions). The agent's view is stable and predictable. Cross-conversation deconfliction happens in the editor UI, where the GM has full context.

### Supersession Within, Coexistence Across

**Decision:** Same-conversation suggestions targeting the same blocks supersede. Different-conversation suggestions coexist.

**Why:** Within a conversation, the user is iterating — "try again, make it darker." The latest attempt represents their current intent. Across conversations, proposals are independent ideas from potentially different users. Both deserve review. Superseding across conversations would silently discard one user's agent's work, which violates the expectation that each user's agent works independently.

### Suggestion Duplication over Cross-Reference

**Decision:** Suggestions are duplicated — one copy as history in the conversation LoroDoc, one as a live mark on the Thing's LoroDoc.

**Why:** A conversation should be entirely self-contained and portable. It's a document. Reopening it after days should not require resolving foreign keys or joining ThingActor rooms to discover what was suggested. The active suggestion on the page has its own lifecycle (accepted, rejected, superseded) that the conversation doesn't need to track structurally. The outcomes table provides status decoration at read time for users who want it, and eval signal for measuring agent quality.

### Status Tightens Downward Only

**Decision:** `[gm_only]` inside `[known]` is valid. `[known]` inside `[gm_only]` is a parse error. Status can only tighten as you descend the heading hierarchy.

**Why:** A `gm_only` section with a `known` child is a locked room with an open window — the classification on the container is meaningless. This constraint simplifies the player-facing filter (prune `gm_only` subtrees, no need to check children for overrides) and eliminates a class of accidental information leaks.

### Tags as Hashtags

**Decision:** Tags render as `#NPC #Human #Villain [gm_only]` using hashtag syntax.

**Why:** Universally understood (Obsidian, Logseq, social media). Visually distinct from narrative relationships. Tags are graph relationships (`tagged` edges to tag-Things) but serve a different purpose than narrative edges — the hashtag syntax makes this distinction visible.

### Preamble as Implicit Position

**Decision:** The preamble is the content between the H1 and the first structural element. No explicit tag marks it.

**Why:** Every markdown document already has this concept. Adding an explicit `<preamble>` wrapper would be redundant and is one more non-markdown element for the LLM to manage.

---

## Open Questions

- **Loro mark/annotation primitives.** Whether Loro natively supports range annotations (like ProseMirror marks) or whether suggestion marks are LoroMap entries keyed by suggestion ID with block UUID lists is a Loro/TipTap spike question. The model is the same either way; the storage representation differs.

- **TipTap extension design.** The editor needs a custom extension for suggestion rendering — inline diff for single suggestions, multi-proposal view for overlapping suggestions, accept/reject controls, read-only enforcement on target blocks. This is a frontend design concern informed by but not determined by this document.

- **Proposal block rendering details.** The visual design of inline diffs (tracked-changes style, side-by-side, or something else) and the multi-suggestion UI (tabs, stacked diffs, carousel) are frontend decisions. The backend provides: which blocks are suggested, what the proposed replacement is, who proposed it, and whether other suggestions overlap.

- **Proposed page visibility in other pages' serialization.** When SessionIngest creates a proposed NPC, should that NPC appear in other pages' `<relationships>` blocks and RAG results? Likely yes with a `[proposed]` annotation, but the lifecycle and cleanup model needs design.

- **Suggestion expiry.** The mark model needs an expiry mechanism — either a TTL checked at render time, or a periodic sweep by the ThingActor. The outcomes table should record `expired` as an outcome.

- **Bulk review UX.** After SessionIngest produces many suggestions across a page, is there a "review mode" that walks through suggestions sequentially? The backend needs to support whatever bulk operations the UX requires (the SuggestionTarget trait may need batch methods).

- **Starter pack marketplace.** The "Town Market" — a communal free marketplace for user-submitted starter packs. Deferred to Milestone 2+.

- **Preamble quality feedback loop.** Measuring whether GM-authored preambles improve over time and whether retrieval quality correlates with preamble quality requires the eval framework (Milestone 3).
