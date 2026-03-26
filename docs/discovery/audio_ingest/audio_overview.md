# Loreweaver — Audio Pipeline Architecture

## Context

This document describes the architecture for processing session audio (and other raw sources) into structured campaign data. It sits between the [vision doc](../../01_vision.md) (which defines the post-session workflow) and the [storage analysis](../../archive/discovery/2026-02-14-storage-overview.md) (which defines how data is persisted).

**The problem:** A raw session transcript is unstructured, messy, and long (30k-60k+ tokens for a 3-4 hour session). The campaign graph is structured, block-native, and entity-resolved. Something needs to bridge that gap.

---

## Why a Pipeline, Not a Single LLM Call

The naive approach — hand the transcript to an LLM with tools pointed at the campaign graph and say "produce a journal entry" — breaks down for several reasons:

1. **Token budget**: The transcript alone may exceed context windows, and you also need campaign context loaded. Chunking strategy is application logic, not something to delegate to the LLM.

2. **Reliability**: A single monolithic prompt that does entity extraction, narrative cleanup, contradiction detection, and relationship proposal will do all of them poorly. Discrete stages with defined input/output schemas are more reliable and debuggable.

3. **Cost control**: Not every stage needs the most capable model. Entity resolution against a known list might work fine with a cheaper model; narrative drafting benefits from a stronger one. A pipeline lets you choose per-stage.

4. **Reviewability**: The GM needs to review at specific points (the journal draft, the entity proposals). A pipeline has natural checkpoints; a monolithic call produces a blob.

---

## Pipeline Stages

```
Raw Sources (transcript + GM notes + player recollections)
        |
  [1. Ingest & Merge]
        |
  [2. Chunking]
        |
  [3. Entity Extraction & Resolution]    <-- AI + campaign graph tools
        |
  [4. Journal Draft]                     <-- AI + campaign context
        |
  [5. Block Assembly]                    <-- deterministic, no AI
        |
  [6. Proposal Generation]              <-- AI + campaign graph tools
        |
  Review Queue (GM reviews)
        |
  Graph Mutations (applied to DB)
```

### Stage 1: Ingest & Merge

**Input:** Raw session sources — audio transcript, GM notes, player recollections (from `session_sources` table).

**Process:** Non-AI or light AI. Combines sources into a unified timeline. Deduplication if multiple sources cover the same moments, timestamp alignment between audio and notes.

**Output:** Merged text with source markers (so any downstream content can trace back to which source and timestamp it came from).

### Stage 2: Chunking

**Input:** Merged text from Stage 1.

**Process:** Split into processable segments that fit within LLM context windows alongside campaign context.

- **Naive approach:** Token-based chunking with overlap. Simple, predictable.
- **Better approach:** Scene/topic boundary detection (light AI pass). A session naturally has scenes — "the tavern conversation," "the journey to Grimhollow," "the ambush." Chunking at scene boundaries produces more coherent segments.

Each chunk carries source refs back to timestamps in the original audio.

**Output:** Ordered list of chunks, each with source references.

### Stage 3: Entity Extraction & Resolution

**Input:** Individual chunks + campaign graph context.

**Process:** Per-chunk LLM processing. For each chunk:

1. **RAG retrieval**: Embed the chunk, retrieve top-N candidate nodes from the campaign graph via vector search. This preloads relevant campaign context into the prompt.
2. **Entity detection**: The LLM identifies entity mentions in the chunk — people, places, factions, items, lore.
3. **Resolution**: For each detected entity, resolve against existing nodes. "The blacksmith" → Tormund (existing NPC). "A hooded figure named Kael" → new entity, or existing Kael if already in the graph.
4. **Annotation**: The chunk text is annotated with resolved entity references using inline markup.

**Tools available to the LLM:**

- `search_entities(query, type?)` — find existing nodes by name/description
- `get_entity(id)` — get full details of a node
- `get_entity_relationships(id)` — get relationships for context ("is this the Kael who frequents the Rusty Anchor?")

**Output:** Annotated chunks with resolved entity references. Unresolved entities flagged as "new."

### Stage 4: Journal Draft

**Input:** All annotated chunks from Stage 3, in order.

**Process:** Reassemble annotated chunks into a coherent narrative. The LLM cleans up transcript artifacts (filler words, crosstalk, out-of-character table talk), applies narrative structure (scene breaks, pacing), and produces a readable journal entry — while preserving the entity annotations from Stage 3.

Campaign context (recent session summaries, active plot threads) is injected to help the LLM maintain continuity and tone.

**Output:** A complete journal draft as marked-up text with entity references:

```
The party arrived at [[loc:rusty-anchor|the Rusty Anchor]] where
they met [[npc:kael|a hooded figure who introduced himself as Kael]].
He warned them about [[faction:silver-compact|the Silver Compact]].
```

### Stage 5: Block Assembly

**Input:** Marked-up journal draft from Stage 4.

**Process:** Deterministic — no AI involved. A parser:

1. **Segments** the narrative into blocks (paragraphs, scene breaks, heading heuristics)
2. **Extracts** the `[[type:id|display text]]` markers into mention records
3. **Links** each block back to its source chunk (and thus to the audio timestamp via source_ref)
4. **Assigns** default status (`gm_only`) to all blocks

