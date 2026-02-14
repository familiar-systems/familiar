# PostgreSQL vs Turso — Storage Re-evaluation

## Context

The [storage overview](./storage_overview.md) evaluated storage options before any architectural decisions were made. It recommended PostgreSQL as the starting point, with Turso as an interesting alternative.

Since then, several architectural decisions have been made:

- **SPA architecture** with 4 apps: `web` (static), `api` (Hono+tRPC), `collab` (Hocuspocus), `worker` (job consumer). See [SPA project structure](../../plans/2026-02-14-project-structure-spa-design.md).
- **AI workflow unification** with durable suggestions, agent conversations, and the interactive/batch AI split. See [AI workflow design](../../plans/2026-02-14-ai-workflow-unification-design.md).
- **Drizzle ORM** as the database abstraction (supports both PostgreSQL and SQLite/libSQL).
- **Self-hosted deployment** as a first-class requirement — the same codebase must run on the customer's own infrastructure.

This document re-evaluates PostgreSQL vs Turso in light of these decisions.

---

## What Changed Since the Storage Overview

### Decisions that were assumed to favor PostgreSQL

The SPA project structure specified **pg-boss** (PostgreSQL-backed job queue) and **pgvector** (AI embeddings). These were treated as hard dependencies on PostgreSQL. On re-examination:

**pg-boss is a convenience, not a necessity.** Loreweaver's job volume is tiny — a few SessionIngest jobs per day per GM. A simple polling job table (`SELECT ... WHERE status = 'pending' LIMIT 1`) works at this scale. pg-boss solves problems (exponential backoff, priority queues, dead letter queues) that Loreweaver won't need for a long time.

**pgvector vs libSQL vector search is not a differentiator at Loreweaver's scale.** A large campaign has ~20,000 blocks — that's 20,000 vectors. pgvector handles tens of millions; libSQL handles this trivially. Both offer SQL-native interfaces. The maturity gap (pgvector is more battle-tested, libSQL vector search is newer) doesn't matter when the dataset is 3-4 orders of magnitude below where performance differences emerge.

### New concerns that emerged

**Testing with realistic data is expensive.** Loreweaver's test data requires actual audio transcription and AI processing ($2+ in tokens per session, plus wall clock time). You can't faker.js a realistic campaign — the entity mentions, relationship graph, suggestion history, and journal narrative are all deeply interconnected. Once you have a realistic seed database, the ability to branch it instantly (rather than rebuilding it) saves real time and money.

**Campaign isolation is a security-critical concern.** Every query in a multi-tenant PostgreSQL database must include `campaign_id`. Forget it once — in a query, a join, an RLS policy — and you have a cross-campaign data leak. This is one of the most common security bugs in SaaS applications. It's enforced by discipline, not structure.

---

## The Two Paths

### Path 1: PostgreSQL

Standard single-database multi-tenancy. All campaigns share one database with `campaign_id` on every table.

**Production deployment:**
```
UpCloud (or any cloud)
┌─────────────────────────────────────┐
│  apps/api + apps/collab + apps/worker │
│              │                       │
│              ▼                       │
│     PostgreSQL (managed)             │
│     - All campaigns in one database  │
│     - campaign_id on every table     │
│     - RLS for tenant isolation       │
│     - pgvector for embeddings        │
│     - pg-boss for job queue          │
└─────────────────────────────────────┘
```

**Development workflow:**
- Local: Docker + PostgreSQL container
- Testing: testcontainers (spin up PostgreSQL per test suite, ~2-3s startup)
- Branching: Neon free tier (unlimited branches) or dump/restore
- CI: PostgreSQL service container

**Self-hosted:** Run PostgreSQL. Universal — every cloud, every VPS, Docker Compose. Well-documented, massive community.

**What it gives you:**
- The most conventional path. Fewest surprises. Largest ecosystem.
- Every ORM, migration tool, hosting provider, tutorial assumes PostgreSQL.
- pgvector is the most mature SQL-native vector search.
- RLS provides database-level tenant isolation (with caveats — see below).
- pg-boss is a turnkey job queue.

