# AI Serialization & Editing Model

**Status:** Draft - designed, not built.
**Date:** 2026-06-30
**Supersedes:** [AI Serialization Format v2](../archive/plans/2026-03-25-ai-serialization-format-v2.md) (archived). The v2 doc over-specified mechanism we only know the general shape of; that precision read as decided and misled design. This doc carries forward its invariants and reasoning and drops the premature detail.
**Related:** [AI PRD](2026-02-22-ai-prd.md) - product behavior; [Templates](2026-06-29-templates.md) - authoring/import; [Entity Relationship Temporal Model](2026-06-23-entity-relationship-temporal-model.md) - relationships and reveals; [Campaign Actor Domain Design](2026-05-04-campaign-actor-domain-design.md) - actor topology, the `f()` orchestration, suggestion persistence; [Campaign Creation Architecture](2026-05-22-campaign-creation-architecture.md) - block storage and grep mechanism.

## Scope

This doc owns three things:

1. The **format** the AI agent reads and writes (a markdown dialect).
2. The **compiler** between that format and the CRDT document model.
3. The **editing model** - how agent edits become reviewable proposals.

It states **shape and invariants, not wire syntax or struct layouts.** The examples are illustrative; the concrete syntax, tool signatures, and storage representations are owned by code when built. Over-precision in this doc is a liability: a written specific reads as a settled decision even when it is a guess.

Out of scope: product behavior (workflows, the suggestion *system*, retrieval *capabilities*) belongs to the [AI PRD](2026-02-22-ai-prd.md); template authoring and import belong to [Templates](2026-06-29-templates.md).

## Context

The agent works across read and write workflows over campaign pages. The editing medium is TipTap (ProseMirror) backed by Loro CRDTs; the campaign is a graph of pages (nodes), relationships (edges), and blocks (atomic content within pages). The agent cannot reason in ProseMirror JSON or Loro operations - it needs a human-readable surface. That surface is markdown: nearly what a human would write in Obsidian or Logseq, with a small number of non-markdown annotations for data that is not page content.

## Invariants

These are the durable decisions. They constrain any implementation; the mechanism that satisfies them is open.

1. **Markdown is the format.** LLMs are most fluent in markdown, users already think in it, and the heading hierarchy yields the page tree, path-based addressing, and edit anchoring with no custom syntax. The fewer non-markdown elements, the fewer ways for the model to emit invalid output.

2. **Headings are the tree.** `#`/`##`/`###` define structure; `# History` with `## Session 3` nested is a parent/child. The page title is frontmatter, never a heading, so the body owns the full heading range.

3. **Visibility is literal per block, fail-closed.** Each block's own status (`gm_only` / `player_visible` / `retconned`) is the whole truth; nothing inherits down the heading tree; the stored default is `gm_only`, so a forgotten reveal hides rather than leaks. In markdown this serializes as `<player_visible>` / `<gm_only>` spans, which are **run-length encoding over per-block status - a wire-format compression, never a stored scope.** A block added later does not "fall into" a surrounding span. (Already true in code: status is a per-block column.)

4. **References carry stable identity plus a frozen label.** A reference is `(page id, label)`. The id is identity - it powers backlinks, graph traversal, and click-through, and it survives renames. The label is the prose *as written* and never auto-rewrites: a journal that said "the Count" keeps saying "the Count" even after the party learns he is Duc Croissant. Output is **faithful** (emit the frozen label); input is **forgiving** (the agent writes a bare name, the linker resolves it by alias/fuzzy match and freezes whatever was written). The agent never sees or manages ids.

5. **Relationships are graph-derived, read-only in the format, mutated only via tools.** They appear in the serialization as context so the agent can reason about connections, but page content (CRDT) and graph structure (relational) are different write paths. The agent changes edges through a `propose_relationship`-style call, not by editing the document.

6. **The agent proposes; the GM disposes.** Every agent write is a durable, reviewable proposal. Nothing the agent does mutates the graph or page content directly.

7. **Progressive disclosure.** The format serializes at increasing cost - an index card (the preamble), the card plus retrieval-selected blocks, then the full page. The preamble is the index card: the densest identity text, and what gets packed when the agent needs breadth over depth.

## The format (shape)

One compact, illustrative example - syntax is not the contract:

