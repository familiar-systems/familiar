# familiar.systems - AI Serialization Format & Agent Editing Model (v2)

**Status:** Draft
**Date:** 2026-03-25
**Supersedes:** AI Serialization Format v1 (undated)
**Related decisions:** [Campaign Actor Domain Design](./2026-05-04-campaign-actor-domain-design.md), [Hocuspocus Architecture ADR](../archive/plans/2026-03-14-hocuspocus-architecture.md), [AI Workflow Unification](./2026-02-14-ai-workflow-unification-design.md), [AI PRD](./2026-02-22-ai-prd.md), [Templates](./2026-06-29-templates.md)

---

## Context

familiar.systems's AI agent operates across three workflows (SessionIngest, Planning & Refinement, Q&A) that all require reading campaign page content and - for the write workflows - modifying it. The editing medium is a rich text editor (TipTap on ProseMirror) backed by Loro CRDTs synced via the loro-dev/protocol. The campaign structure is a graph: Pages (nodes), relationships (edges), and blocks (atomic content units within pages).

The agent cannot work directly with ProseMirror JSON or Loro CRDT operations - it needs a human-readable format it can reason about and edit. This document defines that format, the tool surface the agent uses to make changes, the suggestion model that governs how AI proposals interact with each other and with human editors, and the compilation pipeline that bridges agent edits back to the CRDT layer.

The core insight: **the serialization format is markdown**. The page tree is the heading hierarchy. References are wiki-links. Status annotations and graph context are the only non-markdown additions. The agent reads and writes a format that is nearly indistinguishable from what a human would write in Obsidian or Logseq.

> **Page identity lives in frontmatter; visibility serializes as spans.** The title, the page's `status`, and extensible identity fields (e.g. `aliases`) live in YAML frontmatter; the body uses native heading levels (`#` is the top section). In-flow content carries visibility as `<player_visible>` / `<gm_only>` spans (see _Visibility Status_), one status per block with nothing inherited down the heading tree. The authoring-side mirror is [Templates](2026-06-29-templates.md).

---

## The Serialization Format

### Design Principles

1. **Markdown is the format.** The heading hierarchy (`#`, `##`, `###`) defines the page tree. Content is standard markdown. LLMs are most fluent in markdown - it is the format they are best trained to read and produce.
2. **Headings are the tree.** `# History` is a section. `## Session 3` nested inside it is a child. The page title is not a heading; it is frontmatter (see _Page Identity_ below). The tree structure users create by dragging blocks in the editor and nesting headings is the same tree the agent sees and addresses.
3. **References are wiki-links.** `{Silver Compact}` in the text is a reference to the Page named "Silver Compact." The linker resolves names to graph nodes. The agent never sees or writes IDs.
4. **Non-markdown annotations exist only for data that isn't page content.** Graph-derived relationships, computed TOCs, and visibility status need markup. Everything else is markdown.
5. **Visibility is literal per block, fail-closed.** Each block carries its own status; nothing inherits down the heading tree. A block is player-visible only if its own status is `player_visible`; the stored default is `gm_only`, so a forgotten reveal hides rather than leaks. In the markdown this serializes as `<player_visible>` / `<gm_only>` spans, run-length encoding over per-block status, not a scope (see _Visibility Status_).

### Full Page Example (Tier 3)

```md
---
name: Kael
status: player_visible
aliases: [The Quartermaster]
---

#NPC #Human <gm_only>#Villain</gm_only>

<player_visible>
Kael is a former {Silver Compact} operative turned informant,
working out of the {Rusty Anchor} in {Northport}. He knows more
than he lets on and trusts no one.
</player_visible>

<gm_only>
He's still reporting to the {Silver Compact}. His defection was
staged.
</gm_only>

<relationships>
@Silver Compact - formerly affiliated with | former operative
@Rusty Anchor - frequents | frequented by
@Tormund - distrusts | distrusted by
</relationships>

<player_visible>
# Appearance

Wiry build, dark eyes that never settle. A scar runs from
his left ear to his jaw - he says it's from a bar fight.
It isn't.

# Personality

Deflects with humor. Answers questions with questions. Loyal
to whoever is paying - or so he wants people to think.
</player_visible>

<gm_only>
# Secrets

Kael is still reporting to the {Silver Compact}. His "betrayal"
was staged to place him as a mole inside {Northport}'s
intelligence network.

<prior>His handler is {Whisper}, who operates out of {Ashenmoor}.</prior>
<suggestion>His handler is {Whisper}, who operates out of {Ashenmoor},
but he's begun feeding {Whisper} false intelligence.</suggestion>
</gm_only>

<player_visible>
# History

## Session 0

Kael was introduced to the party by {Tormund} at the
{Rusty Anchor}.

## Session 3

Kael revealed his former ties to the {Silver Compact}
during the ambush at {Northport} docks.

## Session 7

The party discovered Kael had been feeding information
to both sides.
</player_visible>

<gm_only>
# GM Notes

Planning to have Kael betray the party in session 9. The reveal
should come through {Whisper} showing up at the {Rusty Anchor}.
</gm_only>
```

**Note on `<prior>/<suggestion>` rendering:** The agent sees its own pending suggestions inline as `<prior>/<suggestion>` pairs. This is a serialization-time projection - the underlying representation is a mark on block UUIDs (see "Suggestion Model" below). The agent doesn't manage marks or UUIDs. It sees readable diffs of its own work.

### Format Elements

#### Page Identity (Frontmatter)

