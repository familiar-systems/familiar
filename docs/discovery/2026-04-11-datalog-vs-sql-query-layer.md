# Datalog vs SQL for Campaign Query Layer — Discovery

## Friction

The campaign data model is fundamentally a graph of entities, relationships, mentions, and block-structured documents. The current architecture plan uses libSQL as cold storage, petgraph as an in-memory graph query layer, and typed tool calls (`search_entities`, `get_entity`, `get_entity_relationships`) as the interface for AI pipeline stages.

This works for the push-context pipeline path (session ingest), where the orchestrator knows what context to preload and the tool vocabulary is pre-shaped. But it leaves open the question of how LLM agents perform ad-hoc exploratory reads against campaign state — the pull-context path, where an agent needs to compose queries the pipeline designers didn't anticipate.

Typed tool calls are flat. "Find all characters present in the Amber Tribunal who have a relationship to the Crimson Pact" requires multiple sequential tool invocations — `list_entities(scene_id)`, then `get_relationships(entity_id)` for each result, then filter. The LLM orchestrates the joins across round trips, losing context and spending tokens at each step.

Meanwhile, the data model maps naturally to EAV triples and graph traversals. This is the exact problem space datalog was designed for.

## Why Datalog Is Enticing

familiar.systems was explicitly inspired by Logseq, which uses DataScript (a ClojureScript datalog engine) over EAV triples. The structural parallel is strong.

The campaign schema is small and stable. Approximately 20-25 relation types total:

- Things (~14 block types inside LoroDoc-backed pages)
- Relationships (node-to-node, freeform semantic labels)
- Mentions (block-to-node, block-to-block)
- ToC entries (arbitrary hierarchical organization)
- Suggestions (discriminated union: create_thing, update_blocks, create_relationship, journal_draft, contradiction)
- Sessions, journals, agent conversations
- CampaignVocabulary entries
- Viewing semantics and creation dates on all of the above

All of this is naturally EAV. A datalog query over this schema is short, declarative, and compositional. The equivalent SQL requires joins across normalized tables, recursive CTEs for graph traversals, and more tokens for an LLM to emit correctly.

A concrete comparison. In datalog:

```datalog
[:find ?name ?type
 :where [?e :name ?name]
        [?e :type ?type]
        [?e :present-in ?scene]
        [?scene :name "The Amber Tribunal"]]
```

The equivalent SQL involves joins across entity, relationship, and scene tables. The datalog is closer to natural language. The LLM has fewer structural ways to get it wrong.

Additional properties that favor datalog for this use case:

- **Termination by definition.** Datalog queries cannot loop. This is a real safety property when an LLM is authoring queries at runtime.
- **Composability.** Complex graph traversals (transitive closure, multi-hop relationship discovery) are native one-query operations, not multi-step tool call orchestration.
- **Schema-as-help-text.** The datalog schema compiles to a concise tool description that can be provided on first invocation or dumped via `db_query --help`. The same applies to SQL (as DDL), but the datalog version is terser and closer to natural language.
- **Read-only failure mode.** These are exploratory reads feeding into draft generation. A bad query produces a worse draft, which the GM catches at the approval gate. No data corruption risk.

## What We Discovered

### The Rust ecosystem has no viable runtime datalog engine

This is the binding constraint.

**CozoDB** was the ideal candidate: embeddable Rust datalog database with graph focus, HNSW vector search, SQLite as a storage backend, MPL-2.0 license. It checked every box. The project is dead — no meaningful maintenance activity.

**Ascent and Crepe** are the two active, well-maintained Rust datalog crates. Both are compile-time macro-based: rules are compiled into Rust code via proc macros. There is no runtime query evaluation. An LLM cannot emit a query string at runtime and have it evaluated. These are designed for static program analysis (Ascent powers parts of the Rust borrow checker via polonius-engine), not for database query interfaces.

**DataScript** (what Logseq actually uses) is a ClojureScript library. It would require crossing a language boundary (WASM or FFI) to use from Rust, and introduces a dependency on an ecosystem entirely outside the project's stack.

**Writing a mini datalog evaluator** is tractable given the small schema. The evaluation loop for datalog over a bounded, in-memory fact set (pattern matching, join via binding set intersection, fixed-point iteration for transitive closure) is well-documented. But it is still a real project to build and maintain — not a weekend hack if it needs to be reliable enough for production LLM tool invocation.

### LLM training data distribution works against datalog

