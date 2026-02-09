# Loreweaver — Storage Architecture Analysis

## Context

This document analyzes storage options for Loreweaver before any code is written. The goal is to choose a storage strategy that fits the product's data model (a property graph with rich content blocks) and its deployment model (server-hosted web app).

**Decision:** Server-hosted web app. Language TBD.

---

## What the Data Looks Like

The [vision doc](./01_vision.md) defines a **property graph with rich content**:

- **Nodes**: Campaigns, Arcs, Sessions, Things (NPCs, locations, items, factions, etc.)
- **Edges**: Labeled, directed relationships between any two nodes (e.g., Kael -> Rusty Anchor: "frequents")
- **Blocks**: Rich content units nested inside nodes (text, headings, stat blocks, images, AI suggestions)
- **Status**: Every primitive (node, edge, block) carries a status field: `gm_only | known | retconned`
- **References**: Blocks can reference other blocks (transclusion), and journal entries contain entity mentions that link to thing nodes

## Query Patterns

1. **Shallow graph traversal** -- "show me everything related to this NPC" (1-3 hops)
2. **Status filtering** -- every query potentially filters by status (player view vs GM view)
3. **Full-text search** -- "find all mentions of Grimhollow across journal entries and things"
4. **Temporal ordering** -- sessions are chronologically ordered
5. **Block resolution** -- fetch a specific block by ID for transclusion
6. **AI context retrieval** -- find the most relevant nodes/blocks for a given query (semantic/vector search)
7. **Bulk reads** -- render a thing page with all its blocks, edges, and transcluded content

## Scale Reality Check

A single campaign is **small data**. Even a long-running campaign (100+ sessions over years) might have:

- ~200-500 thing nodes
- ~1,000-5,000 edges
- ~5,000-20,000 blocks
- ~100-200 session nodes

This is trivially small for any database engine. The challenge is **flexibility and developer ergonomics**, not scale.

---

## Options Evaluated

### Option A: SQLite (local-first)

Store the graph as relational tables in a SQLite database. The campaign *is* a `.db` file.

**Schema sketch:**

```sql
nodes      (id, type, template, status, properties JSON, created_at, updated_at)
edges      (id, source_id, target_id, label, status, properties JSON, created_at, updated_at)
blocks     (id, node_id, parent_block_id, type, position, status, content JSON, source_ref, created_at, updated_at)
block_refs (id, from_block_id, to_block_id, ref_type)
```

Graph traversals use recursive CTEs:

```sql
WITH RECURSIVE related AS (
  SELECT target_id, label, 1 as depth
  FROM edges WHERE source_id = ? AND status != 'retconned'
  UNION ALL
  SELECT e.target_id, e.label, r.depth + 1
  FROM edges e JOIN related r ON e.source_id = r.target_id
  WHERE r.depth < 3 AND e.status != 'retconned'
)
SELECT * FROM related;
```

**Extensions:**

