# SQLite over PostgreSQL — Database Re-evaluation

> **Decided: SQLite files (database-per-campaign), with [Turso Database](https://github.com/tursodatabase/turso) as the upgrade path.** This supersedes the [previous PostgreSQL decision](../archive/discovery/2026-02-18-postgres-vs-turso.md). The original decision was sound given its assumptions, but those assumptions changed when PR preview environments became a priority and the deployment infrastructure crystallized around Hetzner + k3s.

## Context

The [previous decision](../archive/discovery/2026-02-18-postgres-vs-turso.md) chose PostgreSQL over Turso/libSQL. The reasoning was:

1. **Turso confirmed no plans to expand beyond AWS** — conflicting with EU data sovereignty preferences.
2. **PostgreSQL is universally available** across every EU provider and deployment tier.
3. **Ecosystem maturity** — pgvector, pg-boss, Hocuspocus PostgreSQL adapter, universal self-hosting knowledge.

That decision also explicitly deferred PR preview environments ("deferred until team grows beyond solo dev") and database branching ("deferred until `pg_dump` snapshots feel too slow"). Both of those assumptions have changed.

## What changed

### PR preview deploys are happening now, not later

The deployment strategy shifted to k3s on Hetzner with PR preview environments as a core workflow — not a deferred luxury. This was motivated by the value of deployment integration testing for a multi-service architecture (Traefik proxying, WebSocket routing, container networking) that works locally but can break in deployment. Branch deploys catch these issues before merge.

With PostgreSQL, PR preview database branching requires either:

- **Neon** (US company, AWS-only, $5+/mo for branching, external dependency) — exactly the kind of managed service dependency the project wants to avoid.
- **`pg_dump`/`pg_restore`** — works, but slow for larger databases, requires PostgreSQL tooling in CI, and produces a full copy rather than a lightweight branch.

With SQLite files, PR preview branching is `cp`. Copy the platform database. Copy the campaign databases you want. Done. No external service, no special tooling, no cost. The branch is files in a directory.

### The Turso managed service is no longer relevant

The previous decision rejected the Turso/libSQL path partly because Turso's managed cloud platform runs exclusively on AWS. That concern no longer applies because the plan doesn't use Turso's managed service at all.

The architecture is self-hosted SQLite files on a Hetzner Volume. No managed database provider. No third-party cloud dependency. No EU sovereignty concern. The database is files on disk in a German data center.

### Turso Database (the Rust rewrite) provides a clean upgrade path

[Turso Database](https://github.com/tursodatabase/turso) is a Rust rewrite of SQLite — not the managed Turso cloud service, and not libSQL (the C fork). It is MIT licensed, in-process, SQLite-compatible at the file format level, and provides:

- **`BEGIN CONCURRENT`** — MVCC-based concurrent writes (experimental, not needed given the architecture, but removes the single-writer constraint if future features require it).
- **Native vector search** — equivalent to what pgvector provides for PostgreSQL.
- **FTS powered by Tantivy** — full-text search.
- **JavaScript binding** (`@tursodatabase/database`) — same usage pattern as better-sqlite3.
- **Cross-platform** — Linux, macOS, Windows, WebAssembly.

Currently in beta (v0.5.0 as of March 2026, 17.7k GitHub stars, 206 contributors, deterministic simulation testing + Antithesis testing, $1k data corruption bug bounty). The upgrade path is: swap the database driver in `@familiar-systems/db`, test against existing `.db` files, deploy. No infrastructure changes.

### The concurrent write concern is addressed by defense in depth

The previous analysis flagged SQLite's single-writer limitation as a potential problem with multiple containers (API, collab, worker) writing to the same database. On closer examination, concurrent write contention doesn't happen in familiar.systems's architecture — but the database is protected even if that assumption is violated.

**Layer 1 — Architectural serialization.** The only write path to campaign data is through Hocuspocus. Human edits and AI agent writes (via DirectConnection) both funnel through the Y.Doc and flush as a single debounced `onStoreDocument` write every few seconds. The worker writes batch results to different campaign databases than the one being actively edited. The API server mostly reads. There is no scenario where two processes contend on the same campaign database file.

**Layer 2 — SQLite's built-in locking.** WAL mode (readers never block writers, writers never block readers) plus `PRAGMA busy_timeout = 5000` (a writer that collides with another writer retries for up to 5 seconds before returning `SQLITE_BUSY`). If layer 1's assumption is ever violated — a bug, a race condition, a new write path added later — SQLite itself prevents corruption. The second writer waits or gets a retryable error. Data is never corrupted. These two PRAGMAs are set once in the database connection setup and require no application-level locking code.

**Layer 3 — Future (Turso Database).** `BEGIN CONCURRENT` provides MVCC where concurrent writes don't even conflict unless they touch the same rows. Available when Turso Database exits beta.

SQLite's internal locking is battle-tested across billions of deployments. No application-level mutex (Node.js lock, file lock, etc.) should be implemented — it would be redundant with SQLite's own locking and introduces deadlock/crash-recovery risks that SQLite already handles correctly.

**Required PRAGMAs** (set once per connection in `@familiar-systems/db`):

```sql
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 5000;
```

### The ecosystem advantages of PostgreSQL aren't load-bearing yet

The original decision cited pgvector, pg-boss, and the Hocuspocus PostgreSQL adapter as ecosystem advantages. None of these have been integrated:

- **pgvector** → SQLite/libSQL vector search and Turso Database's native vector support are adequate at familiar.systems's scale (~20,000 vectors per large campaign — orders of magnitude below where the maturity gap matters).
- **pg-boss** → familiar.systems's job volume is tiny (a few SessionIngest jobs per day per GM). A simple polling job table works. pg-boss solves problems (exponential backoff, priority queues, dead letter queues) that won't be needed for a long time.
- **Hocuspocus PostgreSQL adapter** → A SQLite storage adapter needs to be written or found regardless of which database is chosen. This is application code, not an infrastructure dependency.

## The decision

### Database-per-campaign with SQLite files

One platform database holds users, campaign metadata, memberships, and the job queue. Each campaign gets its own database containing all graph content.

```
Hetzner Volume mounted at /data/
├── platform.db                    # Users, campaigns, memberships, job queue
├── campaigns/
│   ├── campaign-abc.db            # First campaign's graph data
│   ├── campaign-def.db            # Second campaign's graph data
│   └── ...
└── previews/
    └── pr-42/                     # PR preview copy
        ├── platform.db
        └── campaigns/
            └── campaign-abc.db
```

### PR preview access control

Preview environments contain copied production data and must be restricted to contributors. Three layers of protection:

**Traefik basic auth on preview subdomains.** Traefik on k3s supports basic auth middleware via Ingress annotations. Shared credentials that all contributors know. This is the outer gate — keeps random visitors out.

**Hanko authentication.** Preview environments run the same application code, which requires Hanko login. The preview points at the same Hanko instance as production, so contributors authenticate with their real accounts.

**Platform DB filtering.** After copying the platform database for a preview, a cleanup script deletes all users except contributors:

```sql
DELETE FROM users WHERE email NOT IN ('michael@...', 'contributor@...');
-- Foreign key cascades clean up memberships and related rows
```

The list of contributor emails lives in a version-controlled `contributors.sql` file in the repo. Even if someone bypasses both Traefik basic auth and Hanko, the preview database contains no user data beyond the contributor list.

### What this gives us

**Structural campaign isolation.** Cross-campaign data leaks are impossible — the wrong data doesn't exist in the database being queried. No `campaign_id` on every table, every query, every join. The database IS the campaign.

**Trivial PR preview branching.** `cp` the files. No external service, no API calls, no cost. Cleanup is `rm -rf`. This is the primary motivator for the decision change.

**Zero-infrastructure local dev.** No Docker PostgreSQL container. SQLite files on disk. `:memory:` databases for tests — instant, in-process, zero config. `turbo dev` starts all apps with no service dependencies.

**Simpler self-hosting.** A self-hoster runs the same architecture with SQLite files on their machine. No PostgreSQL server to install, configure, or maintain. The application code is identical — just file paths instead of connection strings.

**Natural "campaign as data" model.** Backup a campaign = copy a file. Export a campaign = send the file. Delete a campaign = delete the file. Archive a campaign = move the file to cold storage.

**Decoupled data from compute.** Database files live on a Hetzner Volume. Upgrade VPS by detaching the Volume, attaching to a new server. Floating IP reassignment gives zero-downtime cutover. Data lifetime is independent of server lifetime.

### What this costs us

**Two-tier connection management.** The application routes to the correct campaign database based on the request. This is a connection-per-campaign pattern rather than a single connection pool. Drizzle handles this via `createClient({ url: "file:./campaigns/abc.db" })`, but it's a different pattern to manage.

**Cross-campaign queries require the platform database.** "Show me all my campaigns" queries the platform database. "Search across all my campaigns" requires either ATTACH (currently read-only) or application-level aggregation. This is acceptable — the platform database answers cross-campaign questions, and campaign databases answer within-campaign questions.

**Schema migrations across N databases.** Adding a column to a campaign table requires running the migration against every campaign database. Drizzle can handle this, but the "iterate over all campaign DBs" runner needs to be built. Turso's managed platform automates this via parent→child propagation, but that's not available in the self-hosted path.

**Hocuspocus storage adapter.** ~~Hocuspocus has a PostgreSQL adapter. A SQLite adapter needs to be written or found.~~ Resolved: the [Hocuspocus Architecture ADR](.../archive/discovery/plans/2026-03-14-hocuspocus-architecture.md) stores Y.Doc blobs as a nullable BLOB column in the campaign database, materialized to relational tables via `onStoreDocument`. No separate adapter needed.

**Less mainstream for self-hosters.** "Run PostgreSQL" is universal knowledge. "Your campaigns are SQLite files" is less familiar, though arguably simpler in practice.

## Storage tier progression

**Tier 1 (now):** SQLite files on a Hetzner Volume. WAL mode + `busy_timeout` for write safety. Hocuspocus serializes all campaign writes through `onStoreDocument`. No concurrent write contention by design, with SQLite's built-in locking as the safety net. See "defense in depth" above.

**Tier 2 (when Turso Database exits beta):** Swap in Turso Database. Same files, better engine. `BEGIN CONCURRENT` available if needed. No infrastructure changes — only the database driver in `@familiar-systems/db` changes.

Both tiers use the same file-on-disk model. The Hetzner Volume, the PR preview copy logic, and the Pulumi infrastructure don't change between tiers. Note: the [Hocuspocus Architecture ADR](.../archive/discovery/plans/2026-03-14-hocuspocus-architecture.md) makes Object Storage the source of truth with local disk as a working cache (campaign checkout/checkin lifecycle with ~30-second writeback).

## Self-hosting story

```
docker-compose.yml
├── reverse-proxy → Caddy/Traefik serving two apexes on the host's ports
├── site          → nginx serving Astro build (marketing apex: /)
├── web           → nginx serving Vite build (app apex: /)
├── platform      → Rust container (app apex: /api)
├── campaign      → Rust container (app apex: /campaign)
├── worker        → job consumer container
└── (no database container needed)

volumes:
  ./data:/data    # SQLite files on the host
```

All containers mount the data directory. No database server to install, configure, or maintain. For a single-GM self-hosted setup, this is meaningfully simpler than the PostgreSQL equivalent.

## Open questions

1. ~~**Hocuspocus SQLite storage adapter.** Does one exist, or does it need to be written?~~ Resolved: Y.Doc blobs live as a nullable BLOB column in the campaign database, materialized to relational tables via `onStoreDocument`. No separate adapter needed. See [Hocuspocus Architecture ADR](.../archive/discovery/plans/2026-03-14-hocuspocus-architecture.md).

2. **Drizzle multi-database ergonomics.** The "resolve campaign ID → get database client → run query" pattern needs a clean abstraction. Turso has a [starter template with Drizzle](https://turso.tech/blog/creating-a-multitenant-saas-service-with-turso-remix-and-drizzle-6205cf47) that demonstrates this for their managed service — the pattern should transfer to local files.

3. **Schema migration runner.** A script that iterates over all campaign databases and runs pending migrations. Straightforward to build, but needs to exist before the first schema change after launch.

4. **ATTACH for cross-campaign queries.** Currently read-only in libSQL. If Turso Database supports read-write ATTACH, cross-campaign operations (e.g. "copy this NPC to another campaign") become possible without application-level orchestration.

## References

- [Previous PostgreSQL decision](../archive/discovery/2026-02-18-postgres-vs-turso.md) — superseded by this document
- [Turso Database (Rust SQLite rewrite)](https://github.com/tursodatabase/turso) — MIT licensed, in-process, SQLite-compatible
- [Turso Database JavaScript binding](https://www.npmjs.com/package/@tursodatabase/database)
- [Infrastructure (k3s on Hetzner)](../../plans/2026-03-30-infrastructure.md) — deployment infrastructure this decision integrates with
- [Hanko](https://www.hanko.io/) — authentication provider for familiar.systems
- [SQLite WAL mode](https://www.sqlite.org/wal.html) — write-ahead logging for concurrent read/write
- [Deployment strategy (archived)](../../archive/plans/2026-02-18-deployment-strategy.md) — superseded; see [current infrastructure plan](../../plans/2026-03-30-infrastructure.md)
- [Storage overview](../archive/discovery/2026-02-14-storage-overview.md) — original storage analysis
