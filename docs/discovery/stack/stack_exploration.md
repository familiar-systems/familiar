# familiar.systems - Technology Stack Exploration

## Context

This document evaluates technology stacks for familiar.systems before any code is written. It sits alongside the [storage analysis](../../archive/discovery/2026-02-14-storage-overview.md) (PostgreSQL or Turso) and the [audio pipeline design](../audio_ingest/audio_overview.md) (multi-stage AI processing).

The [vision doc](../../vision.md) defines a **server-hosted web app** (with self-hosted option) centered on a **block-based rich text editor** wired into a **property graph**. The stack needs to serve three workloads:

1. **Web application** - CRUD, auth, multi-user, real-time updates
2. **Rich text editing** - block-based content with entity mentions, transclusion, status visualization, and source linking
3. **AI pipeline** - multi-stage audio-to-graph processing, LLM API orchestration, vector search

**Operator profile:** Solo developer. Background in Java 8+, C#, Scala. Daily work in Python. Strong preference for static typing. No prior frontend experience.

---

## The Hard Problem: Rich Text Editing

The editor is the hardest technical problem in this stack. Not the database, not the AI pipeline, not the graph. Browser-based rich text editing deserves its own analysis because it constrains everything else.

### Why It's Hard

All rich text editing in the browser sits on top of `contentEditable` - a browser API that is famously inconsistent across engines, poorly specified, and full of edge cases. Every rich text editor you've used in a browser (Google Docs, Notion, Confluence) is fighting `contentEditable` under the hood.

This is a solved-enough problem - but only if you use a library that abstracts over the browser's quirks. Rolling your own is not viable.

### What familiar.systems Needs from the Editor

1. **Block-based content** - paragraphs, headings, stat blocks, images, AI suggestion blocks
2. **Inline entity mentions** - autocomplete-driven references (`@Kael` → resolved node link) that become clickable graph links
3. **Transclusion** - embedding a block from another node, rendered inline, updated live
4. **Status visualization** - GM-only blocks dimmed/tinted, retconned blocks struck through, directly in the editor
5. **Source linking metadata** - blocks carry references back to audio timestamps
6. **Collaborative editing** (eventually) - GM and players editing concurrently

### Editor Libraries

From lowest to highest level of abstraction:

**ProseMirror** - The gold standard for structured rich text editing. Schema-defined document model, transaction-based state management, plugin architecture. Powers the New York Times CMS, Atlassian's editor, and others. The API is powerful and low-level - you define your document schema, write plugins for behavior, and manage state transitions explicitly. Steep learning curve, maximum control.

**TipTap** - A framework built on ProseMirror that provides a significantly more ergonomic developer API while preserving ProseMirror's full power underneath. Key capabilities relevant to familiar.systems:

- **Mention extension** - autocomplete-driven inline references, out of the box
- **Custom node views** - embed full React/Vue/Svelte components inside the editor (for stat blocks, AI suggestion cards, transcluded blocks)
- **Collaboration** - real-time collaborative editing via Yjs (CRDT-based)
- **Extensions** - modular architecture; add capabilities without touching core

TipTap is what most new projects building structured editors choose. It handles requirements 1, 2, and 6 directly. Requirements 3-5 are custom work but well within TipTap's extension model - you'd write custom node types and decorations.

**BlockNote** - Built on TipTap/ProseMirror. Gives you Notion-style block editing out of the box - drag handles, slash commands, block types. Interesting because familiar.systems's content model is block-native. Trade-off: you gain a faster starting point but lose some flexibility compared to raw TipTap. Newer, smaller community.

**Lexical** (Meta) - Different architecture from ProseMirror (tree-based, not schema-based). Good React integration, good performance. Less mature plugin ecosystem. The mention and collaboration stories are less developed than TipTap's. Actively maintained by Meta but smaller community outside Meta's own products.

**Slate** - Was popular circa 2018-2020. Has gone through major rewrites and API instability. Less recommended for new projects.

### Editor Recommendation

**TipTap**, starting with basic rich text and adding custom extensions incrementally:

1. Start with TipTap's built-in block types (paragraph, heading, list, image)
2. Add the Mention extension for entity references
3. Build a custom node type for transcluded blocks
4. Add ProseMirror decorations for status visualization (gm_only, retconned)
5. Add source-linking metadata as node attributes on blocks

BlockNote is worth evaluating if the Notion-style UX (slash commands, drag handles) is desired from day one. But starting with TipTap gives more control over the editor's behavior and a more gradual learning curve.

### The Inescapable Constraint