This stage is a conventional parser, not an LLM call. It's testable, deterministic, and doesn't hallucinate.

**Output:** Structured blocks with mention records, ready for storage. Every block has a `source_ref` tracing back to the audio.

**Why not have the AI produce blocks directly?** LLMs are unreliable at producing complex structured output (nested JSON with ordering, types, and references). Annotated prose with lightweight markup keeps the AI focused on what it's good at (language, entity recognition, narrative), and the structured extraction is a solved problem.

### Stage 6: Proposal Generation

**Input:** The entity-annotated journal from Stage 4, plus campaign graph context.

**Process:** AI-driven analysis of what the session established:

- **New entities** — characters, locations, items mentioned for the first time. Proposed as new nodes with whatever context the journal provides.
- **New relationships** — "Kael warned the party about the Silver Compact" → `kael -> warned about -> silver-compact`. Proposed with labels.
- **Entity updates** — changes to existing nodes ("Tormund is now deceased", "the party learned Grimhollow is a ruin").
- **Contradiction flags** — "Session 12 established Kael is from Northport, but this session says he's from the Eastern Reaches."

**Output:** A review queue of proposals, each linking back to the journal block(s) that triggered it.

---

## Campaign Context Interface

The AI stages (3, 4, 6) need access to the campaign graph. This is exposed through a **campaign context interface** — an application-level abstraction over the database.

### Interface

```
search_entities(query, type?) → [{id, name, type, summary}]
get_entity(id)               → {node with blocks, status}
get_entity_relationships(id) → [{relationship, target}]
search_blocks(query)         → [{block, node, relevance}]    # vector search
get_recent_sessions(n)       → [{session summary, key events}]
```

This interface:

- Knows about status filtering (the AI should see GM-only content during processing)
- Is campaign-scoped (queries are always within a single campaign)
- Shapes results for LLM consumption (summaries, not raw DB rows)

### Two Exposure Patterns

**Internal (pipeline use) — push context.** The pipeline orchestrator calls the context interface directly, retrieves relevant data, and injects it into the LLM prompt as system context _before_ the call. The orchestrator decides what's relevant. This is more token-efficient and reliable — you control exactly what the LLM sees.

**External (MCP) — pull context.** The same interface, exposed as an MCP server, lets the GM use external AI tools (Claude, ChatGPT, etc.) with their campaign graph as live context. "I'm prepping for next session — what unresolved plot threads involve Kael?" The LLM decides what to fetch via tool calls.

**Why both, not just MCP everywhere?** The internal pipeline wants to preload context aggressively — embed the chunk, retrieve top-N relevant nodes, inject them before the LLM call. That's push. MCP is pull — the LLM discovers what it needs via tool calls, which adds round-trips, token overhead, and unpredictability. Inside the pipeline, push is better. For interactive GM queries, pull (MCP) is the right UX because the GM's questions are open-ended.

The alternative — using MCP as the internal pipeline interface — adds protocol overhead, serialization cost, and forces the tool-call interaction pattern even when preloaded context would be more efficient.

---

## Service Layer Shape

The pipeline is owned by a **session processor** service that orchestrates the stages:

```
SessionProcessor
  ├── IngestionService        — merges raw sources, aligns timestamps
  ├── ChunkingService         — splits merged text into segments
  ├── EntityResolver          — AI stage: extract & resolve entities
  ├── JournalDrafter          — AI stage: produce narrative draft
  ├── BlockAssembler          — deterministic: parse markup into blocks
  ├── ProposalGenerator       — AI stage: propose entities, relationships, updates
  └── uses: CampaignContext   — injected interface to the campaign graph
```

Each stage has a defined input/output contract. The `CampaignContext` interface is injected into AI-facing stages. The same interface is separately exposed via MCP for external AI access.

**Checkpoint boundaries** where GM review can intervene:

- After Stage 4 (journal draft) — the GM edits the narrative before it's finalized
- After Stage 6 (proposals) — the GM accepts, edits, or rejects entity/relationship proposals

---

## Open Questions

- **Chunking granularity**: What's the right chunk size? Too small and entity resolution loses cross-scene context. Too large and you hit token limits. Likely needs tuning per model.
- **Entity markup format**: `[[type:id|display]]` is one option. XML-style tags or a sidecar annotation format are alternatives. The choice affects how robust the markup is to narrative rewriting in Stage 4.
- **Streaming vs. batch**: Should the pipeline process chunks in parallel (faster) or sequentially (each chunk benefits from entities resolved in previous chunks)? Sequential is more accurate; parallel is faster. Hybrid possible — parallel extraction, sequential resolution.
- **Model selection per stage**: Which stages benefit from stronger models vs. cheaper ones? Entity resolution against a known list might work with a smaller model; narrative drafting and contradiction detection likely need a stronger one.
- **Incremental reprocessing**: If the GM edits the journal draft (Stage 4 checkpoint), do Stages 5-6 re-run automatically? Probably yes, since the edits may change which entities are referenced.
