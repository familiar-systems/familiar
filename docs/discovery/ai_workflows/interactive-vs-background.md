# familiar.systems — Interactive vs Background AI Workflows

> **Resolved.** The questions raised in this document are addressed in the [AI workflow unification design](../../plans/2026-02-14-ai-workflow-unification-design.md), which unifies interactive and background workflows through a shared suggestion primitive and a single agent interface.

## Context

The [project structure design](../../archive/plans/2026-02-14-project-structure-design.md) defines two places where AI work happens:

1. **`apps/worker`** — background job consumer for long-running AI tasks (audio transcription, entity extraction, journal drafting). Jobs are enqueued by the API server and processed independently. Duration: minutes to tens of minutes.
2. **`apps/api`** — tRPC server that also handles interactive AI requests (content generation, suggestions, "what do we know about X?"). Streams tokens directly to the browser. Duration: seconds.

This document captures the current thinking and flags an open question: **should these two workflows be unified?**

---

## The Two Workflows Today

### Background (Worker)

Triggered by events like audio upload or journal finalization. The user doesn't watch the result appear — they come back later.

```
GM uploads audio for session 13
    → apps/api enqueues a "transcribe-session" job
    → apps/worker picks it up (polling)
    → Worker runs multi-stage pipeline:
        1. Audio → transcription (minutes)
        2. Transcription + notes → raw journal
        3. Raw journal + campaign graph → structured journal draft
        4. Journal draft → entity extraction proposals
        5. Entity extraction → contradiction checking
    → Results written to database
    → GM sees draft journal + review queue when they come back
```

**Characteristics:**

- Fire-and-forget from the user's perspective
- Multi-stage pipeline (output of one stage feeds the next)
- Total duration: 10+ minutes for a 3-hour recording
- Result is persisted — appears in review queue, not streamed to browser
- Must survive deploys (the web server can restart without killing the job)

### Interactive (API Server)

Triggered by direct user action while they're actively working. The user watches tokens stream in real-time.

```
GM is prepping session 14, clicks "Help me flesh out @Holeinthegroundmurder"
    → apps/api receives the request
    → API gathers context from the database:
        - The dungeon's page content
        - Backlinks: every block mentioning this dungeon (sessions 12, 13)
        - Relationships: connected entities (monsters, factions, items)
        - Session 14 prep notes written so far
    → API builds prompt with campaign context
    → API calls LLM provider, streams tokens
    → Tokens stream through API to the browser
    → GM sees text appearing in real-time
```

**Characteristics:**

- Synchronous from the user's perspective (they're watching)
- Single LLM call with assembled context
- Duration: 5-15 seconds
- Result is ephemeral until the GM explicitly accepts/saves it
- Latency-sensitive — any queuing delay is felt directly

---

## Where They Differ

| Dimension          | Background                       | Interactive                        |
| ------------------ | -------------------------------- | ---------------------------------- |
| Trigger            | Event (upload, finalize)         | User action (button click, prompt) |
| User expectation   | "I'll come back"                 | "I'm watching this now"            |
| Duration           | Minutes                          | Seconds                            |
| Stages             | Multi-stage pipeline             | Single context-gather + LLM call   |
| Result destination | Database (review queue)          | Browser (streaming tokens)         |
| Failure mode       | Retry silently, notify when done | Show error immediately             |
| Deploy sensitivity | Must survive restarts            | Short-lived, restart is fine       |
| Context assembly   | Same                             | Same                               |

---

## Where They Converge

Both workflows share the same core operation at their heart:

1. **Assemble context from the campaign graph** — gather the relevant nodes, blocks, relationships, and mentions
2. **Build a prompt** — combine context with a task-specific instruction
3. **Call an LLM** — send the prompt, get a response
4. **Do something with the result** — stream it to the user, or persist it and extract structure

The context assembly (step 1) and prompt building (step 2) are identical. The LLM call (step 3) differs only in whether the response is streamed to a browser or consumed server-side. The result handling (step 4) is where the real divergence lives.

The background pipeline is really just **multiple interactive-style operations chained together**, where:

- Each stage's output feeds the next stage's context
- The user isn't watching, so there's no streaming
- The intermediate results are persisted between stages (for recovery and auditability)

---

## Open Question: Can These Be Unified?

The current design has two separate code paths for AI work:

- `@familiar-systems/ai` (pipelines + prompts) used by `apps/worker`
- Interactive tRPC procedures in `apps/api` that also call LLM providers

This means context assembly logic, prompt templates, and LLM client code could end up duplicated or split awkwardly between the two paths.

**Possible unification directions to explore:**

### A. Shared pipeline primitives

Both workflows decompose into the same steps: gather context → build prompt → call LLM → handle result. A shared "pipeline step" abstraction could be used by both the worker (chaining steps) and the API server (running a single step with streaming).

### B. Worker as the single AI executor

All AI work — interactive and background — routes through the worker. Interactive requests get priority queuing and the worker streams results back via WebSocket or SSE. The API server never calls LLMs directly.

Trade-off: adds latency to interactive requests (queuing overhead) but centralizes all AI logic.

### C. Streaming background jobs

The background pipeline could stream intermediate results to the browser if the GM happens to be watching. "Your audio is processing... transcription complete... generating journal draft..." with tokens appearing in real-time. The GM can walk away at any point and the job continues.

This blurs the line between "background" and "interactive" — it's the same job, the difference is just whether someone is watching.

### D. The pipeline is always the same, the delivery channel varies

Define every AI operation as a pipeline of steps. The pipeline doesn't know or care whether someone is watching. A "delivery channel" adapter handles the output:

- **Streaming channel**: pipes tokens to a WebSocket/SSE connection if someone is listening
- **Persistence channel**: writes results to the database
- Both can be active simultaneously

---

## Questions for Next Session

1. Is there a real benefit to unifying, or is the bifurcation actually correct because the constraints (latency vs throughput) are genuinely different?
2. If we unify, does the worker become the single place where LLMs are called? Or does the API server call LLMs directly for interactive use?
3. Should background jobs be "watchable" — can the GM open a live view of a transcription-in-progress?
4. How does this interact with rate limiting and cost management? If all LLM calls go through one path, rate limiting is easier. If they're split, you need to coordinate.
5. Does the `@familiar-systems/ai` package already solve this? It defines pipelines and prompts — both the worker and the API server import it. Is the "unification" just ensuring that package is the single source of truth for all AI operations, with the caller deciding delivery?