**The editor is TypeScript/JavaScript regardless of what the backend is.** ProseMirror, TipTap, BlockNote, and Lexical are all TypeScript libraries. There is no server-side shortcut - HTMX, Phoenix LiveView, and server-rendered templates cannot help with the editor. You will write TypeScript for this part of the application.

The question is: how much TypeScript do you write _beyond_ the editor?

---

## Backend Language Analysis

The workload is web CRUD, AI pipeline orchestration (I/O-bound LLM API calls and text processing), and graph queries (shallow traversals on small data). None of these are CPU-bound or latency-critical at campaign scale.

### TypeScript (Node.js / Bun)

Full-stack: same language as the frontend.

**Strengths:**

- **One type system end-to-end.** The block model - the core domain object - is defined once and flows through the editor, the API, and the database layer. With tRPC, the API contract is inferred from the backend types with zero code generation.
- **Largest web ecosystem.** Every problem has been solved, every integration has a library.
- **Editor alignment.** TipTap's document schema is a TypeScript construct. If the backend also speaks TypeScript, the editor schema and the database schema can share type definitions.
- **Async I/O is native.** The AI pipeline (calling LLM APIs, waiting on responses, processing text) maps directly to Node's event loop.

**Weaknesses:**

- TypeScript's type system is structural, not nominal. Escape hatches exist (`any`, type assertions). Strictness is enforced by configuration and linting, not by the language.
- Runtime doesn't enforce types. Data crossing system boundaries (API responses, database rows) needs explicit validation (Zod, Valibot).
- The JS/TS ecosystem has high churn. Library choices that seem standard today may not be in two years.

**Mitigation for type weakness:** `strict: true` in tsconfig, `noUncheckedIndexedAccess`, Zod for runtime validation at system boundaries, ESLint `no-explicit-any` rule. This combination gets close to the discipline of a nominally typed language.

### Python (FastAPI + Pydantic)

**Strengths:**

- **No learning curve on the backend.** Daily working language.
- **FastAPI + Pydantic** provide strong typing at the API and data layer. Pydantic validates at runtime, not just at type-check time.
- **Best AI/ML ecosystem.** If you ever want local embedding generation, model inference, or integration with Python-native AI tools, the libraries are here.
- **basedpyright** on strict settings catches most type errors at edit time, approaching the experience of a statically typed language.

**Weaknesses:**

- **Two-language tax.** The block model is defined twice - once in Python (storage/API), once in TypeScript (editor). They must stay in sync. You can generate TypeScript types from Pydantic models (tools like `pydantic-to-typescript` exist), but it's friction and another tool in the chain.
- **Python's type system is opt-in.** Third-party libraries may have incomplete stubs. `typing.Any` leaks through the ecosystem.
- **Async Python is adequate but less mature** than Node's event loop. `asyncio` works but the ecosystem of async-compatible libraries is smaller.

### Rust (Axum)

**Strengths:**

- **Strongest type system.** Algebraic types, exhaustive pattern matching, no null. If it compiles, type-level guarantees are real.
- **sqlx** provides compile-time checked SQL queries against the actual database schema.
- **If audio processing ever moves in-process**, Rust handles binary data and concurrency naturally.

**Weaknesses:**

- **Compile-edit-test cycle is slower.** For web CRUD, where you iterate on endpoints and UI integration, this friction is meaningful.
- **The AI pipeline is I/O-bound.** Calling HTTP APIs and processing strings does not benefit from zero-cost abstractions or memory safety without GC.
- **Two languages**, plus Rust's learning curve is the steepest of any option here. Learning Rust and TypeScript and frontend simultaneously is a lot of surface area.
- **Thinner web ecosystem.** Fewer ORMs, fewer auth libraries, fewer "install and configure" solutions compared to Node or Python.

**Verdict:** The type system is beautiful, but the iteration speed penalty and ecosystem thinness make it a poor fit for a solo developer building a web app where the backend is not the hard part.

### Kotlin (Ktor)

**Strengths:**

- **Leverages JVM background directly.** Sealed classes, data classes, coroutines, null safety - excellent type system.
- **Ktor** is a lightweight, idiomatic web framework. Spring Boot is the heavyweight alternative with more batteries included.
- **Exposed** (Kotlin ORM) or **jOOQ** for type-safe database access.

**Weaknesses:**

- **JVM startup time and memory footprint.** Not a problem for a long-running production server, but noticeable in dev (restart cycles) and in self-hosted deployments on small VMs.
- **Two-language tax**, same as Python - block model defined twice, API contract needs code generation or manual sync.
- **Kotlin's web ecosystem is smaller** than Node, Python, or Go. You'll occasionally find that the library you need doesn't exist or is maintained by one person.

