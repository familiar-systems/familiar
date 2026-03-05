# Loreweaver — TipTap Editor Evaluation

## Context

[TipTap Editor](https://github.com/ueberdosis/tiptap) is a headless rich text editor framework built on [ProseMirror](https://prosemirror.net/). It provides a developer-friendly API layer over ProseMirror's powerful but low-level primitives: schema-based document model, transaction-based state management, decorations, and a plugin architecture.

This document evaluates TipTap against Loreweaver's specific editor requirements. For the broader stack analysis, see the [stack exploration](../stack_exploration.md). For the Lexical comparison, see [lexical.md](./lexical.md).

### Scope: open-source editor only

TipTap the company sells a cloud platform (collaboration hosting, comments, AI toolkit, DOCX conversion, document history). **This evaluation ignores all of that.** The core editor framework and its open-source extensions are MIT-licensed. Everything Loreweaver needs — the editor, custom extensions, mentions, node views, decorations, and the collaboration extension — lives in the open-source layer.

For self-hosted collaboration, the Yjs protocol is open and the [Hocuspocus](https://github.com/ueberdosis/hocuspocus) WebSocket server is open source. Loreweaver would never touch TipTap Cloud.

**One risk to name and move on from:** TipTap the company is visibly pushing toward cloud revenue. Hocuspocus could receive less investment over time. This is mitigated by Yjs being the underlying protocol — other Yjs servers exist, and the collaboration extension is protocol-level, not server-level. If Hocuspocus stalls, you swap the server, not the editor.

---

## How It Maps to Loreweaver's Requirements

### 1. Block-based content

TipTap's document model is ProseMirror's: a tree of typed nodes defined by a schema.

```
doc
  ├ heading { level: 2 }
  │  └ text "The Rusty Anchor"
  ├ paragraph
  │  ├ text "The party arrived at the tavern where they met "
  │  ├ mention { id: "npc:kael", label: "Kael" }
  │  └ text ", a hooded figure."
  ├ statBlock (custom node view → React component)
  ├ transcludedBlock (custom node view → React component)
  └ aiSuggestion (custom node view → React component)
```

Each block type is an **extension** — a self-contained module that defines the node's schema, parsing rules, rendering, keyboard shortcuts, and commands. TipTap ships ~60 open-source extensions; custom ones follow the same API.

The schema is **strict by design**: content that doesn't match the schema is rejected. You declare exactly what nesting is allowed:

```typescript
// A session journal: contains blocks
const SessionJournal = Node.create({
    name: "sessionJournal",
    content: "(paragraph | heading | statBlock | transcludedBlock | aiSuggestion)+",
});

// A stat block: leaf node, renders as React component
const StatBlock = Node.create({
    name: "statBlock",
    group: "block",
    atom: true, // cannot be edited inline; click to open
    addNodeView() {
        return ReactNodeViewRenderer(StatBlockComponent);
    },
});
```

**Assessment:** Direct fit. Loreweaver's block types (text, headings, stat blocks, transcluded blocks, AI suggestions) map 1:1 to TipTap node extensions. The schema enforces structural validity — you can't accidentally nest a stat block inside a heading.

### 2. Inline entity mentions

TipTap ships a [Mention extension](https://tiptap.dev/docs/editor/extensions/nodes/mention) that provides:

- Inline mention nodes with configurable rendering
- Autocomplete popup with keyboard navigation (up/down/enter)
- A `suggestion` config that accepts an async items function — wire this to entity search
- Custom rendering for the mention chip (show entity type, status indicator, etc.)

```typescript
Mention.configure({
    suggestion: {
        items: async ({ query }) => {
            return await searchEntities(query);
        },
        render: () => {
            // Return popup component for autocomplete
        },
    },
    renderHTML({ node }) {
        return [
            "span",
            { "data-entity-id": node.attrs.id, class: "entity-mention" },
            node.attrs.label,
        ];
    },
});
```

**Assessment:** Production-ready out of the box. The autocomplete drives entity resolution, and each mention node carries the entity ID as an attribute — which becomes the `mention` record in the database. This is the strongest advantage over Lexical, where mentions are a playground reference implementation requiring substantial work.

### 3. Transclusion

No built-in extension, but well-supported by the architecture. A transcluded block is a custom node that:

- Is defined as an `atom` (non-editable inline; click to navigate)
- Carries a `blockId` attribute pointing to the source block
- Renders via a React node view that fetches and displays the source content
- Updates when the source block changes (via subscription or polling)

```typescript
const TranscludedBlock = Node.create({
    name: "transcludedBlock",
    group: "block",
    atom: true,
    addAttributes() {
        return { blockId: { default: null } };
    },
    addNodeView() {
        return ReactNodeViewRenderer(TranscludedBlockView);
    },
});
```

The React component (`TranscludedBlockView`) fetches the referenced block and renders it read-only.

**Assessment:** Custom work, but the extension model makes it clean. The pattern (atom node + React node view + data fetching) is well-documented and used by many TipTap projects for embeds.

### 4. Status visualization (the ProseMirror advantage)

This is where TipTap's ProseMirror foundation pays off. ProseMirror has **decorations** — a rendering layer that sits on top of the document without mutating it.

Three types of decoration, all useful for Loreweaver:

| Decoration type       | Use case                                | Example                                                   |
| --------------------- | --------------------------------------- | --------------------------------------------------------- |
| **Node decoration**   | Style an entire block based on metadata | Dim a GM-only paragraph, strike through a retconned block |
| **Inline decoration** | Highlight a range of text               | Highlight all mentions of a hovered entity                |
| **Widget decoration** | Insert a visual element at a position   | Source-link timestamp indicator at block start            |

```typescript
// Plugin that adds status decorations to blocks
const StatusDecorationPlugin = new Plugin({
    props: {
        decorations(state) {
            const decorations: Decoration[] = [];
            state.doc.descendants((node, pos) => {
                if (node.attrs.status === "gm_only") {
                    decorations.push(
                        Decoration.node(pos, pos + node.nodeSize, { class: "block-gm-only" }),
                    );
                }
                if (node.attrs.status === "retconned") {
                    decorations.push(
                        Decoration.node(pos, pos + node.nodeSize, { class: "block-retconned" }),
                    );
                }
            });
            return DecorationSet.create(state.doc, decorations);
        },
    },
});
```

The CSS does the visual work:

```css
.block-gm-only {
    opacity: 0.6;
    background: var(--gm-only-tint);
}
.block-retconned {
    text-decoration: line-through;
    opacity: 0.5;
}
```

**Assessment:** This is the single strongest reason to choose TipTap over Lexical. Status visualization, mention highlighting, and source-link indicators are all decorations — they drive visual presentation from metadata without touching the document model. Lexical cannot do this (see [lexical.md](./lexical.md)).

### 5. Source linking

Each block node can carry arbitrary attributes. A `sourceRef` attribute stores the audio timestamp:

```typescript
const JournalParagraph = Paragraph.extend({
    addAttributes() {
        return {
            sourceRef: { default: null }, // e.g. "audio:session-42:1:23:45"
            status: { default: "gm_only" },
        };
    },
});
```

A widget decoration at the start of each sourced block renders a clickable timestamp indicator. The indicator is visual-only — it doesn't exist in the document model.

**Assessment:** Straightforward. Attributes for data, decorations for display.

### 6. Collaborative editing

TipTap's [Collaboration extension](https://tiptap.dev/docs/editor/extensions/functionality/collaboration) uses Yjs (a CRDT library) for conflict-free real-time editing. The self-hosted server is [Hocuspocus](https://github.com/ueberdosis/hocuspocus):

- WebSocket-based Yjs sync
- Persistence hooks (save to database on change)
- Authentication hooks
- Redis scaling for multiple server instances
- SQLite or custom storage backends

```typescript
import Collaboration from "@tiptap/extension-collaboration";
import { HocuspocusProvider } from "@hocuspocus/provider";

const provider = new HocuspocusProvider({
    url: "ws://localhost:1234",
    name: `session-${sessionId}`,
});

const editor = new Editor({
    extensions: [Collaboration.configure({ document: provider.document })],
});
```

**Assessment:** Collaboration is not a launch requirement (the GM is the primary editor), but the path exists and is fully self-hostable. When player editing (character sheets, recollections) is added, the infrastructure is ready.

---

## Sharp Edges

### 1. Schema evolution is a real problem for long-lived data

TipTap's schema is strict. Content that doesn't match the current schema is **silently stripped by default**. For a campaign notebook where data is long-lived (years of sessions) and loss is catastrophic, this needs explicit handling.

The scenario: You ship with paragraph, heading, and mention nodes. Six months later you add a stat block node — old documents load fine, they don't contain stat blocks. But if you _remove_ or _rename_ a node type, or change its content rules, existing documents silently lose content on load.

**Mitigations:**

- Enable `enableContentCheck: true` to detect schema mismatches instead of silently stripping
- Handle the `contentError` event to alert or migrate rather than lose data
- Never remove node types from the schema — deprecate them with a read-only rendering
- Store the raw document JSON in the database alongside any editor state, so you can always recover the original
- Version your schema and write migrations (same discipline as database migrations)

This is not unique to TipTap — any schema-enforced editor has this problem. But campaign data spanning years of play makes it higher stakes than most use cases. Plan for it from day one.

### 2. React re-rendering requires discipline

The most common TipTap performance issue is the editor re-rendering too often in React. By default, `useEditor` re-renders on every transaction (every keystroke). If the editor shares a component with other state, unrelated state changes re-render the editor.

**Mitigations (all documented and straightforward):**

- Isolate the editor in its own React component
- Use `shouldRerenderOnTransaction: false` (v2.5.0+) to disable default re-rendering
- Use `useEditorState` hook to subscribe to only the specific state you need (e.g., "is bold active?")

TipTap's docs state that the core "is even able to edit an entire book" — performance bottlenecks are integration patterns, not the editor engine.

### 3. Custom node views have a rendering cost

Each React node view (stat block, transcluded block, AI suggestion card) is a synchronous React render inside the ProseMirror DOM. With many node views in a single document, rendering adds up.

For Loreweaver, a session journal might have 5-20 custom node views. This is well within normal range. Hundreds would be a problem.

**Mitigation:** Lazy-load node view content. The React component mounts immediately (lightweight shell) and fetches data asynchronously.

### 4. ProseMirror is still underneath

TipTap abstracts ProseMirror well for common operations, but for advanced customization (custom decorations, complex input rules, collaborative cursor rendering), you'll read ProseMirror docs and think in ProseMirror concepts. TipTap doesn't replace ProseMirror knowledge — it reduces how often you need it.

This is a feature, not a bug: ProseMirror's power is there when you need it. But expect to invest time in the transaction model, the decoration system, and the plugin API for the custom features Loreweaver requires.

### 5. No React Native

ProseMirror depends on the browser DOM. If Loreweaver ever needs a native mobile editor, TipTap can't help. The vision doc describes a web application — not a current concern, but a hard boundary.

---

## What TipTap Doesn't Solve

These are application-level concerns that sit outside the editor:

- **Block-level permissions**: TipTap doesn't know about GM-only vs Known. The application decides which blocks to include in the editor state for a given user. For the GM: all blocks. For a player: filter to Known blocks before loading.
- **Mention → database sync**: The Mention extension creates inline nodes in the editor. Extracting these into `mention` records in the database is application logic — parse the document JSON, walk the tree, collect mention nodes.
- **Transclusion freshness**: A transcluded block's source may change. The React node view must subscribe to updates or poll. This is a data-fetching problem, not an editor problem.
- **Block-level source refs**: TipTap doesn't know about audio timestamps. The `sourceRef` attribute is opaque metadata. Linking it to an audio player is application UI.

---

## Verdict

TipTap Editor (open-source, MIT) is the recommended editor for Loreweaver. The reasons are specific to the product's requirements:

1. **Decorations** solve the pervasive visual annotation problem (status, mention highlighting, source links) without polluting the document model
2. **The Mention extension** provides production-ready entity references with autocomplete
3. **React node views** handle the custom block types (stat blocks, transcluded blocks, AI suggestions)
4. **The schema system** enforces structural validity on long-lived campaign data (with the caveat that schema evolution needs migration discipline from day one)
5. **Hocuspocus** provides a self-hostable collaboration path when multiplayer editing is needed
6. **The ecosystem** is the largest of any structured editor — more solved problems, more extensions, more community answers

The sharp edges (schema evolution, React re-rendering, ProseMirror learning curve) are real but well-documented and manageable with known patterns.

---

## Sources

- [TipTap Editor GitHub](https://github.com/ueberdosis/tiptap) — source code (MIT license)
- [TipTap Editor documentation](https://tiptap.dev/docs)
- [TipTap Schema](https://tiptap.dev/docs/editor/core-concepts/schema) — document structure and validation
- [TipTap Mention extension](https://tiptap.dev/docs/editor/extensions/nodes/mention) — inline entity references
- [TipTap Node Views (React)](https://tiptap.dev/docs/editor/guides/node-views/react) — embedding React components
- [TipTap Collaboration extension](https://tiptap.dev/docs/editor/extensions/functionality/collaboration) — Yjs-based real-time editing
- [TipTap Performance guide](https://tiptap.dev/docs/guides/performance) — React re-rendering and optimization
- [TipTap Invalid Schema handling](https://tiptap.dev/docs/guides/invalid-schema) — schema evolution strategies
- [Hocuspocus GitHub](https://github.com/ueberdosis/hocuspocus) — self-hosted Yjs collaboration server
- [ProseMirror](https://prosemirror.net/) — underlying editor toolkit
- [Yjs](https://yjs.dev/) — CRDT framework for collaborative editing
- [Which rich text editor framework should you choose in 2025? | Liveblocks](https://liveblocks.io/blog/which-rich-text-editor-framework-should-you-choose-in-2025) — detailed framework comparison
