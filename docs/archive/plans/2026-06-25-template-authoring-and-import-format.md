# Template Authoring & Import Format

**Status:** Draft
**Date:** 2026-06-25
**Related:** [Templates as Pages](2026-02-20-templates-as-pages.md) · [AI Serialization Format v2](2026-03-25-ai-serialization-format-v2.md) · [Multi-Section Document Structure](2026-06-07-multi-section-document-structure.md) · [Campaign Creation Architecture](2026-05-22-campaign-creation-architecture.md) · [Glossary](../glossary.md)

> This doc owns **how authored templates are written and imported**: the on-disk format, its localization model, the markdown-to-blocks compilation, and how a new campaign is born with its system's templates. The storage/CRDT layout it feeds is owned by [Multi-Section Document Structure](2026-06-07-multi-section-document-structure.md); the agent-facing markdown dialect it borrows from is owned by [AI Serialization Format v2](2026-03-25-ai-serialization-format-v2.md).

---

## Context

`content/` already holds an authoring format for the campaign-creation catalog: `systems.yaml` (the game systems shown in the wizard, each with a `bundle` of template slugs) and `templates/**/*.yaml` (the templates a bundle references). The template format is `meta` (localized name, description, icon) plus a `body` expressed as a ProseMirror-style node tree.

That format has three problems:

1. **It reads as code.** A node tree authored in YAML is unwelcoming to a contributor who could otherwise write a markdown document.
2. **Its localization model forces structural parity.** Every human-readable string is a `LocalizedString` (a map keyed by language tag, `en` required). Keying each _paragraph_ by locale forces every language to use the same paragraph count and the same breakdown. Languages do not translate paragraph-for-paragraph; a concept that is two paragraphs in English may be three in Finnish.
3. **It duplicates work the AI system needs anyway.** The agent reads and writes pages as markdown ([AI Serialization Format v2](2026-03-25-ai-serialization-format-v2.md)); a markdown-to-blocks compiler is on that roadmap regardless. Authoring templates in a separate node-tree format means building and maintaining two paths into the same block model.

There is also a runtime gap: today the template `body` is parsed and discarded (only `meta` is consumed, for the catalog endpoint), no importer turns a template into a Page, and the selected system's `bundle` is read-only wizard decoration that campaign creation never consults. A new campaign is born with one empty home page and zero templates.

This doc resolves the format and defines the path that closes those gaps.

---

## TL;DR (the decisions)

1. **Two surfaces, split by nature.** `systems.yaml` stays as catalog _config_ (ids, colors, the `popular` flag, the `bundle`, and short localized labels). Template _bodies_ become **markdown**.
2. **A template is a pure cloneable skeleton.** It is a locale-invariant set of sections with short placeholder prompts. Authoring guidance does not live on the template; it lives in a **skill** loaded at clone time (defined and localized once). Reference and learn prose ("what is an NPC") also moves to skills.
3. **Localization is per-locale files, not per-string maps.** Each template is a set of markdown files, one per locale, in the template's own directory (`common/npc/npc.en.md`, `common/npc/npc.fi.md`, ...), with `en` required and canonical for invariants. Prose is localized; structure and directives are not.
4. **The format is a graph-independent subset of the agent markdown dialect.** Title (frontmatter), preamble-by-position, native `#` sections and headings, paragraphs, visibility marks, `#tags`, and `{placeholder}` text. The graph-coupled parts of the dialect are excluded (see _Format_ below).
5. **Visibility is fail-closed.** Unmarked content is `gm_only`; `[known]` is the explicit reveal. A forgotten mark hides, it never leaks (see _Visibility_ below).
6. **The compiler is the shared foundation for the agent's `f-inverse`.** Build markdown-to-blocks once, scoped to this subset, structured so the AI write path reuses it later.
7. **The bundle is consumed at campaign creation.** On first checkout, the selected system's `bundle` is instantiated as `template`-kind Pages, resolved to the campaign's content locale.

---

## The two surfaces

`content/` carries two different kinds of thing, and only one of them benefits from markdown.

**Catalog config (`systems.yaml`) stays structured.** A game system is ids, a hex color, a `popular` flag, a `bundle` of template slugs, and short labels (name, tagline). This is maintainer configuration that legitimately looks like configuration; markdown would not improve it. Its short labels can stay `LocalizedString` maps, since a one-line tagline has no structural-parity problem. (Whether to also move these to per-locale files is an open thread, leaning no.)