**What it costs you:**
- `campaign_id` on every table, every query, every join, every RLS policy. Miss it once → data leak.
- RLS policies that span tables (e.g., mention visibility requires joining to parent block's status) add complexity and can affect query performance.
- Testing requires Docker or an external PostgreSQL instance. No `:memory:` option.
- No native database branching — requires Neon (external service) or dump/restore.

### Path 2: Turso / libSQL (database-per-campaign)

Each campaign gets its own database. A separate platform database holds cross-campaign concerns.

**Production deployment:**
```
UpCloud (or any cloud)
┌─────────────────────────────────────────────────┐
│  apps/api + apps/collab + apps/worker            │
│              │                                   │
│    ┌─────────┼──────────────┐                    │
│    ▼         ▼              ▼                    │
│  Platform   Campaign A    Campaign B    ...      │
│  Database   Database      Database               │
│  (libSQL)   (libSQL)      (libSQL)               │
│                                                  │
│  users       nodes         nodes                 │
│  campaigns   blocks        blocks                │
│  memberships relationships relationships         │
│  job_queue   mentions      mentions              │
│  billing     sessions      sessions              │
│              suggestions   suggestions           │
│              conversations conversations         │
└─────────────────────────────────────────────────┘
```

**Two-tier architecture:**

| Platform database (one) | Campaign databases (many) |
|---|---|
| Users, authentication | Nodes, blocks, edges |
| Campaign metadata (name, owner, memberships) | Sessions, journal entries |
| Job queue | Suggestions, suggestion batches |
| Billing | Agent conversations |
| Cross-campaign search index (if needed) | Session sources |

The platform database is standard — one database, no per-campaign isolation needed. Campaign databases hold all graph content. The application routes to the correct campaign database based on the request.

**Development workflow:**
- Local: libSQL or SQLite files. No Docker needed.
- Testing: `:memory:` databases — instant, in-process, zero config.
- Branching: Native copy-on-write branching (Turso) or just copy a file (libSQL/SQLite).
- CI: No service containers needed. Tests run against in-memory databases.

**Self-hosted:** Run libSQL server, or for a single-GM setup, use SQLite files directly. Less mainstream than PostgreSQL but fully open source.

**What it gives you:**
- **Structural campaign isolation.** Cross-campaign data leaks are impossible — the wrong data doesn't exist in the database you're querying.
- **Trivial testing.** `:memory:` databases with zero setup, instant creation, no Docker.
- **Native branching.** Copy-on-write database branches for development and CI.
- **Natural "campaign as data" model.** A campaign IS a database. Backup, export, or delete a campaign by operating on its database.
- **Simpler per-campaign queries.** No `WHERE campaign_id = ?` anywhere. Every query is campaign-scoped by definition.

**What it costs you:**
- **Two-tier architecture.** Two connection patterns, two migration targets, routing logic to pick the right database.
- **Cross-campaign queries are harder.** "Show me all my campaigns" queries the platform database. "Search across all my campaigns" requires either ATTACH (currently read-only on Turso) or application-level aggregation.
- **Connection management.** Each campaign is a separate connection. Turso's HTTP-based client mitigates this (no persistent connection pool needed), but it's a different pattern than a single PostgreSQL connection pool.
- **Schema migrations across N databases.** Turso handles this with parent→child schema propagation. Self-hosted libSQL would need a migration runner that iterates over campaign databases.
- **Less mainstream for self-hosters.** "Run PostgreSQL" is universal knowledge. "Run libSQL" is less familiar, though the actual complexity is lower (it's just SQLite-compatible).

---

## Head-to-Head Comparison

| Concern | PostgreSQL | Turso / libSQL |
|---|---|---|
| **Campaign isolation** | Convention (`campaign_id` + RLS). Discipline-dependent. | Structural (database-per-campaign). Cannot leak. |
| **Testing** | Docker + testcontainers (~2-3s startup). CI needs service config. | `:memory:` — instant, in-process, zero config. |
| **Database branching** | Neon (external service, free tier) or dump/restore. | Native copy-on-write. Instant. |
| **AI embeddings** | pgvector — mature, battle-tested. Overkill at Loreweaver's scale. | libSQL vector search — adequate at Loreweaver's scale. Newer. |
| **Full-text search** | tsvector/tsquery — powerful, built-in. | FTS5 — powerful, built-in. Roughly equivalent. |
| **Job queue** | pg-boss — turnkey, PostgreSQL-native. | Simple polling table. Fine at Loreweaver's scale. |
| **Cross-campaign queries** | Trivial: `WHERE user_id = ?` | ATTACH (read-only) or application-level aggregation. |
| **Schema migrations** | Run against one database. Standard. | Propagate across N databases. Turso automates this; self-hosted needs scripting. |
| **Connection management** | Single connection pool. Standard. | Connection-per-campaign. HTTP client mitigates pooling concerns. |
| **Self-hosted simplicity** | Run PostgreSQL. Universal knowledge. | Run libSQL. Less mainstream but arguably simpler (no server config for basic use). |
| **ORM support** | Drizzle's primary target. Best documentation. | Drizzle supports libSQL/Turso. Works, but PostgreSQL gets more ecosystem attention. |
| **Ecosystem** | Massive. Every problem already solved. | Smaller. Growing. Turso invests heavily in developer experience. |
| **Risk profile** | Low. You cannot go wrong with PostgreSQL. | Medium. Less beaten path. libSQL is open source and SQLite-compatible, limiting downside. |

---

## What Loreweaver's Data Actually Looks Like in Each Model

### A single campaign (PostgreSQL)

All data lives in one database. Campaign isolation via `campaign_id` column.

```sql
-- Every table has campaign_id
SELECT * FROM nodes WHERE campaign_id = 'abc' AND status != 'retconned';

-- Relationships: must check campaign_id on both sides of the join
SELECT r.* FROM relationships r
  JOIN nodes n ON r.source_node_id = n.id
  WHERE n.campaign_id = 'abc' AND r.status != 'retconned';

-- Mentions: visibility requires joining to parent block's status
SELECT m.*, b.status FROM mentions m
  JOIN blocks b ON m.source_block_id = b.id
  WHERE m.target_node_id = ? AND b.campaign_id = 'abc';
```

### A single campaign (Turso)

Campaign data lives in its own database. No campaign_id needed.

```sql
-- No campaign_id — this database IS the campaign
SELECT * FROM nodes WHERE status != 'retconned';

-- Relationships: simpler join, no campaign scoping
SELECT r.* FROM relationships r
  JOIN nodes n ON r.source_node_id = n.id
  WHERE r.status != 'retconned';

-- Mentions: same join for status, no campaign scoping
SELECT m.*, b.status FROM mentions m
  JOIN blocks b ON m.source_block_id = b.id
  WHERE m.target_node_id = ?;
```

### Cross-campaign (the dashboard)

```sql
-- PostgreSQL: trivial
SELECT c.* FROM campaigns c
  JOIN campaign_memberships cm ON c.id = cm.campaign_id
  WHERE cm.user_id = ?;

-- Turso: query the platform database (same simplicity)
-- Campaign metadata lives in the platform DB, not in campaign DBs
SELECT c.* FROM campaigns c
  JOIN campaign_memberships cm ON c.id = cm.campaign_id
  WHERE cm.user_id = ?;
```

The cross-campaign query difference only matters for operations that need to *aggregate content across campaigns* — e.g., "search all my campaigns for mentions of 'dragon'". This is a rare operation that could be solved by a search index in the platform database.

---

## Turso Database Management Mechanics

Every Turso database has two identifiers:

| Field | Example | Purpose |
|---|---|---|
| **Name** | `campaign-7f3a2b` | Human-readable, unique per organization. Lowercase, numbers, dashes. Max 64 chars. |
| **DbId** | `a1b2c3d4-e5f6-...` | UUID. Immutable internal identifier. |

The connection URL is derived from the name: `libsql://campaign-7f3a2b-yourorg.turso.io`

Databases are created programmatically via the [Turso Platform API](https://docs.turso.tech/api-reference/databases/create):

```typescript
// When a GM creates a new campaign
const response = await fetch(
  `https://api.turso.tech/v1/organizations/${org}/databases`,
  {
    method: "POST",
    headers: { Authorization: `Bearer ${TURSO_API_TOKEN}` },
    body: JSON.stringify({
      name: `campaign-${campaignId}`,
      group: "default",
    }),
  }
);
// Returns: { DbId: "uuid", Hostname: "...", Name: "campaign-..." }
```

Listing databases: `GET /v1/organizations/{org}/databases` returns all databases with their names, UUIDs, regions, and parent info (if branched).

### The platform database

With database-per-campaign, the platform database becomes remarkably simple — just routing and metadata:

```sql
-- Platform database (one, shared across all campaigns)

users (
  id          UUID PRIMARY KEY,
  email       TEXT UNIQUE NOT NULL,
  name        TEXT,
  created_at  TIMESTAMP
)

campaigns (
  id          UUID PRIMARY KEY,
  name        TEXT NOT NULL,
  owner_id    UUID REFERENCES users(id),
  db_name     TEXT NOT NULL,      -- Turso database name: "campaign-7f3a2b"
  db_id       TEXT,               -- Turso DbId (UUID), for API operations
  created_at  TIMESTAMP
)

campaign_memberships (
  campaign_id UUID REFERENCES campaigns(id),
  user_id     UUID REFERENCES users(id),
  role        TEXT NOT NULL,      -- 'gm' | 'player'
  PRIMARY KEY (campaign_id, user_id)
)

job_queue (
  id          UUID PRIMARY KEY,
  campaign_id UUID REFERENCES campaigns(id),
  type        TEXT NOT NULL,      -- 'transcribe' | 'draft_journal' | ...
  payload     JSON NOT NULL,
  status      TEXT NOT NULL DEFAULT 'pending',
  created_at  TIMESTAMP,
  started_at  TIMESTAMP,
  completed_at TIMESTAMP
)
```

Four tables. No nodes, no blocks, no relationships, no suggestions, no conversations. All graph content lives in the campaign databases.

Compare this to the PostgreSQL model where every table has `campaign_id`, every query filters by it, and RLS policies enforce it. The Turso model splits the concern: the platform database answers "which campaigns exist and who can access them?" (simple relational queries), and the campaign database answers "what's in this campaign?" (graph traversals, mention resolution, suggestion management — all the complex stuff, with zero multi-tenancy overhead).

### Request flow

```
1. GM navigates to /campaign/abc-123
2. apps/api looks up campaign in platform DB:
   SELECT db_name FROM campaigns
     JOIN campaign_memberships ON ...
     WHERE campaigns.id = 'abc-123' AND user_id = ?
   → returns db_name = "campaign-7f3a2b"

3. apps/api creates a Drizzle client for that campaign DB:
   const campaignDb = drizzle(
     createClient({ url: `libsql://campaign-7f3a2b-org.turso.io`, authToken })
   );

4. All subsequent queries use campaignDb:
   campaignDb.select().from(nodes).where(eq(nodes.status, 'known'))
   // No campaign_id. This database IS the campaign.
```

### Worker flow

```
1. Worker polls platform DB:
   SELECT * FROM job_queue WHERE status = 'pending' LIMIT 1

2. Job has campaign_id → look up db_name:
   SELECT db_name FROM campaigns WHERE id = job.campaign_id

3. Worker connects to campaign DB, runs the pipeline:
   const campaignDb = drizzle(createClient({ url: `libsql://...` }));
   // Transcribe, extract entities, create suggestions — all in campaign DB
```

### Self-hosted deployment

For a self-hoster (single GM, local deployment), the same architecture simplifies to SQLite files:

```
Self-hosted (single machine)
├── platform.db          # SQLite file — users, campaigns, job queue
├── campaigns/
│   ├── campaign-abc.db  # SQLite file — first campaign's data
│   └── campaign-def.db  # SQLite file — second campaign's data
```

No Turso account needed. No network. The application code is identical — `createClient({ url: "file:./campaigns/campaign-abc.db" })` instead of `libsql://...turso.io`. Drizzle doesn't care. This is arguably a simpler self-hosted story than PostgreSQL — no database server to install, configure, or maintain.

---

## Database Branching for Development

### The problem branching solves

Loreweaver's test data is expensive to create — audio transcription costs money, AI entity extraction costs tokens ($2+ per session), and building up a realistic campaign graph takes many processed sessions. Once you have a database with realistic data, the ability to branch it instantly (rather than rebuilding it) saves real time and money.

### PostgreSQL + Neon

One database, one branch, everything included:

```bash
# Branch the single database
neonctl branches create --name fix-suggestion-review

# Deploy preview with the branch connection string
DATABASE_URL=postgresql://...fix-suggestion-review...

# Cleanup
neonctl branches delete fix-suggestion-review
```

Simple — one command, one connection string. But you get every campaign in the branch, whether you need them or not.

For automated tests, you still need Docker + testcontainers (~2-3s startup per suite) or a running PostgreSQL instance. No `:memory:` option.

### Turso: Selective branching

With database-per-campaign, you choose which databases to branch:

```bash
# Branch the platform database
turso db create platform-feature-xyz --group default --from-db platform

# Branch only the campaign(s) you need
turso db create campaign-abc-feature-xyz --group default --from-db campaign-abc
```

Each branch is an instant copy-on-write snapshot. Writes to the branch don't affect the parent.

**Routing branched environments:** Branch the platform DB and rewrite its `db_name` entries to point to branched campaign DBs:

```sql
-- In the branched platform DB
UPDATE campaigns
  SET db_name = 'campaign-abc-feature-xyz'
  WHERE db_name = 'campaign-abc';

-- Remove campaigns that weren't branched
DELETE FROM campaigns
  WHERE db_name NOT IN ('campaign-abc-feature-xyz');
```

The application code doesn't change. It reads `db_name` from the platform DB and connects. The branched platform DB just points to branched campaign DBs.

**For automated tests:** No Turso, no network, no branching needed:

```typescript
// Instant, in-process, fully isolated
const platformDb = drizzle(createClient({ url: ":memory:" }));
const campaignDb = drizzle(createClient({ url: ":memory:" }));
await runMigrations(platformDb, "platform");
await runMigrations(campaignDb, "campaign");
await seed(platformDb, campaignDb);
// Run tests — instant setup, instant teardown
```

### Branching comparison

| | PostgreSQL + Neon | Turso |
|---|---|---|
| **Branch a preview env** | One command. Everything included. | Branch platform DB + selected campaign DBs. More commands, but you choose what to include. |
| **Routing in preview** | Change `DATABASE_URL` | Branch platform DB and rewrite `db_name` pointers |
| **Automated tests** | testcontainers (~2-3s) | `:memory:` (instant) |
| **Cleanup** | Delete one branch | Delete N branches (scriptable) |
| **Granularity** | All-or-nothing | Pick which campaigns to branch |

---

## Open Questions

These would need to be resolved before committing to Turso:

1. **Drizzle + multi-database ergonomics.** How does Drizzle handle creating a new database connection per request? Is there a clean pattern for "resolve campaign ID → get database client → run query"? Turso has a [starter template with Drizzle](https://turso.tech/blog/creating-a-multitenant-saas-service-with-turso-remix-and-drizzle-6205cf47) that demonstrates this.

2. **Self-hosted libSQL experience.** What does "run libSQL" look like for a self-hoster? Is it `docker run libsql/server`? How does schema propagation work outside of Turso's managed platform?

3. **ATTACH becoming read-write.** Turso has stated they're working on read-write ATTACH. If/when this ships, cross-campaign writes (e.g., "copy this NPC to another campaign") become possible without application-level orchestration.

4. **Hocuspocus + libSQL.** Does Hocuspocus's PostgreSQL storage adapter have a libSQL/SQLite equivalent, or would we need to write one? Hocuspocus stores Y.Doc state — this could live in the platform database or in the campaign database.

5. **Connection overhead at scale.** With 100 GMs and 3 campaigns each, the API server handles 300 potential campaign databases. Turso's HTTP-based client (no persistent connections) should handle this, but it's worth validating.

---

## Sources

### PostgreSQL
- [pgvector — open-source vector similarity search](https://github.com/pgvector/pgvector)
- [pgvector 0.8.0 performance improvements](https://www.instaclustr.com/education/vector-database/pgvector-key-features-tutorial-and-pros-and-cons-2026-guide/)
- [UpCloud managed PostgreSQL](https://upcloud.com/postgresql-managed-databases/)
- [Neon pricing (database branching)](https://neon.com/pricing)
- [Neon — open source serverless PostgreSQL](https://github.com/neondatabase/neon)

### Turso / libSQL
- [Turso pricing](https://turso.tech/pricing)
- [Turso database-per-tenant architecture](https://turso.tech/blog/database-per-tenant-architectures-get-production-friendly-improvements)
- [Turso read-only ATTACH](https://turso.tech/blog/introducing-read-only-database-attach-in-turso)
- [Turso AI & embeddings (libSQL vector search)](https://docs.turso.tech/features/ai-and-embeddings)
- [Turso + Drizzle multi-tenant tutorial](https://turso.tech/blog/creating-a-multitenant-saas-service-with-turso-remix-and-drizzle-6205cf47)
- [State of vector search in SQLite](https://marcobambini.substack.com/p/the-state-of-vector-search-in-sqlite)
- [Turso multi-tenancy overview](https://turso.tech/multi-tenancy)

### Turso Platform API
- [Create database](https://docs.turso.tech/api-reference/databases/create)
- [List databases](https://docs.turso.tech/api-reference/databases/list)
- [Turso concepts — database](https://docs.turso.tech/concepts)

### Testing & Development Workflow
- [Neon free tier — unlimited branches](https://neon.com/docs/introduction/plans)
- [PostgreSQL branching comparison (Xata vs Neon vs Supabase)](https://xata.io/blog/neon-vs-supabase-vs-xata-postgres-branching-part-2)