```md
---
name: Duc Croissant
status: player_visible
---

#NPC #Noble <gm_only>#Villain</gm_only>

<player_visible>
A flamboyant pastry magnate the party knows as {the Count}.
</player_visible>

<gm_only>
He is the financier behind the Ashen Syndicate.
</gm_only>

<relationships>
@Ashen Syndicate - secretly funds | bankrolled by
@Northport - resides in | home of
</relationships>

<player_visible>
# Appearance

Immaculate, powdered, never without a glass of something sparkling.
</player_visible>
```

- **Frontmatter** is the page's identity block: `name` (the page's current display name), the page's own `status`, and extensible identity facets. It holds only facets with no per-item visibility; anything secret-per-item stays inline under a span.
- **Tags** are hashtags immediately after the frontmatter; each is a `tagged` relationship to a tag-page and carries its own visibility (a hidden tag wraps in `<gm_only>`).
- **Preamble** is the text between the frontmatter and the first structural element - defined by position, no wrapper. It is the index card.
- **References** are inline (`{the Count}`): the frozen label is shown; the id rides in the internal representation, not in the agent's view (see *Names over time*).
- **Relationships** are a read-only `<relationships>` block (`@Target - outgoing | incoming`).
- **Visibility spans** wrap contiguous equal-status runs (invariant 3).
- **Sections** are headings; names unique within a parent give path addressing (`History/Session 3`).
- A **table of contents** with per-section word counts is a computed projection used by the cheaper retrieval tiers - a `stat` before a `cat`.

## Names over time

Frozen labels (invariant 4) make historical prose honest by construction, and they make the leak-safe path the default - a player-written label can never expose a name the party did not know.

- **Reveals are knowledge-axis facts, not renames.** "The party learns the Count is Duc Croissant" is a secret relationship that flips `Hidden → Revealed(session)` in the [temporal model](2026-06-23-entity-relationship-temporal-model.md), and/or a `gm_only` block on a page titled with the *known* name. Per-block `status` already prevents the spoiler; no per-name visibility machinery is required.
- **GM-facing serialization may annotate the canonical entity** behind a frozen epithet (illustratively, `{the Count → Duc Croissant}`) so the agent can unify references across blocks. This annotation is **omitted from player-facing serialization**, which keeps the player view leak-safe for the same reason the prose is.
- **Deferred: aliases as a first-class feature.** Multiple names per entity, each permission/knowledge-scoped, is the *many* case (and the only reason to ever model one being as two pages). The shipping model is the 0/1 case: one display name plus frozen historical labels. See [Templates](2026-06-29-templates.md) *Deferrals*.

## The compiler

The format requires transformation between the agent's markdown and the LoroDoc model. The serializer is **stateful** - this is the load-bearing correction over the archived doc.

```
f(loro_doc, permission, conversation_id) -> (markdown, alignment)
```

- `permission` is the reader's role. It both filters the output (a player-facing serialization keeps only `player_visible` blocks and strips the spans) and **bounds the reverse**: an edit derived from a player-scoped serialization structurally cannot touch a block the player could not see.
- `conversation_id` scopes which pending suggestions render inline (each conversation sees only its own, as `<prior>/<suggestion>` pairs - illustrative). It is also the key the alignment is stored under.
- `alignment` is the **hidden state that maps each markdown region back to the block identity it came from.** Without it the reverse transform is **underdetermined**: given edited markdown, you cannot distinguish "block edited" from "block deleted and a new block inserted" - the agent never sees ids and the markdown carries no positional anchor.

The reverse, `f⁻¹`, is therefore **a diff/patch over the alignment**, not a fresh parse: diff the agent's edited markdown against what it was given, map the changed regions to block ids via the alignment, and emit block-level operations as a proposal.

**The alignment mechanism is an open spike, not a spec.** Candidate shapes:

- *Invisible anchors in the markdown* the agent must preserve - robust identity, kills the edit-vs-delete ambiguity, but agents can mangle markers.
- *An external alignment map* keyed by `conversation_id` - keeps the markdown clean, but leans on diffing free-form edited text, which is fragile when the agent reflows.
- *String-match patches* (the archived doc's approach: match `old_content` against the page) - carries neither anchors nor a map, which is precisely the under-determination above.

**Staleness is the same problem as the alignment going invalid.** The instant another writer mutates the doc, an outstanding alignment may no longer hold, so a pending agent edit must block or be re-aligned. "Blocking" and "stale alignment" are one concern, not two.

**The compiler is a stateless service, not an actor.** It needs document state from the page's actor, graph context for relationships, and (for the retrieval tiers) embedding results; an orchestrator gathers those and calls the compiler. The compiler always reads from live actors, never from cold storage.

## Three transforms, not one codec

The "is there one bidirectional codec?" question dissolves once you see that only one direction needs the alignment state:

| Transform | Direction | State | Notes |
|---|---|---|---|
| **Template import** | `markdown file → blocks` | none | One-way parse; fresh block ids minted; no prior to align against. Owned by [Templates](2026-06-29-templates.md). |
| **Search projection** | `blocks → markdown` | none | One-way, per-block, lossy-OK (an inexpressible node renders a placeholder). Feeds grep. See [Campaign Creation Architecture](2026-05-22-campaign-creation-architecture.md). |
| **Agent edit** | `loro → markdown → edits → ops` | **alignment** | The only round-trip, and the only stateful one (the `f`/`f⁻¹` above). |

Template import and the search projection are simple one-way functions and share, at most, a parser direction. They are **not** "the same transformation the agent write path needs" - that path is the stateful diff. Conflating them is what made the round-trip look harder than it is and the import look harder than it is.

## Editing model (suggestions)

The output of `f⁻¹` is a **suggestion**: a durable, reviewable proposal, never an applied edit (invariant 6). Two decisions are load-bearing here; everything else about the suggestion *system* (types, lifecycle, batching, contradiction handling) is product behavior owned by the [AI PRD](2026-02-22-ai-prd.md).

- **Suggestions are annotations on stable blocks, not edits to the document tree.** A suggestion marks a contiguous block range and carries proposed content as metadata; the original blocks are untouched until the GM accepts. This keeps the tree stable so overlapping and concurrent proposals coexist without a serialization-order dependency - the same model as comment threads in a rich-text editor. (Whether the mark is a native Loro range annotation or a keyed map entry is a Loro/TipTap spike; the model is the same either way.)
- **Serialization is conversation-scoped.** An agent sees only its own pending suggestions; others' marks render as plain content. Cross-proposal deconfliction is the GM's job in the editor, not the backend's.

## Storage

The at-rest format is settled elsewhere and noted here only to close the loop: a block's content is persisted as its **lossless block JSON (the ProseMirror node tree) - the source of truth.** The agent markdown and the search markdown are **derived projections** of that source, allowed to be lossy. The CRDT oplog is not persisted; the LoroDoc is reconstructed from relational rows on checkout. See [Campaign Creation Architecture](2026-05-22-campaign-creation-architecture.md) for the schema and the grep mechanism, and [Campaign Collaboration Architecture](2026-03-25-campaign-collaboration-architecture.md) for the "relational data is the data at rest" invariant.

## Non-goals / deferred

- **Exact wire syntax and tool signatures.** Owned by code when built; the examples here are illustrative.
- **The suggestion system's product surface** (types, lifecycle, batches, contradiction) - [AI PRD](2026-02-22-ai-prd.md).
- **Template authoring/import format** - [Templates](2026-06-29-templates.md).
- **Multi-name aliases with permission/knowledge scopes** (and the two-page persona modeling that only it would justify).
- **Rich-text marks in serialized blocks** - gated on the block codec's marks work; today the codec is plain-text plus structure.
- **Editor rendering** of suggestions (inline diff, multi-proposal UI) - a frontend concern.
- **Session pages** as a distinct kind - they will follow this format; specifics land when the `session` kind does.

## Open spikes

- **The alignment primitive** (the `f`/`f⁻¹` hidden state). The single highest-leverage unknown: invisible anchors vs. external map vs. string-match. Prototype, do not specify.
- **GM epithet → canonical annotation** in serialization: shape and how it interacts with the agent's reference-unification reasoning.
- **Loro mark/annotation primitives** for suggestion marks (native range annotation vs. keyed entries).
- **Proposed-entity visibility** in other pages' serialization (does a proposed NPC appear in relationships/RAG, and with what lifecycle).