```md
---
name: Kael
status: player_visible
aliases: [The Quartermaster]
---
```

The page's identity lives in YAML frontmatter, not in the body: `name` (the title, stored as `meta.title`), the page's visibility `status`, and extensible identity fields like `aliases` (alternate names the linker resolves against). The title is a field, never a heading, so the body owns the full heading range and `#` is the top section.

`status` is the page node's own visibility. The stored default is `gm_only` (fail closed: a newly created page is hidden until the GM reveals it); `status: player_visible` reveals the page to players. It is the node's own status and nothing more: it neither reveals nor hides anything in the body, because every in-flow block carries its own status independently and nothing inherits down the heading tree. Status sits in frontmatter because the page root annotates no content positioned in the flow; in-flow content carries its visibility through `<player_visible>` / `<gm_only>` spans instead.

Frontmatter is a typed, extensible identity block. The discipline that keeps it from becoming a junk drawer: it holds page facets with no per-item visibility that are not content. Anything that can be secret per item (tags, a concealed alias) stays inline under a visibility span.

#### Tags

```md
#NPC #Human <gm_only>#Villain</gm_only>
```

Hashtag syntax, immediately after the frontmatter, before the preamble. Tags are graph relationships (`Kael -[tagged]-> NPC`) rendered in a compact form, each carrying its own visibility (the `tagged` relationship's status). Hidden tags are wrapped in a `<gm_only>` span; the rest are player-visible. In this example players see `#NPC #Human` but not `#Villain`, and the player-facing serialization drops the wrapped tag entirely.

Tags are Pages in the graph - the "NPC" tag is itself a page. Tagging is a relationship with the label `tagged`.

#### Preamble

The content between the frontmatter (and the tags line) and the first structural element (`<relationships>`, `<toc>`, or the first `#` heading) is the preamble. It has no explicit tag - its position defines it.

The preamble is the most important text on the page for retrieval. It is the index card: dense with identity, role, affiliations, and what makes this entity interesting. It is the text returned at the cheapest retrieval tier. When the agent packs 20 entities into a context window for entity resolution, it packs 20 preambles.

Preamble blocks are hidden by default. The player-facing index-card paragraph is wrapped in a `<player_visible>` span; a secret one is simply left unwrapped:

```md
<player_visible>
Kael is a former {Silver Compact} operative turned informant.
</player_visible>

He's still reporting to the {Silver Compact}. His defection was staged.
```

The preamble is backed by its own storage container and is **AI-authored by default**, kept current by the maintenance pipeline in [Multi-Section Document Structure](2026-06-07-multi-section-document-structure.md#preamble-maintenance). Its position still defines it in this format (see *Preamble as Implicit Position*); the storage container does not change the markdown.

#### References (Wiki-Links)

```md
{Silver Compact}
```

A reference to another Page in the campaign graph, resolved by name. The linker resolves display names to graph nodes using fuzzy/alias matching against the current graph state. If ambiguous (two Pages with the same name), the linker flags it for GM review.

The agent always writes bare names. It does not see or manage IDs. Name changes are handled by the linker's alias matching - if "Yurgath Tribe" is renamed to "Yurgath Clan," the linker resolves the old name to the renamed entity.

References appear in running prose, not in a separate structure. They serve the same function as wiki-links in Obsidian or Logseq.

#### Relationships

```md
<relationships>
@Silver Compact - formerly affiliated with | former operative
@Rusty Anchor - frequents | frequented by
@Tormund - distrusts | distrusted by
</relationships>
```

Graph-derived, read-only context. Relationships are edges in the campaign graph, not page content. They appear in the serialization format so the agent can reason about the Page's connections, but they are **not editable through the document**. The agent mutates relationships via the `propose_relationship` tool call.

The format is: `@Target - outgoing label | incoming label`. The `@` prefix distinguishes relationship targets from inline wiki-link references.

Relationships with `[gm_only]` status are excluded when serializing for a player-facing context.

#### Visibility Status

Visibility is **literal per block**. Each block stores its own status, and that status is the whole truth: a block is player-visible only if its own status is `player_visible`. **Nothing inherits.** A heading marked `gm_only` does not hide the blocks beneath it; a heading marked `player_visible` does not reveal them. The stored default is `gm_only`, so a block with no reveal is hidden. `retconned` content is excluded from the serialization entirely (retrievable only on explicit request).

In the markdown, visibility serializes as XML-like spans wrapping a contiguous run of equal-status blocks:

```md
<player_visible>
# Appearance

Wiry build, dark eyes that never settle.
</player_visible>

<gm_only>
# Secrets

His defection was staged.
</gm_only>
```

A span is **run-length encoding over per-block status, not a scope.** The compiler coalesces a maximal run of equal-status blocks into one span on the way out and expands it back to per-block status on the way in. The span lives only in the markdown; storage is always one status per block, and a block added later does not "fall into" a surrounding span. Treating a span as an inherited scope would silently rebuild a cascade, which is exactly what the literal model rejects.

Spans beat a per-block suffix for an LLM: the boundary is a single attention target, and a block's status is decodable from its enclosing tag rather than inferred from the absence of a mark (the negative inference models do worst under long context). Labeling differs by surface, on the principle "deltas where a mistake fails safe, total labeling where a wrong inference is costly":

- **Agent read and write (`f` / `f⁻¹`):** total labeling. Every region is wrapped, so the agent never infers a secret from the absence of a tag.
- **Human authoring** (on-disk templates): `<player_visible>` deltas only; bare gaps default `gm_only`. A forgotten wrap hides, never leaks. Owned by [Templates](2026-06-29-templates.md).
- **Player-facing serialization:** the role filter in `f()` keeps only `player_visible` blocks and strips the tags, emitting clean content. Nothing to mislabel, so the player view is structurally leak-proof. The same page thus projects two cards: player-facing RAG packs the `player_visible` subset, GM-facing RAG packs all of it.

Visibility is **co-authored with content**: a single `suggest_replace` carries each block's visibility alongside its prose, because secrecy is part of what the content means. An agent that writes a block without wrapping it has produced `gm_only` content by default.

Why fail-closed: the dangerous direction is a secret going public, so the safe state must be the default. A forgotten reveal leaves a block `gm_only`; the inverse would leak. And because the model is literal, a GM who reveals a fact sees exactly that fact revealed, with no ancestor that can silently swallow the reveal and surface the omission days later.

#### Sections

```md
# Appearance

...

# Secrets

...

# History

## Session 0

...

## Session 3

...
```

Sections are markdown headings. The heading hierarchy defines the page tree. Sections can nest (`## Session 0` inside `# History`). Section names must be unique within their parent - this enables path-based addressing (`History/Session 0`).

The heading hierarchy is the page's structure for navigation and addressing, independent of visibility (which is per block; see _Visibility Status_). When a Page is created from a template, it clones the template's heading structure. The GM can add, remove, or rename headings within the body. The agent addresses them by heading text.

#### Pending Suggestions (Conversation-Scoped)

```md
<prior>His handler is {Whisper}, who operates out of {Ashenmoor}.</prior>
<suggestion>His handler is {Whisper}, who operates out of {Ashenmoor},
but he's begun feeding {Whisper} false intelligence.</suggestion>
```

When the serialization compiler produces markdown for a specific AgentConversation, it renders that conversation's pending suggestions as `<prior>/<suggestion>` pairs inline. The `<prior>` shows the current content of the target blocks. The `<suggestion>` shows the proposed replacement.

**Conversation scoping:** Each agent sees only its own pending suggestions. Other conversations' suggestions are invisible in the serialization - the underlying content is rendered as normal text. This means agents don't reason about each other's proposals. The deconfliction surface is the editor UI, not the agent.

**Underlying representation:** The `<prior>/<suggestion>` rendering is a serialization-time projection. The actual representation in the LoroDoc is a mark on block UUIDs with proposed replacement content as metadata (see "Suggestion Model" below). The agent never manages marks, UUIDs, or CRDT operations.

Three cases from one primitive:

- **Replace:** `<prior>old text</prior><suggestion>new text</suggestion>` - the agent proposes changing content
- **Insert:** `<prior>anchor text</prior><suggestion>anchor text\n\nnew inserted text</suggestion>` - the agent proposes adding content after (or before) existing content. The anchor text appears unchanged in both prior and suggestion; the diff reveals only the insertion.
- **Delete:** `<prior>old text</prior><suggestion></suggestion>` - the agent proposes removing content

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

A computed summary of the page structure with word counts per section. Not editable content - generated by the serializer from the page's heading hierarchy. Appears in tier 1 and tier 2 retrievals to give the agent a `stat` of the page before deciding whether to `cat` the full content.

The indentation mirrors the heading hierarchy. Status annotations on sections are included. Word counts help the agent estimate how much context a full-page read would consume.

---

## Progressive Disclosure (Retrieval Tiers)

The serialization format supports multiple retrieval tiers. The tier selected depends on how many Pages the agent needs to know about and how deeply.

### Tier 1: Index Card

Preamble + tags + relationships + TOC. Enough to understand who/what this is, how it connects, and what's on its page. ~100-150 tokens per entity.

```md
---
name: Kael
status: player_visible
---

#NPC #Human <gm_only>#Villain</gm_only>

<player_visible>
Kael is a former {Silver Compact} operative turned informant,
working out of the {Rusty Anchor} in {Northport}. He knows more
than he lets on and trusts no one.
</player_visible>

<gm_only>
He's still reporting to the {Silver Compact}. His defection was
staged.
</gm_only>

<relationships>
@Silver Compact - formerly affiliated with | former operative
@Rusty Anchor - frequents | frequented by
@Tormund - distrusts | distrusted by
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

Tier 1 plus embedding-selected blocks relevant to the current query. The TOC provides structural context; the RAG blocks provide specific content without loading the full page. The assembled pack is `meta + preamble + relationships + TOC + matched blocks in their TOC position`.

```md
---
name: Kael
status: player_visible
---

#NPC #Human <gm_only>#Villain</gm_only>

<player_visible>
Kael is a former {Silver Compact} operative turned informant,
working out of the {Rusty Anchor} in {Northport}. He knows more
than he lets on and trusts no one.
</player_visible>

<gm_only>
He's still reporting to the {Silver Compact}. His defection was
staged.
</gm_only>

<relationships>
@Silver Compact - formerly affiliated with | former operative
@Rusty Anchor - frequents | frequented by
@Tormund - distrusts | distrusted by
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

**Used when:** Interactive P&R where the agent needs specific context about related entities. GM asks "flesh out Kael's backstory connecting him to the Silver Compact" - agent gets tier 3 for Kael (the edit target), tier 2 for Silver Compact (relevant context).

### Tier 3: Full Page

The complete serialized page - all sections expanded, all content present. The format shown in the full page example above. Includes the requesting conversation's pending suggestions as inline `<prior>/<suggestion>` pairs.

**Used when:** The agent is actively editing a page. The agent needs deep reasoning about a single entity.

### Tier Selection Heuristics

| Scenario                            | Focal entity           | Related entities                 |
| ----------------------------------- | ---------------------- | -------------------------------- |
| SessionIngest entity resolution     | -                      | Tier 1 for all candidates        |
| SessionIngest journal drafting      | Tier 3 for the session | Tier 1-2 for referenced entities |
| P&R: "flesh out this NPC"           | Tier 3 for the target  | Tier 2 for @-referenced entities |
| P&R: "connect these two things"     | Tier 2 for both        | Tier 1 for surrounding context   |
| Q&A: "tell me about Kael"           | Tier 2-3 for Kael      | Tier 1 for connected entities    |
| Q&A: "what happened in session 14?" | Tier 3 for session 14  | Tier 1 for referenced entities   |

---

## Agent Write Tools

The agent has three write tools. **All writes produce proposals - the agent never modifies the campaign graph or page content directly.** For page content, the agent's edits become suggestion marks on blocks that the GM reviews in the editor. For graph structure, the agent's proposals go through the suggestion queue. This is the "AI proposes, GM disposes" principle made structural at every write path.

### `create_page`

Create a new proposed page from a template.

```
create_page(
  template: string,       // template name, e.g. "NPC"
  content: string,         // full page in serialization format
  relationships?: [{       // initial relationships, batched
    target: string,        // target Page name
    label: string,         // outgoing label
    inverse?: string       // incoming label
  }]
)
```

The content is the full markdown for the new page, including title, tags, preamble, and sections. The template determines the OnCreate tag (e.g., template "NPC" auto-tags the new Page as `#NPC`) and provides the section structure as a starting point.

Relationships are bundled with page creation so the agent can express "create Pip, and Pip pickpocketed Tormund" in one coherent proposal. The page and its relationships form a single reviewable unit - rejecting the page cascades to its relationships.

**Used by:** SessionIngest (proposing new entities), P&R ("create me a tavern").

### `suggest_replace`

Propose an inline edit to existing page content via string replacement.

```
suggest_replace(
  page: string,           // page name
  old_content: string,    // content to find (must be unique)
  new_content: string,    // proposed replacement content
  reason: string          // why this change; shown to the GM with the suggestion
)
```

Directly inspired by Claude Code's `str_replace` tool ([open source](https://github.com/anthropics/claude-code)) - the `old_content` must match exactly and appear exactly once on the page. If not found or not unique, the tool fails and the agent retries with more context. Claude Code is a standard harness for ML evals, which means agents are already trained on this interaction pattern. We get good tool-calling behavior for free.

**Precondition (full read).** `suggest_replace` requires the agent to have read the page in full this turn (Tier 3). This is the drift harness: the agent always proposes against the current whole page, so a stale `old_content` fails the match instead of corrupting the document.

**Return value.** On success the tool returns the registered *proposal*, not an applied edit. The page is unchanged until the GM accepts; the conversation's own later reads render its pending suggestion inline (per conversation scoping). A failed or ambiguous match returns an error, and the agent re-reads and retries.

**This does not apply the edit.** The compiler identifies which block UUIDs contain the matched content, creates a suggestion mark on those blocks, and stores the proposed replacement as metadata. The GM sees the suggestion in the editor and accepts or rejects.

**All three mutation types - replace, insert, and delete - are handled by one tool.** No separate `suggest_insert` or `suggest_delete` is needed. The agent includes surrounding content as context in `old_content` and the full result in `new_content`:

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

The anchor content (the part of `old_content` that appears unchanged in `new_content`) is included in the suggestion's `target_blocks`. The editor's inline diff rendering compares target blocks against proposed blocks at the block level and classifies each as unchanged, modified, inserted, or deleted - showing the GM exactly what's changing and what's just context.

**Start-of-document insertion:** Every page has a non-empty preamble (at least the index-card block from the template), so matching an existing leading block and proposing `block + new content` handles insertion at the top. A completely empty page is not a meaningful edge case.

**Used by:** P&R ("flesh out the backstory", "add a section about his childhood"), SessionIngest (proposing journal drafts, inserting new session entries into History).

### `propose_relationship`

Propose a graph edge between two existing Pages.

```
propose_relationship(
  source: string,          // source Page name
  target: string,          // target Page name
  label: string,           // outgoing label
  inverse?: string         // incoming label
)
```

Accepts arrays for batching multiple relationships in one call. Relationships are graph-level proposals - they go through the suggestion queue, not the document editing path.

**Used by:** All write workflows. SessionIngest proposing connections between entities. P&R wiring up entities. The agent recognizing an implicit relationship in narrative content.

---

## Suggestion Model

### Suggestions are marks on block ranges

Every block in a LoroDoc has a UUID (branded as `BlockId`). A suggestion targets a contiguous list of block IDs and proposes replacement content. The original blocks remain in the document tree, unchanged. The suggestion is metadata associated with those blocks - a mark layered on top, not a structural modification.

```rust
struct Suggestion {
    id: SuggestionId,                    // branded UUID
    target_blocks: Vec<BlockId>,         // contiguous block UUIDs
    proposed_content: Vec<Block>,        // replacement blocks
    conversation_id: ConversationId,     // which agent conversation produced this
    author_user_id: UserId,              // which user's agent
    created_at: i64,
    model: String,                       // which LLM model
    reason: String,                      // human-readable justification, shown to the GM
}
```

The suggestion metadata lives in the LoroDoc alongside the document content. The document content itself is untouched by suggestion creation.

#### Why marks, not structural replacement

The v1 design used `<prior>/<suggestion>` tagged CRDT blocks inserted into the document tree - the original content was pulled out and wrapped in a SuggestionBlock node. This had a fundamental structural problem: **creating a suggestion modified the document tree.** If suggestion A targeted paragraph P2, the compiler wrapped P2 in a SuggestionBlock. P2 no longer existed as a standalone paragraph. If suggestion B then targeted P2+P3, the string match against "P2 text\nP3 text" failed because P2 was inside a SuggestionBlock. Each suggestion restructured the tree, creating a serialization order dependency where none should exist.

The deeper issue: suggestions are annotations about content, not modifications to content. This is exactly how TipTap's comment system works - comments are marks on ranges, not structural modifications. Multiple comments can mark the same text, overlapping freely. The content stays where it is. The marks layer on top.

With marks:

- **The document tree is stable.** Creating a suggestion doesn't modify anything. A second suggestion targeting overlapping blocks finds exactly the same content.
- **Multiple suggestions coexist.** Two agents can independently propose changes to the same paragraph, or to overlapping ranges (one targets P2, another targets P2+P3), and both suggestions exist as marks on stable blocks.
- **No serialization order dependency.** It doesn't matter which suggestion was created first. Neither affects the other's ability to find its target.

### Blocking semantics

Blocks with pending suggestion marks are **read-only to human editors** in the editor UI. The GM can:

- **Accept** the suggestion - target blocks are replaced with the proposed content (new blocks get fresh UUIDs). The suggestion mark is removed.
- **Reject** the suggestion - the mark is removed. The original blocks become editable.
- **Edit the proposed replacement** - the GM can modify the suggestion's proposed content before accepting. The original blocks remain read-only.

To edit the original text, the GM rejects the suggestion. One action, clear intent.

#### Why blocking

**Blocking eliminates staleness as a concept.** Without blocking, a human could edit the text underneath a suggestion, causing the suggestion's target content to drift from what the agent reasoned about. The v1 design addressed this with render-time staleness detection - comparing `original_text` to current content. With blocking, the text under a suggestion cannot change via human editing. There is no drift. There is no race condition. There is no `original_text` field to store or compare.

The only way content under a suggestion changes is when a _different_ overlapping suggestion is accepted - which is a deliberate GM action with clear, visible consequences.

**Blocking respects "AI proposes, GM disposes."** The suggestion is a visible, active proposal that demands a decision: accept, reject, or edit the proposal. This is the right interaction model for AI-assisted creative writing where the GM is the authority.

### Conversation-scoped visibility

When the serialization compiler produces markdown for a specific AgentConversation, it includes only that conversation's pending suggestions as `<prior>/<suggestion>` pairs. Other conversations' suggestions are invisible - the underlying content is serialized as normal text.

**Agents don't reason about each other's proposals.** Agent A doesn't see agent B's suggestion. Each agent operates against a clean view of the page with only its own pending work visible.

**The deconfliction surface is the editor, not the backend.** When two agents independently target the same blocks, both marks exist. The GM sees both and reviews them independently. The backend needs no cross-conversation deconfliction logic.

**String matching operates against stable content.** Because the agent's view shows original content (not other conversations' suggestions), and because blocking prevents human edits to suggested blocks, `old_content` in `suggest_replace` will find its target reliably. The only failure case is if the agent's own earlier suggestion was accepted or rejected since the last read - correct behavior that triggers a re-read.

### Supersession rules

**Same conversation, same target blocks: supersede.** When the same AgentConversation produces a new suggestion targeting the same blocks, the new suggestion replaces the old one. The old suggestion is recorded as `superseded` in the outcomes table. Within a conversation, the user is iterating toward their intent - the latest attempt is the one that matters.

**Different conversations, same target blocks: coexist.** Proposals from different conversations are independent ideas. Both deserve review. Neither silently replaces the other. The GM sees both in the editor and accepts either one independently. Accepting one doesn't automatically reject the other, but the other's target blocks may now reference changed content - the editor flags this accordingly.

### Editor rendering

**Single suggestion on a block range:** Inline diff - strikethrough (or dim/red) for original content, highlight (or green) for proposed replacement. Accept/reject controls on the block. This is the common case and should feel like tracked changes in a word processor.

**Multiple overlapping suggestions:** The editor shifts to a UI that acknowledges competing proposals - the exact visual design (stacked diffs, tabs, sidebar) is a frontend concern. The mechanics are identical underneath: each suggestion independently marks blocks and carries proposed content.

### Suggestion lifecycle

1. **Created:** The compiler processes a `suggest_replace` tool call, identifies target block UUIDs, and sends a compiled suggestion to the PageActor. The PageActor applies the mark and metadata. CRDT sync broadcasts to connected editors.
2. **Pending:** Visible in the editor. Target blocks are read-only. The GM can review in context.
3. **Accepted:** Target blocks replaced with proposed content (new block UUIDs). Mark removed. Outcome recorded in `suggestion_outcomes`. Other suggestions whose target blocks overlapped with the accepted suggestion are now referencing changed/removed blocks - the editor flags them.
4. **Rejected:** Mark removed. Original blocks become editable. Outcome recorded.
5. **Superseded (same conversation only):** New suggestion from the same conversation replaces the old one on the same target blocks. Old suggestion recorded as superseded.

### Suggestion outcomes

```sql
CREATE TABLE suggestion_outcomes (
    suggestion_id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    page_id TEXT NOT NULL,
    author_user_id TEXT NOT NULL,
    model TEXT NOT NULL,
    outcome TEXT NOT NULL,          -- 'accepted', 'rejected', 'superseded'
    resolved_by TEXT,               -- user who acted, or conversation that superseded
    resolved_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
```

**For users:** When a conversation is reopened, the conversation doc shows historical suggestions ("I proposed X"). The outcomes table decorates these with resolution status ("accepted 3 days ago by GM B"). The conversation doc is self-contained history; the outcomes table is read-time enrichment.

**For evals:** Accept/reject rates per model, per workflow, per page kind. Time-to-resolution. Supersession rates. This is the primary signal for model selection and prompt tuning.

### Suggestions in conversation history

When an AgentConversation produces a suggestion, the full content (target blocks' current text and proposed replacement) is written into the **conversation LoroDoc** as a historical record AND sent to the PageActor as a live suggestion. These are independent artifacts.

The conversation record is immutable history - "I proposed X." It never changes after creation. The page suggestion is a living proposal that can be accepted, rejected, or superseded. A conversation should be entirely portable and self-contained. Reopening it after hammock time should not require resolving references to find out what was suggested.

---

## The Compiler

The serialization format requires bidirectional transformation between the agent's markdown and the LoroDoc document model.

### `f()` - LoroDoc → Agent Markdown (Serialization)

Produces the agent-readable format from the page's current state.

**Inputs:** A `DocumentState` reference from the PageActor, graph context from the RelationshipGraph actor, a retrieval tier, a role (GM vs player), and optionally a conversation ID for suggestion scoping.

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
2. Read each block's status, coalesce maximal contiguous runs of equal status, and emit `<player_visible>` / `<gm_only>` spans (the default is `gm_only`)
3. Resolve Page references to display names, emit `{Name}` wiki-links
4. Query the graph for the Page's relationships and tags, emit `<relationships>` and hashtag blocks
5. Compute TOC with word counts from the heading structure
6. Filter by role - for player-facing serialization keep only `player_visible` blocks and strip the spans (literal per block; no subtree pruning)
7. If `conversation_id` is provided: find suggestion marks owned by that conversation, render as `<prior>/<suggestion>` pairs inline. Ignore all other conversations' suggestion marks.
8. If `conversation_id` is `None`: ignore all suggestion marks, serialize pure content. Used for non-agent contexts (export, preview).

**The compiler is a stateless service, not an actor.** It needs the PageActor's document state AND the relationship graph AND embedding results (Tier 2) AND role context AND conversation scoping. Putting serialization on the actor would require the actor to hold references to all of these services. The compiler is a pure function with multiple inputs. The AgentConversation orchestrates: asks the PageActor for DocumentState, asks the RelationshipGraph for context, calls the compiler.

**The compiler always reads from actors.** There is no "read from DB for cold entities" path. Every PageActor always holds a full LoroDoc, reconstructed from relational data on startup. Spinning up a PageActor to serve a Tier 1 index card costs one libSQL read and a few milliseconds of CPU. The actor evicts itself on idle. One read path, through actors, always.

### `f⁻¹()` - Agent Tool Call → Compiled Suggestion (Compilation)

Processes a `suggest_replace` call into a compiled suggestion ready for the PageActor to apply.

**Inputs:** The tool call, the current serialized markdown (from `f()` with conversation scoping), and the PageActor's document state (for block UUID resolution).

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

The PageActor receives this and applies it - adding the mark, storing the metadata, broadcasting via CRDT sync. The compiler produces the suggestion. The actor applies it. Clean separation.

**String match failure is the feedback mechanism.** If the content has changed since the agent last read the page (because the agent's own earlier suggestion was accepted or rejected), the string match fails. The agent gets an error, re-reads via `f()`, sees the current state, and adapts. This is the same mechanism from the v1 design - what changed is that other conversations' suggestions and human edits to non-suggested blocks no longer cause failures.

**Superseding proposals from the same conversation:** When the compiler identifies that the target blocks already have a pending suggestion from the same conversation, the `CompiledSuggestion` includes a supersession flag. The PageActor removes the old suggestion mark (recording `superseded` in the outcomes table) and applies the new one.

**Validation:** Each block's visibility is taken literally from its enclosing span, so there is no ancestor gate to reconcile. Malformed visibility spans (overlapping or unclosed `<player_visible>` / `<gm_only>`) are a parse error on this write path, and a block whose span membership is ambiguous resolves fail-closed to `gm_only`. The compiler logs unresolvable references for GM review (not a hard failure) and warns on structural violations (headings that don't match the template's expected sections).

### Reference Resolution (The Linker)

Wiki-link references `{Name}` are resolved by the linker at both serialization and compilation time.

**On serialization (`f()`):** Page references in LoroDoc nodes (stored with node IDs internally) are projected to display names. The agent sees `{Silver Compact}`, not an internal ID.

**On compilation (`f⁻¹()`):** Display names in agent output are resolved back to graph nodes.

**Resolution strategy:**

1. Exact name match against the campaign graph
2. Alias/fuzzy match (handles renames - "Yurgath Tribe" resolves to the renamed "Yurgath Clan")
3. Match against proposed pages in the current batch (handles forward references to pages being created in the same SessionIngest run)
4. If ambiguous or unresolvable: flag for GM review, insert as unlinked text

This is the same entity resolution capability required for SessionIngest transcript processing. The linker is a shared component.

---

## Template Pages and Agent Instructions

> The on-disk **authoring and import** format for templates (per-locale markdown, frontmatter metadata, localization) is owned by [Templates](2026-06-29-templates.md). This section describes the agent's view of a template page. One reconciliation with that doc: the authoring guidance below is not stored on the template; it lives in a **skill** loaded when an entity is cloned, defined and localized once. (Visibility is the same literal, fail-closed convention in both: unwrapped is `gm_only`, a `<player_visible>` span reveals.)

### Templates Define Structure

A template (template) page defines the section layout, status defaults, and placeholder content for a page kind. When a page is created from a template, the system clones the template's structure.

Example NPC template:

```md
---
name: NPC
---

#NPC

<player_visible>
{Who is this person? What's their role in the campaign?
What would you need to know if they came up unexpectedly?}

# Appearance

{What do people notice first?}

# Personality

{How do they behave? What do they want?}
</player_visible>

# Secrets

{What the players don't know.}

<player_visible>
# History

{How they got here.}
</player_visible>

# GM Notes

{Running notes, plans, future hooks.}
```

### OnCreate Directives

Templates specify tags that are automatically applied to Pages created from them. The NPC template has `OnCreate: tag as #NPC`. This creates a `tagged` relationship to the NPC tag-Page at creation time.

The template itself is not tagged NPC - it _creates entities that are tagged NPC_. This prevents templates from appearing in tag queries alongside actual campaign entities.

### AI Instructions Block

Templates include an AI instructions block that tells the agent how to work with this page kind:

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

When the agent works with a Page, it composes instructions from two layers:

1. **Global skills** - shipped with the product, define general capabilities. `create-or-edit-preamble.md` knows what makes a good preamble. `draft-journal-entry.md` knows how to compress a transcript into a session narrative. `propose-relationships.md` knows how to infer relationships from narrative content.

2. **Template AI instructions** - campaign-specific, per-template, editable by the GM. Define what this specific template needs, what sections to prioritize, what tone to use.

For Milestone 1 (single starter pack: Fantasy Generic), game-system-specific skills (D&D knowledge, Daggerheart knowledge) are part of the global skill layer. When the starter pack marketplace exists, system-specific skills become a third layer between global and template instructions.

The most specific layer (template instructions) overrides the most general (global skills) when they conflict.

---

## Session Pages

Session pages follow the same serialization format but serve a different purpose. They are temporal (one per session) and are both the input to and output of SessionIngest.

### Session Template

```md
---
name: "Session 14: The Northport Docks"
status: player_visible
---

<player_visible>
{What happened this session in 2-3 sentences. Written by AI
after processing, editable by GM. This becomes the session's
index card for future retrieval.}
</player_visible>

<relationships>
@The Silver Compact (arc)
</relationships>

<toc>
...
</toc>

<player_visible>
# Journal

{The narrative of what happened. AI-drafted from transcript,
GM-reviewed. The canonical record.}
</player_visible>

# Prep Notes

{What the GM planned before the session. Valuable for
post-session diffing - what happened vs. what was planned.}

# Sources

{Transcript segments, player recollections, GM notes.
The raw material the journal was derived from.}
```

### Session Preamble Quality

The session preamble is the single most important piece of text for retrieval across the campaign timeline. When the agent needs to understand what happened across 20 sessions, it pulls 20 session preambles. The global `draft-journal-entry.md` skill should include specific instructions for session preamble quality: cover key events, name entities involved, note major state changes (deaths, alliances, revelations), two to three sentences, dense and factual.

---

## Key Design Decisions

### Markdown over Custom DSL

**Decision:** The serialization format is standard markdown with minimal annotations, not a custom DSL. The one XML-like construct in the content flow is the visibility span (`<player_visible>` / `<gm_only>`), a deliberate, narrow exception.

**Why:** LLMs are most fluent in markdown. Users already think in markdown. The heading hierarchy provides tree structure, path-based addressing, and splice anchoring without any custom syntax. The fewer non-markdown elements, the less parsing and the fewer ways for the LLM to produce invalid output. Visibility earns its XML exception because a span boundary is a single attention target and makes a block's status locally decodable, where a per-block markdown suffix would push meaning into the absence of a mark. The exception stays scoped to visibility (alongside the existing `<relationships>` / `<toc>` / `<prior>`-`<suggestion>` projections); everything else stays markdown.

### Wiki-Links without IDs

**Decision:** References are bare display names (`{Silver Compact}`), resolved by a linker. The agent never sees or manages entity IDs.

**Why:** IDs are noise in the agent's context window and a source of transcription errors in LLM output. Name-based resolution is already a required capability (SessionIngest entity resolution). The linker handles renames via alias/fuzzy matching. Ambiguity (two Pages with the same name) is rare and flagged for GM review - a data quality signal, not a system failure.

### Relationships as Read Context, Mutated via Tools

**Decision:** Relationships appear in the serialization format as read context. The agent modifies relationships through `propose_relationship` tool calls, not by editing the document.

**Why:** Relationships are graph structure, not page content. They live in the graph database and go through the suggestion queue. Embedding them in the editable document surface would require the compiler to parse relationship changes out of the markdown and route them through a different write path than content changes. Keeping them read-only in the format and editable via tool calls respects the architectural boundary between page content (LoroDoc/CRDT) and graph structure (libSQL/suggestion queue).

### Marks on Blocks, Not Structural Replacement

**Decision:** Suggestions are marks on block UUID ranges with proposed content as metadata. The original blocks remain in the document tree unchanged.

**Why:** Structural replacement (the v1 design) modified the document tree when a suggestion was created. This created serialization order dependencies - the first suggestion changed the tree so subsequent suggestions targeting overlapping content couldn't find their target. Marks don't modify the tree. Multiple suggestions coexist on stable blocks. The pattern follows TipTap's comment thread model, which is proven at scale.

### Blocking over Staleness Detection

**Decision:** Blocks under pending suggestions are read-only to human editors. No staleness concept exists.

**Why:** Staleness detection (comparing original text to current text at render time) was necessary in the v1 design because humans could edit text underneath suggestions. With blocking, the text under a suggestion cannot change. There is no drift. No race conditions. No `original_text` field to store or compare. The escape hatch is one action: reject the suggestion.

### Conversation-Scoped Agent Views

**Decision:** Each agent sees only its own pending suggestions. Other conversations' suggestions are invisible in the serialization.

**Why:** Agents don't need to reason about each other's proposals - that's the GM's job. Conversation scoping means the string-match target is always the original content (for blocks without this conversation's suggestions) or the agent's own prior proposal (for blocks with this conversation's suggestions). The agent's view is stable and predictable. Cross-conversation deconfliction happens in the editor UI, where the GM has full context.

### Supersession Within, Coexistence Across

**Decision:** Same-conversation suggestions targeting the same blocks supersede. Different-conversation suggestions coexist.

**Why:** Within a conversation, the user is iterating - "try again, make it darker." The latest attempt represents their current intent. Across conversations, proposals are independent ideas from potentially different users. Both deserve review. Superseding across conversations would silently discard one user's agent's work, which violates the expectation that each user's agent works independently.

### Suggestion Duplication over Cross-Reference

**Decision:** Suggestions are duplicated - one copy as history in the conversation LoroDoc, one as a live mark on the Page's LoroDoc.

**Why:** A conversation should be entirely self-contained and portable. It's a document. Reopening it after days should not require resolving foreign keys or joining PageActor rooms to discover what was suggested. The active suggestion on the page has its own lifecycle (accepted, rejected, superseded) that the conversation doesn't need to track structurally. The outcomes table provides status decoration at read time for users who want it, and eval signal for measuring agent quality.

### Visibility Is Literal Per Block, Fail-Closed

**Decision:** Each block carries its own status; nothing inherits down the heading tree. A block is player-visible only if its own status is `player_visible`. The stored default is `gm_only`. In the markdown this serializes as `<player_visible>` / `<gm_only>` spans (run-length encoding over per-block status, not a scope).

**Why:** Fail-closed because the dangerous direction is a secret going public, so the safe state is the default and a forgotten reveal hides. Literal (no cascade) because an inheriting model only ever governed a block explicitly revealed under a hidden ancestor, and there it silently overrode the GM's reveal: a hidden-omission failure that surfaces days later when a player reads. Leak-safety comes from the fail-closed default, not from an ancestor gate, so dropping the cascade costs no safety while removing that failure and the editor-vs-serialization divergence it required.

### Tags as Hashtags

**Decision:** Tags render as `#NPC #Human <gm_only>#Villain</gm_only>` using hashtag syntax, hidden tags wrapped in a `<gm_only>` span.

**Why:** Universally understood (Obsidian, Logseq, social media). Visually distinct from narrative relationships. Tags are graph relationships (`tagged` edges to tag-Pages) but serve a different purpose than narrative edges - the hashtag syntax makes this distinction visible.

### Preamble as Implicit Position

**Decision:** The preamble is the content between the H1 and the first structural element. No explicit tag marks it.

**Why:** Every markdown document already has this concept. Adding an explicit `<preamble>` wrapper would be redundant and is one more non-markdown element for the LLM to manage.

---

## Open Questions

- **Loro mark/annotation primitives.** Whether Loro natively supports range annotations (like ProseMirror marks) or whether suggestion marks are LoroMap entries keyed by suggestion ID with block UUID lists is a Loro/TipTap spike question. The model is the same either way; the storage representation differs.

- **TipTap extension design.** The editor needs a custom extension for suggestion rendering - inline diff for single suggestions, multi-proposal view for overlapping suggestions, accept/reject controls, read-only enforcement on target blocks. This is a frontend design concern informed by but not determined by this document.

- **Proposal block rendering details.** The visual design of inline diffs (tracked-changes style, side-by-side, or something else) and the multi-suggestion UI (tabs, stacked diffs, carousel) are frontend decisions. The backend provides: which blocks are suggested, what the proposed replacement is, who proposed it, and whether other suggestions overlap.

- **Proposed page visibility in other pages' serialization.** When SessionIngest creates a proposed NPC, should that NPC appear in other pages' `<relationships>` blocks and RAG results? Likely yes with a `[proposed]` annotation, but the lifecycle and cleanup model needs design.

- **Suggestion expiry.** The mark model needs an expiry mechanism - either a TTL checked at render time, or a periodic sweep by the PageActor. The outcomes table should record `expired` as an outcome.

- **Bulk review UX.** After SessionIngest produces many suggestions across a page, is there a "review mode" that walks through suggestions sequentially? The backend needs to support whatever bulk operations the UX requires (the SuggestionTarget trait may need batch methods).

- **Starter pack marketplace.** The "Town Market" - a communal free marketplace for user-submitted starter packs. Deferred to Milestone 2+.

- **Preamble quality feedback loop.** Measuring whether GM-authored preambles improve over time and whether retrieval quality correlates with preamble quality requires the eval framework (Milestone 3).
