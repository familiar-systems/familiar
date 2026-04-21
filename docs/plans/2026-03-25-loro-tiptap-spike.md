# Spike: Suggestion Marks on Block UUIDs

**Status:** Proposed
**Date:** 2026-03-25
**Purpose:** Validate the one unproven assumption in the [Campaign Actor Domain Design](./2026-03-25-campaign-actor-domain-design.md) and [AI Serialization Format v2](./2026-03-25-ai-serialization-format-v2.md): that AI suggestions can be modeled as marks on block UUID ranges in a LoroDoc, rendered as inline diffs in TipTap, with blocking semantics on the target blocks.

---

## What's Already Validated

| Concern                                      | Status    | How                                                                                              |
| -------------------------------------------- | --------- | ------------------------------------------------------------------------------------------------ |
| LoroDoc ↔ ProseMirror round-trip             | Validated | Prior integration - `loro-prosemirror` with TipTap, custom schema works                          |
| loro-dev/protocol server-side sync           | Validated | Prior integration                                                                                |
| LoroDoc as conversation (streaming, history) | Validated | `@loro-extended` TypeScript project                                                              |
| Loro + TipTap custom node types              | Validated | Prior integration - unbranded types caused editor confusion, which motivated this design session |
| Room multiplexing                            | Validated | loro-dev/protocol supports this natively                                                         |

**What remains unvalidated:** The suggestion model. Specifically: can we layer suggestion metadata onto block ranges in a LoroDoc, render them as inline diffs in TipTap with accept/reject controls, enforce read-only blocking on target blocks, and handle overlapping suggestions from multiple agent conversations?

---

## Key References