- **FTS5** for full-text search across blocks
- [**sqlite-vec**](https://github.com/asg017/sqlite-vec) for vector embeddings (AI context retrieval) -- stable v0.1.0, C with no dependencies, MIT/Apache-2.0
- [**cr-sqlite**](https://github.com/vlcn-io/cr-sqlite) for CRDT-based multi-device sync (future) -- active development, inserts ~2.5x slower than plain SQLite

**Why this fits:**

- Campaign-scale data is trivially small for SQLite -- reads are sub-millisecond
- "Campaign as a file" -- backup, share, migrate by copying a file
- Zero deployment complexity -- no server, no connection strings
- Local-first by nature -- the GM works offline, always
- Shallow graph traversals (1-3 hops) work cleanly with recursive CTEs
- Massive ecosystem: every language has a SQLite driver, every platform runs it

**What it doesn't give you:**

- No native graph query language (Cypher/GQL) -- you write SQL with CTEs
- Multi-user access (player views) requires a sync/server layer on top
- No built-in real-time subscriptions

**Best for:** Local-first desktop app where the GM is the primary (or only) user.

---

### Option B: PostgreSQL + JSONB

Standard relational tables with JSONB for flexible properties, hosted on a server. Same adjacency-table schema as Option A but with PostgreSQL-specific capabilities.

**Additional capabilities over SQLite:**

- [**pgvector**](https://github.com/pgvector/pgvector) for AI embeddings -- mature, widely deployed
- Built-in full-text search via tsvector/tsquery
- Row-level security (RLS) for GM-only/Known visibility filtering
- [**Apache AGE**](https://age.apache.org/) extension for Cypher graph queries if CTEs get unwieldy -- available on [Azure](https://learn.microsoft.com/en-us/azure/postgresql/azure-ai/generative-ai-age-overview), still maturing for self-hosted
- Connection pooling and concurrent multi-user access
- Every hosting provider: Supabase, Neon, RDS, Railway, etc.

**Tradeoffs:**

- Requires a server from day one
- Multi-tenancy means `campaign_id` on every table; isolation is your responsibility
- Harder to migrate *from* if you later want local-first
- RLS adds complexity but maps directly to the status model

**Best for:** Server-hosted multi-user web app.

---

### Option C: SurrealDB (multi-model)

[SurrealDB](https://surrealdb.com) combines document, graph, and relational models in one engine. Written in Rust. [Supports graph relationships natively](https://surrealdb.com/docs/surrealdb/models/graph) with `RELATE` syntax.

**Why it's interesting:**

- Graph relationships are first-class: `RELATE player:kael->frequents->location:rusty_anchor`
- No impedance mismatch -- a node is both a document and a graph vertex
- Record links replace foreign keys with direct references
- Built-in auth and permissions (maps to GM-only/Known visibility)
- Can run embedded (in-process) or as a server
- [Real-time live queries](https://surrealdb.com) and WebSocket integration
- Used at Nvidia, Salesforce, HSBC, Samsung ([source](https://surrealdb.com))

**Why to be cautious:**

- Still maturing -- SurrealDB 2.0 is recent; the API has had breaking changes
- Smaller ecosystem -- fewer ORMs, migration tools, community answers
- Embedding story less proven than SQLite (decades of production use vs. years)
- Technology lock-in risk if development stalls or pivots
- You'd be debugging SurrealDB issues on top of your own product

**Best for:** Teams willing to bet on a younger technology for significantly better graph query ergonomics.

---

### Option D: Hybrid -- SQLite/PostgreSQL + in-memory graph index

Use a relational database as the durable store (same schema as A or B), but maintain an in-memory graph index for traversals. Built from the database on startup, kept in sync as mutations happen.

**Libraries:**

- [petgraph](https://github.com/petgraph/petgraph) (Rust) -- mature graph data structure library
- [graphology](https://graphology.github.io/) (JavaScript) -- full-featured graph library

**Why it's interesting:**

- Best of both worlds: relational durability + fast in-memory graph traversals
- Campaign-scale data fits easily in memory (a few thousand nodes = a few MB)
- Proper adjacency list / hash map structure optimized for BFS/DFS

**Why it might be overkill:**

- Recursive CTEs are already fast enough for shallow traversals on small data
- Two representations of the same data that must stay in sync
- Only pays off if graph query patterns become complex (deep traversals, shortest paths, clustering)

**Best for:** Natural evolution if/when recursive CTEs become painful, without changing the persistence layer.

---

### Option E: Event-sourced mutations

An architectural layer (not a database choice) that stores every change as an immutable event and materializes current state from the event log. Layers on top of any storage option.

**Why it's tempting for Loreweaver:**

- Retconning maps naturally to events: a retcon is an event that marks previous events as superseded
- "Revealed in Session 14" is just an event with a timestamp
- Full audit trail: who changed what, when, and why
- Undo/redo comes for free
- The AI suggestion -> GM review -> accept/reject workflow is fundamentally event-shaped

**Why to avoid it as a foundation:**

- Massive implementation complexity for a greenfield project
- Querying requires materializing views -- you're building two systems
- The retcon use case is adequately served by a status field (`retconned`) -- no time-travel needed
- "Revealed in Session 14" can be a `revealed_at` column
- Can always add event sourcing to a specific hot path later

**Verdict:** Borrow the pattern for specific features (the AI suggestion queue is naturally event-shaped), but don't build the whole system on it.

---

### Option F: Turso / libSQL (hosted SQLite, database-per-campaign)

[Turso](https://turso.tech) is a hosted platform built on [libSQL](https://github.com/tursodatabase/libsql) (a fork of SQLite). The key idea: instead of one database with a `campaign_id` column on every table, **each campaign gets its own database**.

**Why this is compelling for Loreweaver:**

- **Natural tenant isolation**: each campaign is its own database -- no cross-campaign data leaks, no `WHERE campaign_id = ?` on every query
- **Automatic schema propagation**: define the schema on a parent; [Turso pushes it to all child databases](https://turso.tech/blog/database-per-tenant-architectures-get-production-friendly-improvements)
- **Native vector search**: libSQL has built-in vector search support
- **SQLite-compatible**: same SQL dialect, same recursive CTEs, accessed over HTTP
- **Future local-first path**: libSQL can replicate a server database to a local SQLite file -- offline support without a rewrite
- **Affordable at scale**: up to [10k databases on the $29/month plan](https://turso.tech/multi-tenancy), millions via API

**Concerns:**

- Vendor dependency (mitigated by libSQL being open source -- you can self-host)
- Newer platform -- smaller community, less battle-tested
- Cross-campaign queries require [attaching multiple databases](https://turso.tech/multi-tenancy) to one connection
- More limited server-side logic than PostgreSQL (no stored procedures, limited triggers)

**Best for:** Clean campaign isolation without multi-tenancy complexity, with a path to local-first.

---

## Recommendation

**For a server-hosted web app, two strong paths:**

### Path 1: PostgreSQL (recommended starting point)

The default answer for server-hosted web apps.

| Aspect | How PostgreSQL handles it |
|---|---|
| Graph storage | Adjacency tables (nodes, edges, blocks) with JSONB properties |
| Graph traversal | Recursive CTEs; Apache AGE for Cypher if needed later |
| Status filtering | Row-level security (RLS) maps directly to gm_only/known/retconned |
| Full-text search | tsvector/tsquery (built-in) |
| AI embeddings | pgvector (mature, widely deployed) |
| Multi-user | Connection pooling, concurrent access, RLS |
| Hosting | Supabase, Neon, RDS, Railway, etc. |
| Campaign isolation | `campaign_id` column on every table; RLS for enforcement |

**Start here because:**

1. You need a server anyway -- PostgreSQL is the natural fit
2. Multi-user is inherent in a web app -- RLS, connection pooling, concurrent access are built for this
3. pgvector + full-text search work out of the box
4. Unmatched ecosystem -- every hosting provider, every ORM, every problem already solved
5. The adjacency-table schema is portable to SQLite/Turso later if needed

### Path 2: Turso / libSQL (the interesting alternative)

One database per campaign, hosted on Turso.

| Aspect | How Turso handles it |
|---|---|
| Graph storage | Same adjacency tables, same SQL |
| Graph traversal | Recursive CTEs (SQLite-compatible) |
| Status filtering | Application-level filtering (no RLS equivalent) |
| Full-text search | FTS5 (SQLite built-in) |
| AI embeddings | Native libSQL vector search |
| Multi-user | Application-level; one DB per campaign |
| Hosting | Turso platform or self-hosted libSQL |
| Campaign isolation | Architectural -- each campaign IS a database |

**Consider this if:**

- Campaign isolation is a strong requirement (regulatory, self-hosted customers)
- You want a path to "download your campaign" / offline mode
- You prefer simpler per-campaign queries over multi-tenant complexity

---

## Sources

### Databases and Extensions

- [PostgreSQL pgvector](https://github.com/pgvector/pgvector) -- vector similarity search for PostgreSQL
- [Apache AGE](https://age.apache.org/) -- graph database extension for PostgreSQL (Cypher queries)
- [Apache AGE on Azure](https://learn.microsoft.com/en-us/azure/postgresql/azure-ai/generative-ai-age-overview) -- managed PostgreSQL with AGE
- [SurrealDB](https://surrealdb.com) -- multi-model database (document + graph + relational)
- [SurrealDB graph model docs](https://surrealdb.com/docs/surrealdb/models/graph)
- [SurrealDB knowledge graphs](https://surrealdb.com/solutions/knowledge-graphs)
- [Turso](https://turso.tech) -- hosted SQLite/libSQL platform
- [Turso multi-tenancy / database-per-tenant](https://turso.tech/multi-tenancy)
- [Turso database-per-tenant improvements](https://turso.tech/blog/database-per-tenant-architectures-get-production-friendly-improvements)
- [libSQL (GitHub)](https://github.com/tursodatabase/libsql) -- open-source SQLite fork

### SQLite Extensions

- [sqlite-vec](https://github.com/asg017/sqlite-vec) -- vector search extension, stable v0.1.0, C with no dependencies
- [sqlite-vec (Mozilla Builders)](https://builders.mozilla.org/project/sqlite-vec/) -- Mozilla endorsement
- [cr-sqlite](https://github.com/vlcn-io/cr-sqlite) -- CRDT-based multi-writer replication for SQLite
- [simple-graph](https://github.com/dpapathanasiou/simple-graph) -- minimal property graph pattern in SQLite
- [sqlite-graph](https://github.com/agentflare-ai/sqlite-graph) -- alpha-stage Cypher query support for SQLite

### Graph in SQL

- [SQL/PGQ in PostgreSQL (EDB)](https://www.enterprisedb.com/blog/representing-graphs-postgresql-sqlpgq) -- the emerging ISO standard for graph queries in SQL
- [SQLite forum: Will SQLite support GQL/SQL/PGQ?](https://sqlite.org/forum/info/f2eaf07826eb25f4) -- official stance: no native support planned
- [Graph Databases & Query Languages in 2025 (Medium)](https://medium.com/@visrow/graph-databases-query-languages-in-2025-a-practical-guide-39cb7a767aed)

### Local-First and CRDTs

- [Ink & Switch: Local-first software](https://www.inkandswitch.com/essay/local-first/) -- foundational essay on local-first principles
- [OctoBase](https://octobase.dev/) -- local-first collaborative database using CRDTs with a Block abstraction
- [awesome-local-first](https://github.com/alexanderop/awesome-local-first) -- curated list of local-first resources
- [CRDT.tech](https://crdt.tech/) -- canonical CRDT resource
- [The CRDT Dictionary (2025)](https://www.iankduncan.com/engineering/2025-11-27-crdt-dictionary/) -- comprehensive field guide

### Other

- [Hacker News: SQLite Graph Extension (alpha)](https://news.ycombinator.com/item?id=45751339) -- community discussion on sqlite-graph
- [Tonsky: Local, first, forever](https://tonsky.me/blog/crdt-filesync/) -- critique and alternative perspective on local-first