**Authored template bodies become markdown.** This is the surface a contributor writes and the surface the three problems above are about. The rest of this doc concerns it.

---

## What a template is

A template is a **cloneable skeleton**: the ordered sections and the short placeholder prompts inside them ("{What do people notice first?}"). Cloning an entity from the template reproduces this structure as the new page's starting point. The skeleton is what `template_id` lineage points back to ([Templates as Pages](2026-02-20-templates-as-pages.md)).

**Authoring guidance is not on the template; it is a skill.** The "how to write a good NPC" prose (what the preamble should cover, what defaults to secret, what tone to take) is defined **once** in a skill and loaded when an entity is cloned, rather than copied into every template and every locale. The original intent ("reprompt the agent on clone") is preserved; only the mechanism moves: the clone path ensures the relevant skill is loaded, via the create toolcall's return value or a hook. The binding from a template to its skill is a future `OnCreate` skill reference (see _Deferrals_); it is not required on day one, but the format reserves it as a stated goal. Skills are not a built `PageKind` yet, so v0 templates ship skeleton-only.

**Reference and learn prose also moves to skills.** The current template bodies contain encyclopedic prose explaining what an NPC or a Crew is. That prose _is_ technically cloneable, but it is not worth cloning: it would open every new entity with paragraphs about the kind in the abstract, and a skill defines it once and saves context. So it is parked as draft `content/skills/*.md` for when the skill importer lands, rather than discarded.

---

## Localization

The format keys localization at the **file**, not the string. A template is a set of markdown files, one per locale, in the template's own directory and named by locale tag (`common/npc/npc.en.md`, `common/npc/npc.fi.md`), with `en` required as the base. The slug is the directory path (`common/npc`); the locale files live inside it, and the importer keys lineage and `bundle` references on that slug, not on the filename.

What is localized and what is not follows from one observation: **the section structure is driven by the game system, not by the language.** An NPC has the same sections whether authored in English or Finnish; only the words change. So:

- **Locale-invariant** (one source of truth, canonical in `en`): the section structure, the `OnCreate` tag, the future `OnCreate` skill reference, the icon, the slug, and everything in `systems.yaml`.
- **Per-locale prose:** the name and description, and the placeholder prompts.

Because the structure is invariant, a translator fills prose against a fixed section list rather than redesigning the document, and "clone the structure" stays meaningful across locales. A lint that enforces structure parity across a template's locale files is worth adding when a second locale exists; today everything is `en`, so it is deferred.

### Why prose is localized

The placeholder prompts seed a cloned page that the GM and the agent then fill in. In a Finnish campaign they should be Finnish, so the agent is not code-switching between Finnish content and English prompts. The research on this is consistent:

- Using an English prompt to process or generate non-English text creates a cross-lingual mismatch that lowers accuracy, and prompting in one language while expecting output in another triggers the off-target language issue (the model drifts back toward the instruction language).
- For generation tasks specifically, matching the prompt language to the desired output and its cultural cues beats English-only prompting, on fluency and on cultural appropriateness.

The same rationale is why the authoring **skills** (which carry the guidance prose) are localized in their own per-locale files. The corollary holds in both places: **structured directives are not prose and are not translated.** `OnCreate` tags, the skill reference, the icon, and section identifiers are machine config; translating them buys nothing and invites drift. The split above puts each on the correct side of that line.

---

## Format

The template markdown is a **graph-independent subset** of the agent markdown dialect defined in [AI Serialization Format v2](2026-03-25-ai-serialization-format-v2.md). Reusing that dialect is the point: it is what the compiler will consume from the agent anyway.

In scope for the subset:

- The frontmatter title and the page metadata.
- **Preamble by position:** the prose between the frontmatter and the first `#` is the preamble section.
- Native `#` sections and nested headings, and paragraphs.
- Visibility marks (`[known]`), fail-closed (see _Visibility_).
- `#tags`.
- `{placeholder}` prompt text, kept literal (a template ships prompts, not filled content).

Deliberately excluded, because they are graph-coupled and depend on systems that are not built (the linker and the RelationshipGraph):

- `{Name}` wiki-link resolution.
- `<relationships>` blocks (the graph-derived agent-context block).
- Suggestion marks (`<prior>` / `<suggestion>`).

Page metadata (name, description, icon, `OnCreate`) lives in **YAML frontmatter** at the top of each locale file. The body below the frontmatter is the markdown skeleton, and the importer compiles only the body into blocks; frontmatter drives the catalog, the page meta, and lineage.

