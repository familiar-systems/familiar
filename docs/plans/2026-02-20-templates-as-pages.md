# familiar.systems - Templates as Pages

## Context

During initial implementation of `@familiar-systems/domain`, the template system was modeled as a separate entity type (`Template` + `TemplateField[]`) with a typed field schema. A concrete walkthrough of what an NPC page actually looks like revealed that this model is wrong - it creates an artificial split between "template structure" and "page content" that doesn't match how GMs think or work.

This document captures the corrected design and the reasoning behind it.

## The Problem with Separate Templates

The original model:

```
Template {
  id, campaignId, slug, label, icon
  fields: TemplateField[]    // { slug, label, type, options, required, sortOrder }
}

Page {
  id, campaignId, templateId, name, status
}
```

Template defines the schema. Page holds the data. Block holds the content. Three separate concepts, three separate entity types, and the question: **who defines the page layout?**

Walking through "Graydalf the Wisened" (an NPC) makes the problem concrete:

- **Portrait** on the right - is this a template field? A block? A layout instruction?
- **Relationship list** - married to Sabrina (deceased), dean of Hogwurtz - this is graph data, but the GM put it _here on the page_ and might want it above or below the stat block
- **Transcluded stat block** from the Archmage template - a reference to another page's content, embedded inline
- **Freeform narrative** below - the blurb about Graydalf

The original model has no answer for where the page layout lives. Template defines data fields. Blocks are flat ordered content. There's a gap between "what fields exist" and "how the page is arranged" that neither entity covers.

Possible patches - a layout JSON on Template, tree-structured blocks, a separate LayoutDefinition entity - all add complexity to solve a problem that shouldn't exist.

## The Insight: Templates Are Pages

A template is a page you clone from (it behaves like a prototype in the prototypal-inheritance sense: you copy it to make instances). The "NPC template" is itself a page - with a portrait placeholder, a relationship list widget, a stat block transclusion slot, and a freeform content zone - that looks exactly like what an NPC page should look like.

When a GM creates "Graydalf the Wisened" from the NPC template, the system clones the template page's block structure. Graydalf's page starts as a copy of the NPC template page, with placeholders ready to fill in.

**This means:**

1. **Templates are pages.** A template is a page of kind `template`. There is no separate Template entity type.
2. **The template editor IS the page editor.** GMs customize templates by editing them as pages. No separate template-builder UI. No field-type selectors. The GM sees a page and arranges it however they want.
3. **Layout is content.** The arrangement of portrait, relationships, stat block, and narrative on the page is part of the page's block structure - not metadata on a schema entity. The page IS the layout.
4. **GMs own their templates.** Templates are campaign-scoped, created and edited by GMs, cloned from starter packs (or copied from our hosted content hub). The original model already said this, but modeling templates as separate entities implied the template structure was somehow different from page content. It's not.
5. **No structured field schema needed.** The AI's semantic search (RAG) over block content IS the query layer. "Show me all chaotic evil NPCs" is a natural language query resolved by the AI against block content, not a SQL `WHERE` clause against typed field columns. This eliminates the entire category of structured-field machinery (field types, select options, validation rules, queryable indexes) that drove the original `TemplateField` design.

## How It Works

### Template creation

A GM creates or customizes a template by editing it as a page. The NPC template page might contain:

- A columns layout with a portrait placeholder on the right
- A relationship list widget (dynamic - renders this page's graph edges)
- A transclusion slot (for embedding another page's content, e.g. a stat block)
- A freeform content zone with placeholder text ("Write about this NPC...")

These are all blocks (or structures within blocks) using custom TipTap node types that the editor understands.

### Creating an entity from a template

When the GM creates a new NPC, the system:

1. Clones the template page's block structure
2. Creates a new entity page linked to the template (for lineage, not for schema enforcement)
3. The new page looks like the template, with placeholders ready to fill

### Starter packs

A starter pack for D&D 5e ships a set of template pages: NPC, Location, Item, Faction, Monster, etc. These are cloned into the campaign when it's created. The GM can then edit them freely - the starter pack templates are just the starting point.

### Template hub

At some point, it makes sense to let users upload their own templates to a shared repository.

## What This Changes in the Domain Model

### Removed

- `Template` - no longer a separate entity type
- `TemplateField` - no longer exists; field structure is block content
- `TemplateFieldType` - gone
- `TemplateId` - gone (templates are pages, identified by `PageId`)

### Changed

- `Page` carries a `kind: PageKind` field; `template` is one kind (alongside `entity`). Template-ness is a kind, not a separate type or a boolean flag.
- `Page` _may_ reference its source template via `templateId?: PageId` (for lineage tracking - "this was cloned from that template")
- Block structure may need to support nesting or rich layout (columns, widgets) - this is an editor-layer decision, not a domain-layer decision

### Unchanged

- `Block`, `Status`, `Relationship`, `Mention`, `Suggestion`, `AgentConversation` - all unchanged
- The suggestion system still proposes creating entities, and those entity pages still reference a template (now a template page) for the initial block structure

## Categorization and Tags

### Template lineage: `templateId`

"Show me all NPCs" is the most common categorization query. Since every entity cloned from a template carries `templateId: PageId`, this is a trivial lookup - find all pages whose `templateId` points to the NPC template. No tags, no extra fields. Template lineage IS the primary categorization.

### Cross-cutting tags: relationships to tag-pages

`templateId` is single-valued and possibly not even necessary - it answers "what template was this cloned from?" but not "this NPC is also a Villain, a Quest Giver, and Deceased." Cross-cutting tags use the existing graph:

- A tag is a page. The "Villain" tag is a page named "Villain." It can optionally have its own content ("what makes someone a villain in this campaign?"), or it can be a bare named node.
- Tagging is a relationship: `Graydalf -[tagged]-> Villain`.
- Tags have status. A `gm_only` tag relationship means players don't see the classification.
- Tags show up as backlinks: navigating to the "Villain" page shows all pages tagged as villains.
- The AI can propose tags via the existing suggestion system (propose a `tagged` relationship).

**Why not a dedicated `tags: string[]` field?** A flat string array creates a parallel universe that doesn't participate in the graph. Tag-strings don't have status, don't have pages, don't appear in backlinks, and can't be proposed by the AI through the suggestion system. Making tags into pages-with-relationships means the entire existing machinery (status, suggestions, backlinks, RAG, CRDT sync) works for tags with zero new domain concepts.

**UI concern:** The relationship panel for a page would show narrative relationships ("married to Sabrina") alongside structural ones ("tagged Villain"). This is a UI filtering problem - display `tagged` relationships as chips/badges rather than in the main relationship list. The data model doesn't need to distinguish them; the label does.

### Relationship labels as the only discriminator

Both `templateId` and tag-relationships use labels to distinguish structural from narrative connections:

| Query                         | Mechanism                                                              |
| ----------------------------- | ---------------------------------------------------------------------- |
| "Show me all NPCs"            | `templateId = NPC template PageId`                                    |
| "Show me all Villains"        | Relationship where `label = 'tagged'` and `targetId = Villain PageId`  |
| "Who is Graydalf married to?" | Relationship where `label = 'married to'`                              |
| "What are Graydalf's tags?"   | Relationships where `label = 'tagged'`                                 |

No new domain primitives. The graph handles everything.

---

## Open Questions

### Block structure: flat list vs. tree

The template-as-page model means page layout lives in blocks. A page with columns, a portrait on the right, and a relationship list below needs some way to express that structure. Options:

1. **Inside ProseMirror content** - a single Block's `content: JsonValue` contains a TipTap document with custom nodes (columns, widgets, transclusion references). The domain model stays flat; the editor interprets the rich structure.
2. **Block nesting** - blocks can contain child blocks (`parentBlockId?: BlockId`). The domain model is a tree. Status, source attribution, and mention targeting work at any level.
3. **Hybrid** - blocks are flat, but some block types are "layout containers" that reference other blocks by ID (not parent-child, but explicit composition).

This decision belongs to the editor package design, not the domain package. The domain needs to support whatever the editor decides, but shouldn't prematurely commit to a structure.

### Template evolution

When a GM updates the NPC template (adds a "Motivation" section), what happens to existing NPCs created from that template? Options: nothing (existing NPCs are snapshots), opt-in sync, or diff-and-suggest. This is a product decision, not an architecture decision, and doesn't need to be resolved now.

## Relationship to Other Design Documents

- **[Vision](../vision.md)** - says templates give entities structure and ship with starter packs. This document clarifies that templates ARE pages, not a separate concept.
- **[Project Structure](2026-03-26-project-structure-design.md)** - shared types are generated from Rust via ts-rs, `packages/editor` defines the TipTap schema. The block structure question (flat vs. tree) will be resolved when designing the editor package.
- **[AI Workflow](2026-02-14-ai-workflow-unification-design.md)** - suggestions that create entities still reference a template for initial structure. The template is now a `PageId` rather than a `TemplateId`.
