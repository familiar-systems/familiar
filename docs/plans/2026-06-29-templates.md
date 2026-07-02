# Templates

**Status:** Draft
**Date:** 2026-06-29
**Related:** [AI Serialization & Editing Model](2026-06-30-ai-serialization-and-editing-model.md) · [Multi-Section Document Structure](2026-06-07-multi-section-document-structure.md) · [Campaign Creation Architecture](2026-05-22-campaign-creation-architecture.md) · [Glossary](../glossary.md)

> This doc owns the template system end to end: what a template **is** (a page you clone from), how templates are **authored** on disk and **imported** into a campaign, and the **visibility contract** a cloned page inherits. It subsumes the earlier "Templates as Pages" and "Template Authoring & Import Format" notes. The storage/CRDT layout it feeds is owned by [Multi-Section Document Structure](2026-06-07-multi-section-document-structure.md); the agent-facing markdown dialect it borrows from is owned by [AI Serialization & Editing Model](2026-06-30-ai-serialization-and-editing-model.md).

---

## Context

A template is the structured starting point for a campaign entity: open the NPC template, clone it, and you get a page with the right sections and prompts ready to fill. Templates ship with a game system's starter pack and are then the GM's to edit.

Two things this doc settles, because both were split across older notes and one had an open tension:

1. **A template is not a schema; it is a page.** There is no `Template` entity with typed fields. A template is a page of kind `template`, and creating an entity clones its block structure.
2. **Visibility is literal per block.** A block is player-visible exactly when its own status says so. Nothing inherits down a heading tree. This resolves a cascade model that could silently hide content a GM meant to reveal (see [Visibility](#visibility)).

The runtime is partway there: `template` is already a `PageKind` with `template_id` lineage, but today a template's body is parsed and discarded, no importer turns a template into a page, and the selected system's `bundle` is read-only wizard decoration. A new campaign is born with one empty home page and zero templates. This doc defines the format and the path that closes those gaps.

---

## The model: templates are pages

A template behaves like a prototype: you copy it to make instances. The "NPC template" is itself a page that looks like what an NPC page should look like (sections, placeholder prompts), and creating "Graydalf the Wisened" clones that page's block structure into a new entity.

- **Templates are pages.** A template is a page of kind `template` (`PageKind::Template`). There is no separate `Template` type, no `TemplateField`, no `TemplateFieldType`, no `TemplateId`. A template is identified by its `PageId` like any page.
- **The template editor is the page editor.** GMs customize a template by editing it as a page. There is no separate template-builder UI and no field-type selectors. The GM sees a page and arranges it.
- **Layout is content.** The arrangement of prose, headings, and (later) widgets is the page's block structure, not metadata on a schema entity. The page *is* the layout.
- **GMs own their templates.** Templates are campaign-scoped, cloned from a starter pack at campaign creation, then edited freely.
- **No structured field schema.** The AI's semantic search over block content is the query layer. "Show me all chaotic-evil NPCs" is a natural-language query resolved against block content, not a `WHERE` clause against typed columns. This is what removes the entire `TemplateField` machinery (field types, select options, validation, queryable indexes): a separate schema entity would create an artificial split between "template structure" and "page content" that does not match how a page is actually built, and would leave the page layout with no home (neither typed fields nor flat blocks describe where a portrait, a stat block, and a narrative sit on the page). Modeling the template as a page puts the layout where it belongs, in the blocks.

**Lineage.** A cloned entity carries `template_id: Option<PageId>` pointing back at the template it came from. This is lineage, not schema enforcement: the entity is a free page afterward.

---

## Categorization: lineage and tags

Two questions, two mechanisms, no new domain primitives.

**"Show me all NPCs": template lineage.** Every entity cloned from the NPC template carries `template_id` pointing at it, so the query is a lookup over `template_id`. Lineage is the primary categorization.

**Cross-cutting classification: tags as relationships.** `template_id` is single-valued; it answers "cloned from what?" but not "this NPC is also a Villain and Deceased." Cross-cutting tags use the existing graph:

- A tag is a page. The "Villain" tag is a page named "Villain," optionally with its own content.
- Tagging is a relationship: `Graydalf -[tagged]-> Villain`.
- Tags carry visibility (a `gm_only` tag relationship hides the classification from players), show up as backlinks, and can be proposed by the AI through the suggestion system.

A flat `tags: string[]` field is rejected for the same reason a separate `Template` entity is: it spawns a parallel universe that does not participate in the graph (no visibility, no pages, no backlinks, no AI proposals). Tags-as-relationships reuse the whole existing machinery for free. The relationship label (`tagged`) is the only discriminator the UI needs to render tags as chips rather than narrative edges.

---

## Authoring format

`content/` carries two kinds of thing, and only one benefits from markdown.

- **Catalog config (`systems.yaml`) stays structured.** A game system is ids, a hex color, a `popular` flag, a `bundle` of template slugs, and short labels. That is maintainer configuration; markdown would not improve it.
- **Template bodies become markdown.** This is the surface a contributor writes. A node tree authored in YAML reads as code, forces every locale to share a paragraph breakdown, and duplicates the markdown-to-blocks path the AI system needs anyway. Markdown fixes all three.

### What a template is, on disk

A template is a **cloneable skeleton**: the ordered sections and the short placeholder prompts inside them (a line like *What do people notice first?*). Cloning reproduces this structure as the new page's starting point.

Authoring guidance ("how to write a good NPC") is deliberately **not** on the template. It belongs in a **skill** the agent calls when it needs it, defined and localized once, rather than copied into every template and locale. Skills are self-triggered and role-gated (a GM-only authoring skill never enters a player's context), so there is no per-template binding and no clone-time load. The encyclopedic "what an NPC is" prose moves out too, but most of it is droppable: a capable model already knows it, so a skill carries only the actionable heuristics worth steering on. Skills are not a built `PageKind` yet, so v0 templates ship skeleton-only (see [Deferrals](#deferrals)).

### Localization: per-locale files

Localization is keyed at the **file**, not the string. A template is a set of markdown files, one per locale, in the template's own directory and named by locale tag (`common/npc/npc.en.md`, `common/npc/npc.fi.md`), with `en` required and canonical. The slug is the directory path (`common/npc`); lineage and `bundle` references key on the slug, not the filename.

The split follows one observation: **section structure is driven by the game system, not the language.** An NPC has the same sections in English or Finnish; only the words change.

- **Locale-invariant** (canonical in `en`): the section structure, the `OnCreate` tag, the icon, the slug, and everything in `systems.yaml`.
- **Per-locale prose:** the name, description, and placeholder prompts.

Prose is localized because the placeholder prompts seed a page the GM and agent then fill in; in a Finnish campaign they should be Finnish, so the agent is not code-switching between Finnish content and English prompts (prompting in one language while generating in another measurably lowers quality and triggers off-target drift). The corollary holds in reverse: structured directives are not prose and are not translated. A structure-parity lint across a template's locale files is worth adding once a second locale exists; today everything is `en`, so it is deferred.

### Format: a graph-independent subset of the agent dialect

The template body is a subset of the agent markdown dialect ([AI Serialization & Editing Model](2026-06-30-ai-serialization-and-editing-model.md)). Reusing that dialect is the point: it is what the markdown-to-blocks compiler consumes from the agent anyway.

In scope:

- The frontmatter title and page metadata.
- **Preamble by position:** the prose between the frontmatter and the first `#` is the preamble.
- Native `#` sections and nested headings, and paragraphs.
- Visibility spans (`<player_visible>`), fail-closed (see [Visibility](#visibility)).
- `#tags`.

Excluded, because they are graph-coupled and depend on systems that are not built (the linker, the relationship graph): `{Name}` wiki-link resolution, `<relationships>` blocks, and suggestion marks (`<prior>`/`<suggestion>`).

Placeholder prompts carry no special syntax: they are ordinary prose paragraphs that ship to be overwritten (a template ships prompts, not filled content). `{}` denotes an inline reference in the full dialect and is not used in templates.

### Frontmatter is the identity block

Page metadata (name, description, icon, `OnCreate`) lives in YAML frontmatter at the top of each locale file. The title is `meta.title`, a field, not a heading, so the body owns the full heading range: `#` is the top section, `##` nests under it. The importer compiles only the body into blocks; frontmatter drives the catalog, the page meta, and lineage.

Frontmatter is typed and extensible, not a fixed set. The rule that keeps it disciplined: it holds page facets with **no per-item visibility that are not content** (an `aliases` list, a system bundle's stat scalars). Anything that can be secret per item stays inline, the way `#tags` and visibility spans do.

### Worked example (`templates/common/npc/npc.en.md`)

```md
---
name: NPC
description: That one person you met. Or centaur. Or whatever.
icon: contact
onCreate:
    tag: NPC
---

<player_visible>
Who is this person? Their role, their affiliations, and what makes them matter if they come up unexpectedly. A paragraph or two, 1000 words or less, shorter is better.
</player_visible>

<gm_only>
Any secret worth keeping off the public index card. Counts toward the word limit.
</gm_only>

<player_visible>

# Appearance

What do people notice first?

# Personality

How do they behave? What do they want?
</player_visible>

<gm_only>

# Secrets

What the players do not know.
</gm_only>

<player_visible>
# History

How they got here.

## Session 1

Sample session entry. Put session notes here.
</player_visible>

<gm_only>

# GM Notes

Running notes, plans, future hooks.
</gm_only>
```

Every region is wrapped: visible content in `<player_visible>`, secret content (the second preamble line, Secrets, GM Notes) in `<gm_only>`. A template labels both sides explicitly so a clone-many artifact reads unambiguously; the fail-closed default (an unwrapped block is `gm_only`) is the safety net behind that, not a license to leave secrets unmarked.

### Section mapping

The page kind declares the sections; the markdown fills them by position. `PageKind::Template.sections()` (like the entities it clones to) is `[Preamble, Body]`. The many `#` headings (Appearance, Personality, the sample `## Session 1`) are **headings inside the `body` section, not sections themselves** (the container-vs-heading distinction is owned by [Multi-Section Document Structure](2026-06-07-multi-section-document-structure.md)). So:

- Prose before the first `#` compiles into the `preamble` section.
- Everything from the first `#` onward compiles into the `body` section.

Explicit section demarcation only becomes necessary for a kind with more than two containers (a future Session template: prep / summary / transcript / journal); deferred until such a template exists. The relationship panel is **not** part of the template format: the editor renders it from the page's graph edges, so baking a widget block into templates would couple them to that widget's representation. The preamble/body boundary stays purely positional (the first `#`).

---

## Visibility

**Visibility is literal per block.** Each block stores its own status (`gm_only` / `player_visible` / `retconned`), and that stored status is the whole truth: a block is player-visible exactly when its own status is `player_visible`. **Nothing inherits.** A heading marked `gm_only` does not hide the blocks beneath it, and a heading marked `player_visible` does not reveal them. The unit of visibility is the block, the same flat unit the editor and the ProseMirror schema already use.

**The default is fail-closed.** A block with no explicit reveal is `gm_only`. Forgetting to reveal hides; the consequential act (revealing) is the one that must be explicit. This matters double for a template, which is cloned many times: the inverse default would leak in every clone made from a template with a missed mark.

**The GM-facing contract, stated plainly:** *what you mark on a block is what players and their agent see. Nothing inherits. Unmarked is hidden.* The editor renders each block's own status directly, so the GM never sees a block presented as visible that the agent or a player would treat as hidden. The realistic failure a cascade would have caused, a GM reveals a fact, an ancestor silently eats it, and the omission only surfaces days later when a player reads, cannot happen: there is no ancestor to eat it.

### Spans are run-length encoding, not scope

In markdown, visibility serializes as XML-like spans: `<player_visible>...</player_visible>` and `<gm_only>...</gm_only>`. A span is **run-length encoding over per-block status, not a stored scope.** Storage stays one status per block; the compiler coalesces a contiguous run of equal-status blocks into one span on the way out, and expands a span back to per-block status on the way in. The span lives only in the markdown. The instant it hits storage it is per-block again, and a block added later does not "fall into" a scope. If a span ever persisted as an inherited scope, that would silently rebuild the cascade this model exists to avoid.

A span is chosen over a per-block suffix because it reads better to an LLM: a span boundary is one attention target, and a block's status is decodable from its enclosing tag rather than inferred from the absence of a mark. Template authoring uses total labeling (both `<player_visible>` and `<gm_only>` spans), the same convention as the agent's read and write views, so a block's status is always explicit on the page rather than implied by a bare gap. The full per-surface treatment is owned by [AI Serialization & Editing Model](2026-06-30-ai-serialization-and-editing-model.md).

### Vocabulary

The terms are `gm_only` / `player_visible` / `retconned`, one set across the UI, the markdown tags, the docs, and the code. The enum is `Status` in `crates/campaign-shared/src/status.rs`. Today the player-visible variant is still named `Known` in code; renaming it to `PlayerVisible` and propagating the new wire token is tracked there and is out of scope for this doc (see [Deferrals](#deferrals)).

---

## Compilation and import

### Markdown to blocks (the shared core)

A markdown-to-blocks compiler turns a template's body into the rows page genesis already understands: a sequence of `(Section, blob)` where the blob is the existing at-rest block shape (`{ nodeName, attributes (including a freshly minted block id), children }`, the output of the block codec). This is a **one-way parse**: it mints fresh block ids and has no prior document to reconcile against. It is *not* the agent write path - `f⁻¹` is a stateful diff/patch that aligns edited markdown back onto existing blocks (see [AI Serialization & Editing Model](2026-06-30-ai-serialization-and-editing-model.md)). The two share a parser direction at most; this core stays graph-independent, and the AI path's dialect features layer on separately.

A known limitation rides along: the block codec is plain-text only today (it drops rich-text marks), so v0 templates are headings and plain paragraphs. Bold, italic, and links wait on the codec's marks work and do not block this effort.

### Genesis

Page genesis already has a single constructor that buckets `(Section, blob)` rows into the kind's declared section containers (`LoroPageDoc::from_blocks`), but every page is currently born with empty sections because the genesis builder feeds it no rows. Importing a template means feeding the compiled rows into that builder, through the page's owning actor:

- The genesis builder (`build_new_page` / `build_seeded_page`) gains the ability to accept seeded rows and a `template_id`, per its own standing TODO.
- The mutation flows through `PageActor` and the supervisor's create workflow. Nothing writes a page's rows around its owning actor; that invariant holds for imports too.

### Template instantiation (clone)

Creating an entity from a template (`from_template_id`, currently a 501) clones the template page's blocks: deep-copy each block, mint a fresh block id, preserve its `section` and per-block visibility, reset per-section ordering, and set `template_id` on the new page for lineage. There is no guidance to clone: authoring guidance lives in a skill the agent calls when it needs it, so the clone path only clones blocks and applies the `OnCreate` tag.

### Campaign creation

On a campaign's first checkout, the selected system's `bundle` is instantiated as `template`-kind pages, with names and prose resolved to the campaign's content locale (`en` fallback). This replaces today's behavior, where the wizard's system selection is recorded but the bundle is never instantiated. Threading the selected system id and locale from the onboarding wizard into campaign creation is the connecting work.

---

## Deferrals

- **The `Status` code rename** (`Known` -> `PlayerVisible`, and propagating the new wire token across `StatusCol`, the Loro string, generated TS, and consumers). Vocabulary is settled here; the code change is tracked in `status.rs` and run after these docs land.
- **Skills.** `Skill` is not a `PageKind` yet. Authoring guidance belongs in skills the agent calls (self-triggered, role-gated), not on the template; v0 templates ship skeleton-only. The distilled authoring heuristics worth keeping land in those skills once the kind exists.
- **Rich marks in template prose.** Gated on the block codec's marks work; v0 is headings and plain paragraphs.
- **Structure-parity lint** across a template's locale files. Add when a second locale exists.
- **Template evolution.** When a GM updates a template, existing entities cloned from it are unaffected (snapshots). Opt-in sync or diff-and-suggest is a future product decision.
- **`systems.yaml` labels.** Whether to keep the system name and tagline as per-string `LocalizedString` maps or move them to per-locale files; leaning keep, since they are short config strings with no structural-parity problem.
- **Alias visibility / multiple names.** Aliases ship as a public frontmatter list with a single display name. Multiple names per entity, each permission/knowledge-scoped (the *many* case - a concealed identity, or names revealed over time), need per-item visibility and an inline form; deferred until that is a real feature. In-prose references stay honest meanwhile via frozen labels (see [AI Serialization & Editing Model](2026-06-30-ai-serialization-and-editing-model.md)).
- **Wizard-to-creation wiring.** The selected system id and locale must reach campaign creation so the `bundle` can be instantiated; today the bundle is read-only wizard decoration.

---

## References

- [AI Serialization & Editing Model](2026-06-30-ai-serialization-and-editing-model.md): the agent markdown dialect this format subsets; the visibility model and the span serialization.
- [Multi-Section Document Structure](2026-06-07-multi-section-document-structure.md): the section model, the container-vs-heading distinction, preamble-by-position, and the per-block visibility filter.
- [Campaign Creation Architecture](2026-05-22-campaign-creation-architecture.md): the catalog and creation flow the bundle consumption plugs into.
- `crates/campaign-shared/src/status.rs`: the `Status` enum (`gm_only` / `player_visible` / `retconned`).

### Localization rationale (external)

- [Beyond English: Prompt Translation Strategies across Languages and Tasks](https://arxiv.org/pdf/2502.09331)
- [Multilingual Prompting for Improving LLM Generation Diversity](https://arxiv.org/html/2505.15229v2)
- [Why Your LLM Prompts Should Match Your Content Language](https://ryanstenhouse.dev/why-your-llm-prompts-should-match-your-content-language/)