| Resource                                                                                      | Why it matters                                                                                                                                                                  |
| --------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [Campaign Actor Domain Design](./2026-03-25-campaign-actor-domain-design.md)                  | Defines `SuggestionTarget` trait, `Suggestion` struct, the ThingActor's role                                                                                                    |
| [AI Serialization Format v2](./2026-03-25-ai-serialization-format-v2.md)                      | Defines the suggestion model: marks on blocks, blocking, conversation scoping, supersession rules, outcomes table                                                               |
| [loro-prosemirror](https://github.com/loro-dev/loro-prosemirror)                              | The binding we're building on. `LoroSyncPlugin` handles doc ↔ editor sync. We need to understand how it maps custom nodes/marks to determine where suggestion metadata lives.   |
| [TipTap comments](https://tiptap.dev/docs/comments/getting-started/overview)                  | Architectural pattern reference. Comments are marks on ranges with thread data. Our suggestions follow the same pattern. Study the `setThread` / `resolveThread` command model. |
| [TipTap comments editor commands](https://tiptap.dev/docs/comments/integrate/editor-commands) | `setThread`, `removeThread`, `resolveThread`, etc. Our suggestion commands will follow a similar pattern.                                                                       |
| [TipTap extensions guide](https://tiptap.dev/docs/editor/core-concepts/extensions)            | How to build custom nodes, marks, and plugins. The suggestion extension is a custom TipTap extension.                                                                           |
| [ProseMirror marks](https://prosemirror.net/docs/guide/#schema.marks)                         | How marks work on ranges in the document model. Marks can overlap. This is the primitive suggestion marks build on.                                                             |
| [Loro types](https://loro.dev/docs)                                                           | `LoroMap`, `LoroList`, `LoroText`. The suggestion metadata store is likely a `LoroMap` keyed by `SuggestionId`.                                                                 |

---

## The Hypothesis

**Can AI suggestions be modeled as marks on block UUID ranges in a LoroDoc, without modifying the document tree, with multiple overlapping suggestions supported, rendered as inline diffs in TipTap with blocking and accept/reject controls?**

This breaks down into five sub-questions, each building on the last.

---

### Sub-question 1: Where does suggestion metadata live in the LoroDoc?

**Two approaches to evaluate:**

**Approach A - ProseMirror marks:** Suggestions are ProseMirror marks applied to the text ranges of target blocks. The mark carries a `suggestionId` attribute. The suggestion data (proposed content, provenance) lives in a sibling `LoroMap` in the same LoroDoc, keyed by suggestion ID. The mark is the visual/structural anchor. The LoroMap is the data store.

**Approach B - Pure LoroMap, no ProseMirror marks:** Suggestions live entirely in a sibling `LoroMap`. Each entry contains `target_blocks: Vec<BlockId>`, proposed content, and provenance. The TipTap extension reads this map and renders suggestion UI on the appropriate blocks by matching block UUIDs. No ProseMirror mark is applied to the document content at all.

**Approach A is preferable** because ProseMirror marks are the native mechanism for "this range of content has an annotation." The editor's rendering pipeline already handles marks - highlighting, decorations, click handlers. The `loro-prosemirror` binding should sync marks like any other schema element.

**Approach B is the fallback** if marks don't work well for block-level suggestions (marks are traditionally inline - spanning text within a block, not spanning entire blocks). Block-level annotations might need ProseMirror node decorations or a plugin that reads from the LoroMap and produces decorations at render time.

**What to build:**

1. Add a `suggestion` mark to the TipTap schema with a `suggestionId` attribute
2. Add a `suggestions` LoroMap to the LoroDoc (sibling to the document content container)
3. Apply the mark to a paragraph's text range. Write suggestion data to the LoroMap
4. Verify the mark survives sync via `loro-prosemirror` - does `LoroSyncPlugin` handle custom marks with custom attributes?
5. If marks don't work for block-spanning suggestions, try Approach B: read from LoroMap in a ProseMirror plugin, produce node decorations

**Validation criteria:**

- [ ] The chosen approach stores suggestion metadata in the LoroDoc
- [ ] Suggestion metadata syncs correctly between two editors via `LoroSyncPlugin`
- [ ] The document content tree is unchanged by suggestion creation
- [ ] Block UUIDs in the suggestion's `target_blocks` correctly reference blocks in the document

---

### Sub-question 2: Do overlapping suggestions coexist?

**What to build:**

Using whichever approach from SQ1 works:

1. Create suggestion A targeting block P2 (by its BlockId)
2. Create suggestion B targeting blocks P2+P3 (overlapping with A on P2)
3. Verify both suggestions exist in the LoroDoc
4. Verify the document tree is unchanged - P2 and P3 are still normal paragraphs
5. Verify both suggestions render in the editor (both blocks show suggestion UI)
6. Sync to a second editor. Verify both suggestions appear there too

If using Approach A (marks): ProseMirror marks explicitly support overlapping. Two `suggestion` marks with different `suggestionId` values on the same text range should coexist. Verify this.

If using Approach B (LoroMap + decorations): Overlapping is trivial - two LoroMap entries can reference the same block UUIDs without conflict.

**Validation criteria:**

- [ ] Two suggestions can target overlapping block ranges
- [ ] Both suggestions are independently accessible in the LoroDoc
- [ ] Both suggestions render in the editor simultaneously
- [ ] The document content is unchanged

---

### Sub-question 3: Blocking - can target blocks be made read-only?

**What to build:**

A TipTap extension or plugin that:

1. Reads the suggestion data (from marks or LoroMap)
2. Identifies which blocks have pending suggestions
3. Prevents editing of those blocks' content

**Implementation approach:** A ProseMirror plugin with `filterTransaction` that rejects transactions modifying positions within blocks that have pending suggestions. This is the same mechanism used for read-only regions in ProseMirror (e.g., non-editable nodes, locked sections).

```typescript
// Pseudocode for the blocking plugin
filterTransaction(transaction, state) {
  if (!transaction.docChanged) return true;

  const suggestedBlockIds = getSuggestedBlockIds(state);

  // Check if any step in the transaction modifies a suggested block
  for (const step of transaction.steps) {
    const positions = getAffectedPositions(step);
    for (const pos of positions) {
      const blockId = getBlockIdAtPosition(state.doc, pos);
      if (suggestedBlockIds.has(blockId)) {
        return false; // Block the transaction
      }
    }
  }
  return true;
}
```

**Test cases:**

1. Create a suggestion on P2. Try to type in P2 - should be blocked
2. Try to type in P1 or P3 - should work normally
3. Try to delete P2 - should be blocked
4. Try to merge P1 and P2 (backspace at start of P2) - should be blocked
5. Create overlapping suggestions on P2 (from two conversations). Reject one. P2 should remain blocked (the other suggestion is still pending). Reject the second. P2 should become editable

**Validation criteria:**

- [ ] Typing in a suggested block is prevented
- [ ] Typing in non-suggested blocks is unaffected
- [ ] Structural edits to suggested blocks (delete, merge, split) are prevented
- [ ] Rejecting the last suggestion on a block makes it editable again

---

### Sub-question 4: Accept and reject operations

**What to build:**

TipTap commands for suggestion resolution:

**Accept:**

1. Read the suggestion's proposed content from the LoroMap
2. Replace the target blocks' content with the proposed content in the LoroDoc
3. Assign fresh BlockIds to the new content blocks
4. Remove the suggestion from the LoroMap (and the mark, if using Approach A)
5. The `LoroSyncPlugin` propagates the content change and mark removal to connected editors

**Reject:**

1. Remove the suggestion from the LoroMap (and the mark, if using Approach A)
2. The document content is unchanged
3. The `LoroSyncPlugin` propagates the mark removal - the block is no longer highlighted

**Supersession (same conversation, same target blocks):**

1. Detect that the new suggestion from conversation X targets the same blocks as an existing suggestion from conversation X
2. Replace the old suggestion in the LoroMap with the new one
3. Update the mark if needed

**Test cases:**

1. Create suggestion on P2. Accept it. Verify P2's content is replaced. Verify the suggestion is gone from the LoroMap
2. Create suggestion on P3. Reject it. Verify P3's content is unchanged. Verify the suggestion is gone
3. Create overlapping suggestions A (on P2) and B (on P2+P3). Accept A. Verify P2 content changes. Verify suggestion B still exists but its target blocks reference changed content (P2 now has new BlockIds from the accept)
4. Create suggestion from conversation X on P4. Create a second suggestion from conversation X on the same P4. Verify only the second suggestion exists
5. Sync all of the above to a second editor. Verify consistency

**Key concern for accept:** When accepting replaces target blocks with proposed content, the new blocks get fresh BlockIds. Any _other_ suggestion that referenced the old BlockIds now has stale references. The editor needs to handle this - either by detecting that the referenced blocks no longer exist (and rendering the suggestion as invalid) or by not allowing accept when overlapping suggestions exist (forcing the GM to reject the others first).

The design docs say "the editor flags them accordingly" but this spike needs to determine what "accordingly" means concretely. The simplest approach: if a suggestion's target BlockIds don't all exist in the document, the suggestion is invalid and should be auto-cleaned (removed from the LoroMap, recorded as `invalidated` in outcomes).

**Validation criteria:**

- [ ] Accept replaces content and removes the suggestion
- [ ] Reject removes the suggestion without changing content
- [ ] Supersession within a conversation replaces the old suggestion
- [ ] Overlapping suggestions are handled gracefully when one is accepted (either flagged or auto-cleaned)
- [ ] All operations sync correctly to other editors

---

### Sub-question 5: Inline diff rendering

**What to build:**

A TipTap extension (custom node view or decoration-based) that renders suggestions as inline diffs.

**Critical: the renderer must diff at the block level, not show raw before/after.** Because `suggest_replace` follows the Claude Code `str_replace` model (which is open source and a standard ML eval harness), insertions include anchor context - the `old_content` contains surrounding blocks that appear unchanged in `new_content`. A naive "show old, show new" rendering would display unchanged anchor blocks as if they're being modified. The renderer must compare target blocks against proposed blocks block-by-block and classify each as:

- **Unchanged:** Block appears identically in both target and proposed. Render normally, no diff styling. Still read-only (it's in `target_blocks`), but visually indistinguishable from regular content.
- **Modified:** Block exists in both but content differs. Render as strikethrough original + highlighted replacement.
- **Inserted:** Block exists in proposed but has no corresponding target block. Render as highlighted new content.
- **Deleted:** Block exists in target but has no corresponding proposed block. Render as strikethrough.

**Single suggestion on a block range:**

- Unchanged blocks render normally
- Modified/inserted/deleted blocks show inline diff styling
- Accept and reject buttons (inline or floating toolbar)
- Visual indicator that the block range is read-only

**Multiple suggestions on overlapping blocks:**

- A visual indicator showing the count ("2 suggestions")
- A way to navigate between proposals (tabs, dropdown, or stacked view)
- Each proposal shows its provenance (which conversation, which user, when)
- Accept/reject controls per proposal

**Implementation approach:** This is likely a ProseMirror node decoration or custom `nodeView` - not a mark decoration - because it needs to modify how the entire block renders, not just add styling to text. The decoration reads suggestion data from the LoroMap, finds the block's position in the editor, and renders the diff UI around it. A `nodeView` gives full control over rendering without modifying the document structure.

**Validation criteria:**

- [ ] Pure replacement suggestions render as readable inline diffs
- [ ] Insertion suggestions correctly show only the new content as added (anchor blocks render as unchanged)
- [ ] Deletion suggestions show removed content as strikethrough
- [ ] The diff UI clearly distinguishes unchanged, modified, inserted, and deleted blocks
- [ ] Accept/reject controls work and trigger the correct LoroDoc operations
- [ ] Multiple overlapping suggestions are visually distinguishable
- [ ] The read-only state is visually communicated to the user

---

## Out of Scope

Everything else is validated or is straightforward implementation work:

- LoroDoc ↔ ProseMirror sync (validated - `loro-prosemirror` works with custom TipTap schema)
- loro-dev/protocol server-side integration (validated)
- Conversation as LoroDoc with streaming (validated - `@loro-extended`)
- Actor topology and kameo integration (well-understood patterns, not Loro-specific)
- Serialization compiler (`f()` / `f⁻¹()`) - depends on SQ1's answer for how to read/write suggestion marks, but the compiler logic itself is string processing against a known format
- Persistence and reconstruction (validated implicitly - LoroDoc export/import works)
- RelationshipGraph / petgraph (straightforward)
- Suggestion outcomes table (plain SQL, no risk)

---

## Deliverables

1. **Working prototype** - a TipTap editor with suggestion mark support, demonstrating create/accept/reject/supersede on single and overlapping suggestions, with blocking and inline diff rendering. Synced between two editors via `loro-prosemirror` and the loro protocol.
2. **Approach decision** - Approach A (ProseMirror marks + LoroMap) or Approach B (LoroMap + decorations), with rationale.
3. **Architecture notes** - anything that changes in the domain design or serialization format based on what we learn. Particularly: how the `SuggestionTarget` trait maps to the concrete LoroDoc operations, and how the compiler's `f⁻¹()` produces suggestion marks.
4. **Edge case documentation** - what happens when accept invalidates overlapping suggestions, the exact BlockId lifecycle during accept, and any rendering limitations discovered.