**Verdict:** A reasonable choice if JVM comfort outweighs the ecosystem penalty. But the two-language cost is the same as Python, without Python's AI ecosystem advantage.

### Excluded Options

**Go** - Limited generics, no algebraic types, verbose error handling. Coming from Scala/FP, the type system will feel constraining rather than helpful. Go's strengths (simplicity, fast compilation, goroutines) are real but don't offset the expressiveness loss for a solo developer.

**Elixir / Phoenix LiveView** - Dynamically typed (Dialyzer helps but doesn't fully compensate). LiveView is compelling for reducing frontend JS, but the rich text editor is JS regardless, so the main selling point doesn't apply to the hard part of this project.

**C# / Blazor** - Blazor (Server or WASM) could theoretically put C# in the browser, but the rich text editor ecosystem is in JavaScript. TipTap/ProseMirror interop from Blazor is awkward. The strongest C# argument would be ASP.NET Core on the backend + React frontend, but at that point you have the same two-language tax as Python or Kotlin with a thinner open-source web community.

---

## Frontend Framework

Since the editor is the centerpiece and the developer has no frontend experience, the framework choice matters for learning curve and ecosystem support.

### React

**Why it fits:**

- Largest ecosystem. More tutorials, more Stack Overflow answers, more libraries than any alternative.
- TipTap has **first-class React bindings** - the editor renders as a React component with hooks for state management.
- When you get stuck, someone has written about the exact problem you're having.

**Downsides:**

- Hooks-based mental model (useEffect, useState, useMemo) has a learning curve. Some patterns are counterintuitive.
- React re-renders more than necessary by default. For most apps this doesn't matter; for an editor-heavy app, you may need to be deliberate about memoization.

### Svelte / SvelteKit

- Simpler mental model than React. Less boilerplate. Reactivity is built into the language.
- TipTap has Svelte support, but it's less mature than the React bindings.
- Smaller ecosystem means fewer pre-built solutions when you need them.

### Vue

- TipTap actually originated in the Vue ecosystem. Vue bindings are mature.
- Good middle ground between React's ecosystem size and Svelte's simplicity.
- Smaller English-language community than React.

### Frontend Recommendation

**React.** Not because it's the best framework in the abstract, but because when learning frontend for the first time, community size is the dominant factor. Every error message, every integration challenge, every "how do I do X with TipTap in React" question has been answered. The learning curve is real but well-documented.

---

## Composed Stacks

### Stack A: Full TypeScript

```
Framework:    Next.js (or Remix)
Frontend:     React + TipTap
API layer:    tRPC (end-to-end type safety, no code generation)
Database:     Drizzle ORM → PostgreSQL (or Turso/libSQL)
Validation:   Zod (runtime validation at system boundaries)
AI pipeline:  TypeScript, async/await over LLM API calls
```

**One language. One type system. One definition of the block model.**

The block schema in the editor, the API contract, and the database types are all TypeScript. tRPC infers the API types from the backend - no OpenAPI spec, no code generation, no drift.

Drizzle ORM is type-safe and SQL-adjacent (you write SQL-like expressions in TypeScript, not a custom query language). It supports both PostgreSQL and SQLite/Turso, which preserves the storage optionality from the storage analysis.

### Stack B: Python Backend + TypeScript Frontend

```
Backend:      FastAPI + Pydantic
Frontend:     React + TipTap
API contract: OpenAPI (generated from FastAPI, TS types generated from OpenAPI)
Database:     SQLAlchemy → PostgreSQL
Validation:   Pydantic (backend), Zod (frontend)
AI pipeline:  Python (native ecosystem: LiteLLM, LangChain, etc.)
```

**Familiar backend. Best AI ecosystem. Two-language tax on the block model.**

The API contract is maintained through OpenAPI generation: FastAPI auto-generates an OpenAPI spec from Pydantic models, and a tool like `openapi-typescript` generates TypeScript types from that spec. This keeps the two type systems in sync, but it's an additional step in the build pipeline.

Python's AI ecosystem is the strongest argument for this stack. If you want to use LangChain, LlamaIndex, or similar orchestration libraries for the AI pipeline, they're Python-native.

### Stack C: Kotlin Backend + TypeScript Frontend

```
Backend:      Ktor + Exposed (or jOOQ)
Frontend:     React + TipTap
API contract: OpenAPI (Kotlin → spec → TS types)
Database:     Exposed → PostgreSQL
AI pipeline:  Kotlin coroutines, HTTP clients for LLM APIs
```

**Strong JVM typing. Leverages existing background. Thinner ecosystem.**

The same two-language pattern as Stack B, but with Kotlin's stronger type system on the backend. The AI pipeline uses Kotlin coroutines for async orchestration and HTTP clients for LLM API calls - adequate, but without Python's ecosystem of AI-specific libraries.

---

## Recommendation

### Primary: Stack A (Full TypeScript)

The single-language advantage is decisive for a solo developer building an editor-centric application.

**The core argument:** The block model is the central domain object in familiar.systems. It lives in the editor (TipTap schema), in the API contract (request/response types), and in the database (table schema). In a full TypeScript stack, this is one definition that flows through the entire system. In a two-language stack, it's two definitions connected by code generation - a seam that must be maintained and can drift.

**Addressing the type safety concern:** TypeScript's type system is weaker than Rust's or Kotlin's. But with `strict: true`, `noUncheckedIndexedAccess`, Zod at system boundaries, and linting rules against `any`, the practical gap narrows significantly. The types you lose (nominal types, exhaustive checking on primitive values) matter less than the types you gain by not having a language boundary in the middle of your data model.

**The learning cost is unavoidable anyway.** The editor is TypeScript. The frontend is TypeScript. Learning TypeScript is not optional - the question is whether you also learn a second backend language or go deep in one.

### Fallback: Stack B (Python + TypeScript)

If, after evaluating TypeScript, the type system feels too loose or the ecosystem too chaotic, Python is the natural fallback. You lose the single-language advantage but gain a familiar backend and the best AI ecosystem. The two-language tax is real but manageable - many production applications work this way.

### What Stack A does NOT mean

Choosing TypeScript does not mean choosing a specific framework permanently. The recommendation is:

- **React** for UI (most ecosystem support for a frontend newcomer)
- **TipTap** for the editor (best developer experience on ProseMirror)
- **tRPC + Drizzle** for backend (type-safe, SQL-adjacent)
- **Next.js or Remix** for the full-stack framework (conventions, routing, SSR)

These are starting points. React is the most durable choice; the others can be swapped if they don't work out. tRPC can be replaced with a REST layer. Drizzle can be replaced with Prisma or raw SQL. The framework matters less than the language.

---

## Sources

### Editor Libraries

- [ProseMirror](https://prosemirror.net/) - toolkit for building rich text editors
- [TipTap](https://tiptap.dev/) - headless editor framework built on ProseMirror
- [TipTap Mention extension](https://tiptap.dev/docs/editor/extensions/nodes/mention) - inline entity references with autocomplete
- [TipTap Collaboration](https://tiptap.dev/docs/editor/extensions/functionality/collaboration) - real-time collaborative editing via Yjs
- [TipTap Node Views (React)](https://tiptap.dev/docs/editor/guides/node-views/react) - embed React components inside the editor
- [BlockNote](https://www.blocknotejs.org/) - block-based editor built on TipTap/ProseMirror
- [Lexical](https://lexical.dev/) - Meta's extensible text editor framework

### TypeScript Stack

- [Next.js](https://nextjs.org/) - full-stack React framework
- [Remix](https://remix.run/) - full-stack web framework (merged with React Router)
- [tRPC](https://trpc.io/) - end-to-end type-safe APIs without code generation
- [Drizzle ORM](https://orm.drizzle.team/) - TypeScript ORM, SQL-adjacent, supports PostgreSQL + SQLite
- [Zod](https://zod.dev/) - TypeScript-first schema validation
- [Valibot](https://valibot.dev/) - modular schema validation (lighter alternative to Zod)

### Python Stack

- [FastAPI](https://fastapi.tiangolo.com/) - async Python web framework with auto-generated OpenAPI
- [Pydantic](https://docs.pydantic.dev/) - data validation using Python type hints
- [SQLAlchemy](https://www.sqlalchemy.org/) - Python SQL toolkit and ORM
- [basedpyright](https://github.com/DetachHead/basedpyright) - strict Python type checker
- [openapi-typescript](https://openapi-ts.dev/) - generate TypeScript types from OpenAPI specs

### Kotlin Stack

- [Ktor](https://ktor.io/) - Kotlin async web framework
- [Exposed](https://github.com/JetBrains/Exposed) - Kotlin SQL framework (JetBrains)
- [jOOQ](https://www.jooq.org/) - type-safe SQL in Java/Kotlin
