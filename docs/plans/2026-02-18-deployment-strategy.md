# Loreweaver — Deployment Strategy

## Decision

**PostgreSQL, local-first development, one remote environment that grows into production.**

Provider and tooling choices (Hetzner vs UpCloud, Coolify vs Kamal, etc.) are deliberately deferred. This document captures the deployment *strategy* — the shape of the environments and workflow — not the specific infrastructure. See [solo dev deployment landscape](../discovery/deployment/solo_dev_deployment_landscape.md) for the full provider/tool exploration.

---

## Context

Loreweaver is being built by a solo developer. The previous deployment exploration ([EU deployment landscape](../discovery/deployment/eu_deployment_landscape.md)) assumed team-scale workflows: PR preview environments with database branching, automated CD pipelines, and isolated databases per pull request. Those assumptions don't match the current reality.

**What changed:**

- **Storage decision: PostgreSQL.** Turso/libSQL was the other candidate, but Turso confirmed they have no plans to expand beyond AWS. PostgreSQL is universally available across every EU provider and every deployment tier. See [PostgreSQL vs Turso decision](../discovery/archive/2026-02-18-postgres-vs-turso.md).
- **Solo developer.** No team contention on the database. No need for PR-based isolation. The expensive-test-data problem (audio transcription + AI extraction costs $2+ per session) is real, but solvable with `pg_dump` snapshots rather than copy-on-write branching.
- **EU data sovereignty.** Preferred, not mandatory. EU-headquartered providers preferred; non-EU providers acceptable where no EU alternative exists for a needed capability.

---

## Environments

Two environments. No staging.

### Local (development)

Everything runs on the developer's machine:

- **5 apps** via Docker Compose or `turbo dev`: `site` (Astro dev server), `web` (Vite dev server), `api` (Hono), `collab` (Hocuspocus), `worker` (job consumer)
- **PostgreSQL** in a Docker container (or native install)
- **No remote dependencies** — development works fully offline

This is the primary working environment. All feature development, debugging, and testing happens here.

### Remote (production)

A single server running the same 5 apps + PostgreSQL, accessible via a public domain. This server starts as a sandbox ("see it running outside localhost") and grows into production when there are real users.

- **Deploy method:** TBD. Docker Compose via SSH is the floor; Kamal or Coolify are the likely upgrades. See [solo dev deployment landscape](../discovery/deployment/solo_dev_deployment_landscape.md) for options.
- **Provider:** TBD. An EU VPS provider (Hetzner, UpCloud, Scaleway, etc.) is the likely choice. See [EU provider comparison](../discovery/deployment/solo_dev_deployment_landscape.md#eu-native-provider-landscape).
- **Database:** PostgreSQL on the same server (Docker container) initially. Managed PostgreSQL from the VPS provider is the likely upgrade path when the operational overhead of self-managing becomes friction.

### No staging environment

The developer is the only user. "Deploy and check" is the staging process. If this changes (team grows, real users depend on uptime), a staging environment can be added without architectural changes — it's just another instance of the same Docker Compose setup.

---

## Database Strategy

### PostgreSQL everywhere

The same PostgreSQL schema, Drizzle ORM configuration, and migration system runs in every environment:

- **Local dev:** PostgreSQL in Docker (or native)
- **Remote:** PostgreSQL in Docker → managed PostgreSQL (when the operational overhead justifies the cost)
- **Self-hosted (customer):** Customer-managed PostgreSQL (Docker container, managed service, or existing instance)
- **CI:** PostgreSQL service container or testcontainers

### "Branching" without a branching provider

For a solo developer, the safety of database branching — experiment freely, roll back to known-good state — is achieved with snapshots:

```bash
# Before risky migration or experiment
pg_dump mydb > snapshots/before-experiment.sql

# Roll back if needed
dropdb mydb && createdb mydb && psql mydb < snapshots/before-experiment.sql
```

For a dev database with realistic data (50-200MB), this takes 2-5 seconds. If this becomes too slow (database grows past ~1GB, branching multiple times per day), Neon can be layered in as a drop-in replacement — it's PostgreSQL, so the swap is a connection string change, not an architecture change.

### pg-boss for job queue

With PostgreSQL as the foundation, pg-boss (a PostgreSQL-native job queue) is the natural choice for the worker. No additional infrastructure required — the job queue lives in the same database.

---

## Self-Hosting Story

The same codebase runs on customer infrastructure. The deployment strategy must not introduce dependencies that self-hosters can't replicate.

**What a self-hoster runs:**

```
docker-compose.yml
├── site      → nginx serving Astro build (landing page, blog — port 80/443, /*)
├── web       → nginx serving Vite build (SPA — /app/*)
├── api       → Hono container (port 3001)
├── collab    → Hocuspocus container (port 3002)
├── worker    → job consumer container
└── postgres  → PostgreSQL container (or connect to existing instance)
```

**What this strategy preserves:**

- No managed-service dependencies in the application code. The app connects to PostgreSQL via a standard connection string — it doesn't care whether that's a Docker container, a managed instance, or Neon.
- No platform-specific deploy tooling in the application. Coolify/Kamal/Railway are deployment mechanisms, not application dependencies. The self-hoster uses `docker compose up`.
- LLM provider remains pluggable. Hosted instance uses managed keys; self-hosters bring their own.

---

## What This Strategy Defers

These decisions are explicitly postponed until there's a reason to make them:

| Decision | Deferred until |
|---|---|
| **VPS provider** (Hetzner, UpCloud, Scaleway, etc.) | First remote deployment |
| **Deploy tool** (Kamal, Coolify, Docker Compose, etc.) | First remote deployment |
| **Managed PostgreSQL** vs self-managed | Self-managed DB ops become friction |
| **Database branching** (Neon) | `pg_dump` snapshots feel too slow |
| **PR preview environments** | Team grows beyond solo dev |
| **CDN for static assets** | User base grows beyond single-region |
| **Multi-server architecture** | Single VPS becomes a bottleneck |

Each of these can be adopted independently without changing the application architecture. The strategy is designed so that the application code doesn't know or care which deployment tier it's running on.

---

## References

- [Solo dev deployment landscape](../discovery/deployment/solo_dev_deployment_landscape.md) — full provider and tooling exploration
- [EU deployment landscape](../discovery/deployment/eu_deployment_landscape.md) — team-scale deployment options and database branching providers
- [PostgreSQL vs Turso decision](../discovery/archive/2026-02-18-postgres-vs-turso.md) — why PostgreSQL over Turso/libSQL
- [SPA project structure](./2026-02-14-project-structure-spa-design.md) — the 4-app architecture this strategy deploys
