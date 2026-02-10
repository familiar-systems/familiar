# Loreweaver — Lexical Editor Evaluation

## Context

[Lexical](https://lexical.dev/) is Meta's extensible text editor framework. It takes a fundamentally different architectural approach from ProseMirror/TipTap: the document is a mutable tree of typed nodes, manipulated through update callbacks rather than immutable transactions.

This document evaluates Lexical against Loreweaver's specific editor requirements. For the broader editor landscape and the recommended choice (TipTap), see the [stack exploration](../stack_exploration.md).

---

## What's Appealing

### The tree model maps naturally to blocks

Lexical's document tree aligns with Loreweaver's block-native content model. Each `ElementNode` is conceptually a block, `TextNode` handles inline content with formatting, and `DecoratorNode` embeds arbitrary React components (stat blocks, transcluded blocks, AI suggestion cards).

```
root
  ├ heading
  │  └ text "The Rusty Anchor"
  ├ paragraph
  │  ├ text "The party arrived at the tavern where they met "
  │  ├ mention { entity: "npc:kael" }
  │  └ text ", a hooded figure with news of "
  │  └ mention { entity: "faction:silver-compact" }
  ├ stat-block (DecoratorNode → React component)
  │  └ <StatBlockCard npcId="kael" />
  ├ transcluded-block (DecoratorNode → React component)
  │  └ <TranscludedBlock blockId="grimhollow-desc-1" />
```

The editor state serializes to JSON that structurally resembles the data model. The alignment is real.

### DecoratorNode enables rich embedding

Lexical's `DecoratorNode` returns a React component directly from its `decorate()` method. This is how you'd embed stat blocks, transcluded blocks, and AI suggestion cards inside the editor. The API is straightforward:

```typescript
class TranscludedBlockNode extends DecoratorNode<ReactNode> {
  __blockId: string;

  decorate(): ReactNode {
    return <TranscludedBlock blockId={this.__blockId} />;
  }
}
```

TipTap achieves the same via [node views](https://tiptap.dev/docs/editor/guides/node-views/react), which are slightly more boilerplate but functionally equivalent.

### Plugin model is React-native

Lexical plugins are React components rendered as children of `LexicalComposer`. They hook into the editor via `useLexicalComposerContext()`. For a React-based frontend, this feels natural — plugins follow the same patterns as any other React component.

---

## Sharp Edges

### 1. No pure decorations (the dealbreaker for Loreweaver)

ProseMirror has **decorations** — visual overlays that style content without mutating the document state. Dim a block, highlight a mention on hover, show a source-link indicator — all as a rendering layer, invisible to the document model.

Lexical doesn't have this concept. Its "decorator nodes" are actual nodes in the tree that mutate the document. The distinction matters for Loreweaver's specific requirements:

| Requirement | ProseMirror/TipTap | Lexical |
|---|---|---|
| GM-only blocks dimmed | Decoration driven by block metadata | Must mix visual state into node properties or render DOM overlays manually |
| Retconned blocks struck through | Decoration driven by status field | Same workaround |
| Mention highlighting on hover | Decoration on matching mention nodes | Manual DOM calculation, scroll/resize listeners |
| Source-link indicators | Decoration with timestamp metadata | Manual DOM overlay |
| Collaborative cursors | Decoration (standard pattern) | Calculate cursor positions, draw HTML divs on top of text, listen to scroll/resize |

This is not a missing feature that will be added. It's an architectural choice — Lexical models the document as the single source of truth for both content and presentation. [The Liveblocks team](https://liveblocks.io/blog/which-rich-text-editor-framework-should-you-choose-in-2025) (who build collaborative editing infrastructure and spent months deep in both codebases) flagged this as a fundamental concern:

> One of the main issues in extending Lexical is its lack of pure decorations — the ability to style content without affecting the document itself. While Lexical does have "decorator nodes," they mutate the content of the document.

For an editor where every block has a status, every mention is interactive, and source-linking is pervasive, the lack of pure decorations means every visual annotation requires a workaround. The cumulative cost is high.

### 2. Collaboration has structural limitations

Lexical's Yjs binding hardcodes the root node name, making it impossible to have more than one Lexical editor per Yjs document. The playground examples work around this with separate WebSocket connections per editor — acknowledged as unscalable for production.

This matters if Loreweaver has multiple editor instances on the same page (editing a node's description while previewing related blocks, or a split-pane session prep view).

### 3. No 1.0 release

Lexical has not shipped a stable 1.0. The API surface is still moving. Meta maintains it as long as internal products depend on it — and Meta has a track record of both sustained maintenance (React, 10+ years) and quiet deprecation when priorities shift.

ProseMirror, by contrast, has been maintained by Marijn Haverbeke for ~10 years as his primary livelihood. The API has been stable since 1.0 with very few breaking changes.

### 4. Extension ecosystem gap

TipTap ships production-ready extensions: [Mention](https://tiptap.dev/docs/editor/extensions/nodes/mention) (autocomplete, configurable rendering, keyboard navigation), [Collaboration](https://tiptap.dev/docs/editor/extensions/functionality/collaboration) (Yjs-based real-time editing), [Image](https://tiptap.dev/docs/editor/extensions/nodes/image), [Placeholder](https://tiptap.dev/docs/editor/extensions/functionality/placeholder), and dozens more.

Lexical has reference implementations in the playground. These are starting points, not finished products. The mention plugin, for example, demonstrates the pattern but requires substantial work to reach production quality (custom rendering, keyboard navigation, async entity search, debouncing).

For a solo developer learning frontend, the difference between "install the extension and configure it" and "study the playground source and reimplement it" is significant.

### 5. Documentation gaps

Lexical's documentation has known gaps. The playground source code is often the most complete reference for how to implement features. TipTap's documentation is thorough, with per-extension guides, examples, and API references.

---

## What the tree model doesn't uniquely offer

ProseMirror's document is also hierarchical:

```
doc
  ├ heading
  │  └ text "The Rusty Anchor"
  ├ blockquote
  │  └ paragraph
  │     └ text "A weathered sign creaks above the door."
  ├ paragraph
  │  ├ text "The party met "
  │  ├ text "Kael" [marks: mention(npc:kael)]
  │  └ text " at the bar."
```

The structural mapping to blocks exists in both editors. The difference is how the model is defined (ProseMirror: declarative schema; Lexical: class hierarchy) and manipulated (ProseMirror: immutable transactions; Lexical: mutable update callbacks).

ProseMirror's schema additionally provides **compile-time-like validation**: you declare what nesting is legal (a heading contains inline content, a blockquote contains block content) and the schema rejects invalid states at the model level. Lexical validates through runtime checks in node methods — more flexible, less rigorous.

---

## Verdict

Lexical is a defensible choice for teams that want maximum control and are comfortable building infrastructure. The tree model is genuinely elegant, and the React-native plugin model is clean.

For Loreweaver specifically, **the lack of pure decorations is the critical issue**. The editor needs pervasive visual annotation — status indicators on every block, interactive mentions, source-link markers — that should not pollute the document model. ProseMirror's decoration system was designed for exactly this; Lexical's architecture doesn't support it.

Secondary concerns (collaboration limitations, ecosystem gap, pre-1.0 stability, documentation gaps) reinforce the conclusion but aren't individually decisive.

**Recommendation: TipTap** (built on ProseMirror). It provides the same structural mapping to blocks, the same ability to embed React components via node views, plus pure decorations, a mature extension ecosystem, and thorough documentation.

---

## Sources

- [Lexical](https://lexical.dev/) — Meta's extensible text editor framework
- [Lexical DecoratorNode docs](https://github.com/facebook/lexical/blob/main/packages/lexical-website/docs/concepts/nodes.mdx) — custom node types
- [Lexical Collaboration docs](https://github.com/facebook/lexical/blob/main/packages/lexical-website/docs/collaboration/react.md) — Yjs integration
- [Which rich text editor framework should you choose in 2025? | Liveblocks](https://liveblocks.io/blog/which-rich-text-editor-framework-should-you-choose-in-2025) — detailed comparison including decoration limitations
- [Tiptap vs Lexical | Medium](https://medium.com/@faisalmujtaba/tiptap-vs-lexical-which-rich-text-editor-should-you-pick-for-your-next-project-17a1817efcd9) — feature comparison
- [TipTap Mention extension](https://tiptap.dev/docs/editor/extensions/nodes/mention) — production-ready entity mention support
- [TipTap Node Views (React)](https://tiptap.dev/docs/editor/guides/node-views/react) — embedding React components in the editor