LLMs have seen vastly more SQL than datalog in their training data. Datalog is theoretically simpler and closer to natural language, but "theoretically easier" and "empirically more reliable" are different claims. An LLM may produce correct SQL at a higher rate than correct datalog purely from exposure, even if the datalog query is structurally simpler.

This concern applies doubly to any custom DSL. A mini datalog evaluator would have its own syntax with zero representation in training data. The LLM would rely entirely on in-context schema documentation and few-shot examples in the system prompt. This is workable but unproven — it needs empirical evaluation.

### The schema-as-help-text pattern works regardless of query language

Whether the query layer is datalog or SQL, the approach is the same: the campaign schema compiles at build time to a tool description (help text, DDL, or schema dump) that is provided to the LLM agent on first invocation. The LLM does not need the full schema in every system prompt — only when the `db_query` tool is actually invoked for complex graph traversals. Simple reads continue to use pre-shaped typed tool calls.

This means the choice between datalog and SQL affects ergonomics and query correctness, but not the integration pattern.

### The database is the query surface for tools

Every actor with tool access — including LLM agent conversations — holds a read-only connection to the campaign's libSQL database. libSQL supports concurrent reads. Tools like `grep` or `ls` operate against the database the way shell tools operate against a filesystem: the database _is_ the queryable content store.

petgraph in the RelationshipGraph actor stores thing-to-thing relationships only — graph topology, not content. It can tell you that Kael is allied with the Crimson Pact, but not what Kael's page says, what its headings are, or what blocks mention him. A datalog layer over the full database would give you both: graph traversals _and_ content queries in a single declarative interface. This is a qualitative capability difference, not just a different query syntax over the same data.

## Where We Land

Datalog is the correct query model for this domain. The campaign data is EAV triples with graph relationships, the query patterns are declarative graph traversals, and the safety properties (termination, read-only failure mode) align with LLM-authored queries.

The Rust ecosystem does not currently support this. There is no stable, maintained, embeddable runtime datalog engine suitable for the use case. CozoDB was the closest match and is dead. Ascent and Crepe are compile-time only.

The project proceeds with libSQL and typed tool calls for the pipeline (push-context path). For the exploratory agent query path (pull-context), the initial implementation uses typed tool calls with the campaign context interface already designed in the audio pipeline architecture. This is less expressive than datalog but reliable and shippable.

The door remains open to:

1. **A mini datalog evaluator** over in-memory actor state, if the typed tool call approach proves too limiting for agent reasoning. The schema is small enough that this is tractable. Build cost is real but bounded.
2. **CozoDB fork or revival**, if the project regains maintenance or someone forks it. MPL-2.0 permits this.
3. **A new Rust datalog runtime** emerging from the ecosystem. The gap between "Rust has great compile-time datalog" and "Rust has no runtime datalog" is visible and someone may fill it.

The key architectural hedge: the campaign context interface (search_entities, get_entity, get_entity_relationships, search_blocks) is an abstraction boundary. The implementation behind it can change from SQL queries and petgraph traversals to datalog queries without changing the tool interface exposed to LLM agents. If datalog becomes viable, it slots in behind this interface. However, the typed tool call approach is fundamentally less expressive than direct datalog queries — it cannot compose arbitrary joins across graph topology and page content in a single operation. If agent reasoning hits that ceiling, the mini evaluator (option 1) becomes the path forward.

## References

- [Campaign Actor Domain Design](../plans/2026-03-25-campaign-actor-domain-design.md) — actor topology, RelationshipGraph, ThingActor, persistence model
- [Campaign Collaboration Architecture](../plans/2026-03-25-campaign-collaboration-architecture.md) — campaign checkout, libSQL as cold storage, scaling model
- [Audio Pipeline Architecture](./audio_ingest/audio_overview.md) — campaign context interface, push vs pull context patterns
- [SQLite over PostgreSQL](./2026-03-09-sqlite-over-postgres-decision.md) — database-per-campaign decision, libSQL choice
- [CozoDB](https://github.com/cozodb/cozo) — dead; embeddable Rust datalog with graph focus and vector search
- [Ascent](https://github.com/s-arash/ascent) — active; compile-time datalog in Rust via proc macros (not runtime)
- [Crepe](https://crates.io/crates/crepe) — active; compile-time datalog in Rust via proc macros (not runtime)
- [DataScript](https://github.com/tonsky/datascript) — ClojureScript runtime datalog; what Logseq uses