### Frontmatter is the identity block

The title is not a heading; it is `meta.title`, a field, and it lives in frontmatter with the rest of the page's identity (description, icon, `OnCreate`). Because the title is a field, the body owns the full heading range: `#` is the top section, `##` nests under it. That matches the agent serialization (frontmatter identity, native-heading body), so the template body and the agent write path compile through one markdown-to-blocks core at the same levels. The dialect this format subsets, including page-level status as a frontmatter field and the uniform inline `[known]`, is owned by [AI Serialization Format v2](2026-03-25-ai-serialization-format-v2.md).

Frontmatter is typed and extensible, not a fixed set. The rule that keeps it disciplined: it holds page facets with **no per-item visibility that are not content** (an `aliases` list the linker resolves against, say, or a system bundle's stat scalars). Anything that can be secret per item stays inline under the uniform `[known]` token, the way `#tags` do.

### Worked example (`templates/common/npc/npc.en.md`)

```md
---
name: NPC
description: That one person you met. Or centaur. Or whatever.
icon: person-standing
onCreate:
    tag: NPC
    # skill: npc-authoring   (future: ensure this skill is loaded on clone)
---

{Who is this person? Their role, their affiliations, and what makes them matter if they come up unexpectedly. A paragraph or two, 1000 words or less, shorter is better.} [known]

{Any secret worth keeping off the public index card. Counts toward the word limit.}

# Appearance [known]

{What do people notice first?} [known]

# Personality [known]

{How do they behave? What do they want?} [known]

# Secrets

{What the players do not know.}

# History [known]

{How they got here.} [known]

## Session 1 [known]

{A sample session entry. Put session notes here.} [known]

# GM Notes

{Running notes, plans, future hooks.}
```

Every revealed node carries its own `[known]`: revealing does not cascade, so the heading and the prompt beneath it are each marked (a node is visible only if it and all of its ancestors are revealed). The secret sections (Secrets, GM Notes) and the secret preamble line carry no mark at all, so they are hidden by default. Nothing has to be marked `[gm_only]`: hidden is what unmarked already means, and a forgotten `[known]` leaves content hidden rather than leaking it.

### Section mapping

The page kind declares the sections; the markdown fills them by position. There are only **two** sections for `template` (and the entities it clones to): `PageKind::Template.sections()` is `[Preamble, Body]`. The many `#` headings (Appearance, Personality, the sample `## Session 1`) are **headings inside the `body` section, not sections themselves** (the container-vs-heading distinction is owned by [Multi-Section Document Structure](2026-06-07-multi-section-document-structure.md)). So:

- Prose before the first `#` compiles into the `preamble` section.
- Everything from the first `#` onward compiles into the `body` section.

The positional split therefore maps one-to-one onto the two real containers; nothing explicit is hidden, because there are only two sections and the markdown distinguishes both. Explicit section demarcation only becomes necessary for a kind with **more than two** containers (a future Session template: prep / summary / transcript / journal); that is deferred until such a template exists. The relationship panel is **not** part of the template format: the editor renders it from the page's graph edges, so baking a widget block into templates would couple them to that widget's representation and force a template migration if it ever changed. The preamble/body boundary stays purely positional (the first `#`).

---

## Visibility

Visibility is **fail-closed**, the same convention as the agent markdown dialect ([AI Serialization Format v2](2026-03-25-ai-serialization-format-v2.md)). Unmarked content is `gm_only`. `[known]` is the explicit, affirmative reveal. A node is shown to players only if it and every ancestor are marked `[known]`. Revealing does not cascade (each node is marked); hiding does (an unrevealed ancestor hides its subtree). The point of the direction: a forgotten mark leaves content hidden, never exposed. For a template, authored once and cloned many times, the inverse (mark the secret, default visible) would leak in every clone made from a template with a missed mark.

This keeps faith with the `gm_only` storage default (a cloned page is hidden until the GM reveals it) and puts the explicit act on the consequential decision (revealing), not on the safe one (hiding).

One naming note, flagged not resolved: `[known]` is the existing `Status` token and is used here to stay consistent with the code. `player_visible` reads clearer and would be a reasonable rename, but that is a global change (the `Status` enum, the glossary, and both docs) owned by the visibility model, not made here.

---

## Compilation and import

### Markdown to blocks (the shared core)

A markdown-to-blocks compiler turns a template's body into the rows page genesis already understands: a sequence of `(Section, blob)` where the blob is the existing at-rest block shape (`{ nodeName, attributes (including a freshly minted block id), children }`, the output of the block codec). This is the same transformation the agent write path (`f-inverse`) will need, so it is built once as a shared, graph-independent core and the AI path layers the excluded dialect features on top later.

A known limitation rides along: the block codec is plain-text only today (it drops rich-text marks), so v0 templates are headings and plain paragraphs. Bold, italic, and links wait on the codec's marks work and do not block this effort.

### Genesis

Page genesis already has a single constructor that buckets `(Section, blob)` rows into the kind's declared section containers (`LoroPageDoc::from_blocks`), but every page is currently born with empty sections because the genesis builder feeds it no rows. Importing a template means feeding the compiled rows into that builder, through the page's owning actor. Two existing seams carry this:

- The genesis builder (`build_new_page` / `build_seeded_page`) gains the ability to accept seeded rows and a `template_id`, per its own standing TODO.
- The mutation flows through `PageActor` and the supervisor's create workflow. Nothing writes a page's rows around its owning actor; that invariant holds for imports too.

### Template instantiation (clone)

Creating an entity from a template (`from_template_id`, currently a 501) clones the template page's blocks: deep-copy each block, mint a fresh block id, preserve its `section` and visibility, reset per-section ordering, and set `template_id` on the new page for lineage. There is no guidance to clone (it lives in a skill); the clone path instead ensures the relevant authoring skill is loaded, once that binding exists.

### Campaign creation

On a campaign's first checkout, the selected system's `bundle` is instantiated as `template`-kind Pages, with names and prose resolved to the campaign's content locale (`en` fallback). This replaces today's behavior, where the wizard's system selection is recorded but the bundle is never instantiated. Threading the selected system id and locale from the onboarding wizard into campaign creation is the connecting work (see _Open threads_).

---

## Deferrals

- **Rich marks in template prose.** Gated on the block codec's marks work; v0 is headings and plain paragraphs.
- **Skills.** `Skill` is not a `PageKind` yet. The authoring guidance and reference prose belong in skills, and the `OnCreate` skill reference that binds a template to its skill is a stated future goal, not a v0 requirement. Stripped reference prose is parked as `content/skills/` drafts in the meantime.
- **Structure-parity lint** across a template's locale files. Add when a second locale exists.

---

## Open threads

- **Wizard to creation wiring.** The selected system id (and locale) must reach campaign creation so the `bundle` can be instantiated; today the bundle is read-only wizard decoration.
- **`systems.yaml` labels.** Keep the system name and tagline as per-string `LocalizedString` maps, or move them to per-locale files for consistency? Leaning keep, since they are short config strings with no structural-parity problem.
- **Visibility token name** (`known`, or a clearer `player_visible` via a global rename): flagged under _Visibility_, owned by the visibility model. The fail-closed convention itself is now shared with the serialization doc.
- **Template-to-skill binding.** The shape of the `OnCreate` skill reference and the load mechanism (toolcall return vs hook) is settled in intent, deferred in detail until skills are built.
- **Alias visibility.** Aliases ship as a public frontmatter list, visible with the page. A secret alias (a concealed identity distinct from the public name) needs per-item visibility and would move to an inline form like tags; deferred until concealed identities are a real feature.

---

## References

**Internal**

- [Templates as Pages](2026-02-20-templates-as-pages.md): templates are pages of kind `template`; `template_id` lineage; structure cloning.
- [AI Serialization Format v2](2026-03-25-ai-serialization-format-v2.md): the agent markdown dialect this subset borrows from; the template / agent-instructions sketch this doc reconciles.
- [Multi-Section Document Structure](2026-06-07-multi-section-document-structure.md): the section model, the container-vs-heading distinction, and preamble-by-position the import targets.
- [Campaign Creation Architecture](2026-05-22-campaign-creation-architecture.md): the catalog and creation flow the bundle consumption plugs into.

**External (localization rationale)**

- [Beyond English: Prompt Translation Strategies across Languages and Tasks](https://arxiv.org/pdf/2502.09331)
- [Multilingual Prompting for Improving LLM Generation Diversity](https://arxiv.org/html/2505.15229v2)
- [Why Your LLM Prompts Should Match Your Content Language](https://ryanstenhouse.dev/why-your-llm-prompts-should-match-your-content-language/)
- [Language-Specific Neurons: Multilingual Capabilities in LLMs](https://arxiv.org/pdf/2402.16438)
