# Loreweaver — BlockNote Editor Evaluation

## Context

[BlockNote](https://www.blocknotejs.org/) is a block-based rich text editor for React, built on top of **TipTap**, which is built on top of **ProseMirror**. It provides Notion-style editing (slash commands, drag handles, block type switching) with pre-built UI components out of the box.

This document evaluates BlockNote against Loreweaver's specific editor requirements. For the broader stack analysis, see the [stack exploration](../stack_exploration.md). For the TipTap and Lexical evaluations, see [tiptap.md](./tiptap.md) and [lexical.md](./lexical.md).

---

## Licensing: What's Actually Free

BlockNote has a split licensing model. This matters because Loreweaver uses [AGPL-3.0](https://www.gnu.org/licenses/agpl-3.0.html), which is GPL-3.0 compatible (AGPL is a superset of GPL-3.0).

### Core packages — MPL-2.0

The core editor (`@blocknote/core`, `@blocknote/react`, `@blocknote/mantine`, `@blocknote/shadcn`) is licensed under MPL-2.0. This is file-level copyleft: you can use BlockNote in a larger AGPL-licensed project, but modifications to BlockNote's own source files must be shared. Unmodified use is fine. Using BlockNote as a dependency without forking it has zero licensing friction.

**What the core gives you:**

- Block-based editing (paragraphs, headings, lists, code blocks, quotes, dividers, tables, images, video, audio, files)
- Slash menu (type `/` to insert block types)
- Formatting toolbar (bold, italic, etc.)
- Drag handles and block reordering
- Side menu for block operations
- Custom block types via `createReactBlockSpec`
- Real-time collaboration (Yjs, same as TipTap)
- Mentions
- Full TypeScript support

This is a substantial editor. For many apps, the core is the whole product.

### XL packages — GPL-3.0 or $390/month

The XL packages (`@blocknote/xl-*`) are dual-licensed: GPL-3.0 (free for GPL-3.0 projects) or commercial license ($390/month Business tier) for everything else.

**What XL adds:**

- AI integration (context-aware completions, inline editing suggestions)
- Multi-column layouts
- PDF, DOCX, ODT export

**AGPL-3.0 is GPL-3.0 compatible**, so Loreweaver could technically use the XL packages under the GPL-3.0 grant. However, the $390/month commercial license remains the alternative. For a solo part-time developer, neither the cost nor the features justify adopting XL — especially because the AI features aren't the right AI for Loreweaver anyway (see below).

### The AI licensing question specifically

BlockNote's AI features are generic text completion and editing — "make this shorter," "continue writing," "fix grammar." They're powered by an LLM the developer configures.

Loreweaver's AI is campaign-aware: entity extraction against the campaign graph, journal drafting with narrative context, relationship proposal, contradiction detection. BlockNote's AI package cannot do any of this. Loreweaver needs its own AI pipeline regardless (already designed in the [audio pipeline doc](../../audio_ingest/audio_overview.md)).

**Building our own AI-in-editor components** means: custom TipTap/BlockNote blocks that trigger Loreweaver's AI pipeline and render results inline. This is the same custom block work you'd do in TipTap (a React node view that calls your API and displays the result). BlockNote's XL AI package doesn't help — it's solving a different problem.

**Bottom line:** Ignore XL entirely. The core is what matters. Evaluate it on its own merits.

---

## Architecture: Abstraction on Abstraction

BlockNote's stack is three layers deep:

```
BlockNote  (block model, UI components, slash menu, drag handles)
    ↓
TipTap     (extension API, schema management, React bindings)
    ↓
ProseMirror (document model, transactions, decorations, plugins)
```

This is both the strength and the weakness. You get a polished editing experience with minimal code. But when you need something the abstraction doesn't expose, you're reaching through two layers.

---

## How It Maps to Loreweaver's Requirements

### 1. Block-based content

This is BlockNote's primary selling point. The document model is inherently block-native:

```typescript
const document = [
    { type: "heading", props: { level: 2 }, content: "The Rusty Anchor" },
    {
        type: "paragraph",
        content: [
            { type: "text", text: "The party met " },
            { type: "mention", props: { id: "npc:kael" } },
            { type: "text", text: " at the bar." },
        ],
    },
    { type: "statBlock", props: { entityId: "npc:kael" } },
];
```

Each block is a typed JSON object with props and content. Custom blocks are defined with `createReactBlockSpec`:

```typescript
const StatBlock = createReactBlockSpec(
  {
    type: "statBlock",
    propSchema: {
      entityId: { default: "" },
    },
    content: "none",
  },
  {
    render: ({ block }) => <StatBlockCard entityId={block.props.entityId} />,
  }
)
```

**Assessment:** Excellent fit for the data model. Blocks are first-class, typed, and the JSON representation is clean. The block model maps more directly to database rows than TipTap's ProseMirror document tree.

### 2. Inline entity mentions

BlockNote includes mentions in the core. Slash menu integration, autocomplete, custom rendering.

**Assessment:** Comparable to TipTap's Mention extension. Both are production-ready.

### 3. Transclusion

Same pattern as TipTap: custom block with `content: "none"` that renders a React component fetching the source block. BlockNote's `createReactBlockSpec` makes this straightforward.

**Assessment:** Equivalent to TipTap. Custom work either way.

### 4. Status visualization — the critical question

This is where the abstraction layers bite.

Loreweaver needs ProseMirror **decorations** to overlay status indicators on blocks without mutating the document. This is a ProseMirror-level concern — it lives in the plugin/decoration API, two layers below BlockNote.

**Can you access ProseMirror decorations from BlockNote?**

Technically yes. BlockNote's custom block API accepts extensions that can include ProseMirror plugins. And BlockNote exposes the underlying TipTap editor instance. So you can register ProseMirror plugins that add decorations.

But this is an escape hatch, not a supported path. You're writing ProseMirror plugin code (the same code you'd write in raw TipTap) while also working within BlockNote's block abstraction. The two models don't always compose cleanly — BlockNote wraps blocks in its own DOM structure (for drag handles, side menus, etc.), and decorations need to target the right DOM nodes within that structure.

**Assessment:** Possible but awkward. You'd use BlockNote for the nice UI, then immediately break through it for the ProseMirror features that make Loreweaver's editor special. With raw TipTap, decorations are a first-class concept, not an escape hatch.

### 5. Source linking

Same as TipTap: custom attributes on blocks + widget decorations for timestamp indicators. The decoration caveat from #4 applies.

### 6. Collaborative editing

BlockNote uses Yjs (same as TipTap's Collaboration extension). Same self-hosted Hocuspocus server. No difference in capability.

---

## What BlockNote Gives You That TipTap Doesn't

The honest answer: **pre-built UI**.

| Feature              | TipTap                                          | BlockNote                                       |
| -------------------- | ----------------------------------------------- | ----------------------------------------------- |
| Slash menu           | Build it yourself (or use a community package)  | Built-in, animated, filterable                  |
| Drag handles         | Build it yourself                               | Built-in                                        |
| Block type switching | Build it yourself                               | Built-in (click block type to change)           |
| Formatting toolbar   | Build it yourself (or use TipTap's starter kit) | Built-in, positioned automatically              |
| Side menu            | Build it yourself                               | Built-in                                        |
| Theming              | CSS yourself                                    | Mantine or shadcn/ui integration out of the box |

For a developer who has never done frontend, these UI components save real time. Building a slash menu with keyboard navigation, proper positioning, and animation from scratch in TipTap is a day or two of work. BlockNote gives it to you for free.

---

## Sharp Edges

### 1. Three layers of abstraction

When something goes wrong or you need custom behavior at the ProseMirror level, you're debugging through BlockNote → TipTap → ProseMirror. Error messages and stack traces traverse three libraries. Documentation for your specific problem might exist in any of the three layers' docs, or in none of them.

For basic editing, you never see this. For Loreweaver's custom requirements (status decorations, custom mention behavior, source linking), you'll be in this territory regularly.

### 2. Custom block API constraints

BlockNote's custom block props are limited to **primitive types** (boolean, number, string). You can't have a prop that's an object or array. For a stat block that carries structured data, you'd need to serialize to a string prop and parse it back.

TipTap's node attributes have the same limitation (they must be JSON-serializable), but TipTap's node views give you full control over the React component and its data fetching, without the prop schema constraint.

### 3. API instability

BlockNote v0.43.0 (December 2025) removed the `BlockNoteExtension` class in favor of `createExtension`, a major breaking change. The project is pre-1.0 and actively evolving. The API surface you build on today may change.

TipTap is also not immune to this (v2 → v3 transition), but TipTap's core API has been more stable, and ProseMirror beneath it has been rock-solid for ~10 years.

### 4. React-only

BlockNote's UI components are React-specific. If you ever wanted to support a different frontend framework, the editor component library doesn't port. (TipTap has React, Vue, and Svelte bindings; ProseMirror is framework-agnostic.)

Not a current concern — the stack recommendation is React — but a harder lock-in than TipTap.

### 5. Smaller community

BlockNote has ~8k GitHub stars. TipTap has ~30k+. ProseMirror's forum has a decade of solved problems. When you hit an edge case, the odds of someone having documented the answer are lower with BlockNote.

---

## The Real Tradeoff

This is not "BlockNote vs TipTap" in the abstract. It's a specific question:

**Is the pre-built UI (slash menu, drag handles, formatting toolbar) worth the cost of an extra abstraction layer between you and the ProseMirror features Loreweaver needs?**

Arguments for BlockNote:

- Dramatically faster to a working prototype. Slash menu, drag handles, block reordering — all free.
- The Notion-style UX is exactly what a campaign notebook should feel like.
- You've never done frontend. Pre-built components reduce the surface area you need to learn.
- The core is MPL-2.0 and free. No licensing issues.

Arguments for raw TipTap:

- Status decorations are a first-class concept, not an escape hatch.
- When you need custom ProseMirror behavior (and you will, regularly), there's one layer of abstraction, not two.
- Larger community, more stable API, more documented solutions to edge cases.
- The UI components BlockNote gives you (slash menu, toolbar) are buildable in TipTap — it's upfront work, not impossible work.
- You're not locked into BlockNote's opinionated block DOM structure for your custom rendering.

---

## Verdict

**BlockNote's core (free, MPL-2.0) is a legitimate option**, especially for accelerating early development. The Notion-style UX out of the box is genuinely valuable for a solo developer learning frontend.

**The XL packages are irrelevant.** Although AGPL-3.0 is GPL-compatible (making the XL GPL grant usable), the AI features solve a different problem than Loreweaver's campaign-aware AI pipeline. Ignore them.

**The risk is the abstraction tax.** Loreweaver's editor is not a standard notes app. Status visualization via decorations, custom mention behavior wired to the campaign graph, source-linking to audio timestamps — these are ProseMirror-level features. With BlockNote, you'd use the nice UI for the standard parts and break through the abstraction for the custom parts. Whether that's sustainable depends on how much of the editor ends up being custom.

**Two reasonable paths:**

1. **Start with BlockNote** for rapid prototyping, accept that you may outgrow it and migrate to raw TipTap when the custom requirements dominate. The migration is possible (same ProseMirror foundation) but not free — the block model and extension APIs differ.

2. **Start with TipTap** and build the UI components (slash menu, drag handles) yourself or from community packages. More upfront work, but no abstraction layer to fight through for the custom features.

If pressed, I'd lean toward **TipTap** for this project — the custom editor features are not nice-to-haves, they're core to the product. But BlockNote is not a wrong choice for getting something on screen fast, as long as you go in knowing the abstraction will need to be pierced.

---

## Sources

- [BlockNote](https://www.blocknotejs.org/) — block-based editor for React
- [BlockNote GitHub](https://github.com/TypeCellOS/BlockNote) — source code (MPL-2.0 core, GPL-3.0 XL)
- [BlockNote Custom Blocks](https://www.blocknotejs.org/docs/features/custom-schemas/custom-blocks) — custom block API
- [BlockNote Pricing](https://www.blocknotejs.org/pricing) — licensing tiers
- [BlockNote vs TipTap | TipTap](https://tiptap.dev/alternatives/blocknote-vs-tiptap) — comparison (written by TipTap, read with that bias)
- [BlockNote on ProseMirror Forum](https://discuss.prosemirror.net/t/blocknote-open-source-block-based-notion-style-editor-on-top-of-prosemirror/4898) — community discussion
- [BlockNote Review | Velt](https://velt.dev/blog/blocknote-collaborative-editor-guide) — production evaluation
- [Which rich text editor framework should you choose in 2025? | Liveblocks](https://liveblocks.io/blog/which-rich-text-editor-framework-should-you-choose-in-2025) — framework comparison
