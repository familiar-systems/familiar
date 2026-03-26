> **Superseded.** Deployment decided: Coolify on Hetzner with libSQL database-per-campaign. See [deployment strategy](../plans/2026-03-09-deployment-strategy.md). This document's research informed that decision.

# Loreweaver — EU Deployment & Development Lifecycle Landscape

## Context

This document maps the EU/EEA deployment landscape before any deployment decisions are made. The goal is to understand what providers exist, what capabilities they offer, and how database branching and continuous deployment workflows could work with each storage option (PostgreSQL or Turso/libSQL — still undecided).

**Driving requirements:**

- **EU/EEA data sovereignty**: EU/EEA-headquartered providers preferred (mild preference for Nordic/Scandinavian projects); non-EU providers with EU data residency acceptable where no EU alternative exists for a needed capability.
- **Hyperscaler avoidance**: Strong preference against AWS/GCP/Azure infrastructure. Independent EU providers (Hetzner, UpCloud, Exoscale, Scaleway, OVH) preferred. Hyperscaler-dependent services (e.g. Neon on AWS/Azure) are acceptable only if the capability they provide (e.g. database branching) has no viable independent alternative.
- **Branch deployments**: Every PR should get a preview environment with its own database state, enabling continuous deployment to main.
- **Low latency**: The TipTap editor and CRUD operations are the core experience. App server → database round trips must be single-digit milliseconds.
- **Self-hosting**: The same codebase must run on customer infrastructure.

**Decisions already made:**

- SPA architecture with 4 apps: `web` (static), `api` (Hono+tRPC), `collab` (Hocuspocus), `worker` (job consumer). See [SPA project structure](../../plans/2026-02-14-project-structure-spa-design.md).
- Drizzle ORM (supports both PostgreSQL and SQLite/libSQL).
- Storage decision: **PostgreSQL**. See [storage overview](./2026-02-14-storage-overview.md) and [PostgreSQL vs Turso](./2026-02-18-postgres-vs-turso.md).

---

## Database Branching Landscape

Copy-on-write database branching — creating instant, isolated snapshots of a database for PR previews and CI — is the key enabler for continuous deployment to main without a staging environment. This section surveys who offers it.

### Providers with Copy-on-Write Branching

#### Neon (PostgreSQL)

[Neon](https://neon.com) is a serverless PostgreSQL platform that separated storage and compute to offer autoscaling, database branching, and scale-to-zero.

| Aspect              | Details                                                                                                                                                                               |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Headquarters**    | US (acquired by Databricks, 2025)                                                                                                                                                     |
| **EU regions**      | Frankfurt (`aws-eu-central-1`), London (`aws-eu-west-2`) on AWS; Frankfurt (`azure-gwc`) on Azure                                                                                     |
| **Branching**       | Instant copy-on-write. Branch from any point in time. Branches are full PostgreSQL databases with their own connection strings.                                                       |
| **Pricing**         | Free tier: 100 CU-hours/mo, no branches. Launch ($5/mo): 10 branches/project. Scale ($69/mo): 25 branches.                                                                            |
| **Open source**     | Yes — [neon on GitHub](https://github.com/neondatabase/neon). Self-hostable via [Kubernetes operator](https://molnett.com/blog/25-08-05-neon-operator-self-host-serverless-postgres). |
| **Drizzle support** | First-class. Neon is a standard PostgreSQL connection.                                                                                                                                |

**Why it's interesting:** Mature branching with PostgreSQL compatibility. One `neonctl branches create` command gives you a full database snapshot for a PR. Branches are copy-on-write, so they're cheap until you write to them.

**Concerns:** US company (acquired by Databricks, 2025). Runs exclusively on AWS and Azure — no independent EU infrastructure option. Free tier doesn't include branching. Region selection is permanent per project.

#### Turso (libSQL)

[Turso](https://turso.tech) is a hosted platform built on [libSQL](https://github.com/tursodatabase/libsql) (an open-source fork of SQLite). Each database can be branched independently — natural for the database-per-campaign model.

| Aspect              | Details                                                                                                                                                                                                       |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Headquarters**    | Canada/US (originally ChiselStrike, Kitchener ON)                                                                                                                                                             |
| **EU regions**      | Amsterdam, Stockholm, Paris, Frankfurt, Warsaw, London, Bucharest, Madrid. Turso consolidated from Fly.io to AWS (March 2025) — these city endpoints still exist but may map to fewer underlying AWS regions. |
| **Branching**       | Instant copy-on-write per database. `turso db create branch-name --from-db parent-name`.                                                                                                                      |
| **Pricing**         | Free tier: 500 databases, 9 GB total storage, 25M row reads/mo. Scaler ($29/mo): 10,000 databases.                                                                                                            |
| **Open source**     | Yes — libSQL is MIT-licensed. Can run as local SQLite files or self-hosted libSQL server.                                                                                                                     |
| **Drizzle support** | Supported via `@libsql/client`. Works, but PostgreSQL gets more ecosystem attention.                                                                                                                          |

**Why it's interesting:** Database-per-campaign means branching is granular — branch only the campaigns you need for a PR. Free tier includes 500 databases, enough for extensive development. The `:memory:` story for CI tests is unmatched. libSQL is MIT-licensed, so self-hosting avoids the managed platform entirely.

**⚠️ Infrastructure note (Feb 2026):** Turso consolidated from Fly.io to AWS in March 2025. The 8 EU city endpoints still exist in their API, but it's unclear whether each maps to distinct AWS infrastructure or if several route to the same underlying region (e.g. `eu-west-1`). Latency assumptions based on city names should be validated. Branching is a managed-platform feature — self-hosted libSQL does not support CoW branching.

**Concerns:** Canadian/US company. Managed platform now runs on AWS (consolidated from Fly.io, March 2025). Branching N databases per PR requires more orchestration than branching one Neon database. Ecosystem is smaller than PostgreSQL.

#### Simplyblock / Vela (PostgreSQL)

[Simplyblock](https://www.simplyblock.io/) is an NVMe-first Kubernetes storage platform from Germany. Two products are relevant:

- **Simplyblock** (storage layer) — a distributed storage platform deployable via [Helm chart](https://docs.simplyblock.io/) on any Kubernetes cluster, bare metal, or VMs. This is the infrastructure that enables branching at the storage level. See [database branching use case](https://www.simplyblock.io/use-cases/database-branching/).
- **Vela** (database layer) — a [PostgreSQL platform](https://vela.simplyblock.io/) built on top of simplyblock's storage. Adds a Studio UI, Git-style branching workflows, and enterprise features. BYOC (runs in your VPC) or free Sandbox for trial.

| Aspect           | Details                                                                                                                                                                                                                                                 |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Headquarters** | **Germany** (Teltow, near Berlin)                                                                                                                                                                                                                       |
| **Deployment**   | Simplyblock: Helm chart on any K8s (including Hetzner, UpCloud, bare metal). Vela: BYOC on AWS/GCP/Azure/on-prem, or free cloud Sandbox.                                                                                                                |
| **Branching**    | Git-style: named branches, diff between branches, merge, promote, rebase, conflict resolution                                                                                                                                                           |
| **Pricing**      | Vela Sandbox (free): 1 database, 5 clones. Vela Enterprise: $19+/vCPU/mo. Simplyblock storage: see docs.                                                                                                                                                |
| **Open source**  | Yes — both [simplyblock-csi](https://github.com/simplyblock/simplyblock-csi) and [Vela](https://github.com/simplyblock/vela) are Apache-2.0 on GitHub.                                                                                                  |
| **Status**       | Vela repo since September 2025; [announced ~October 2025](https://www.youtube.com/watch?v=mh9psM0gTzI); public beta [press release February 9, 2026](https://www.simplyblock.io/press-releases/simplyblock-vela-beta-launch/). Young but not brand-new. |

**Why it's interesting:** The only EU-headquartered provider with PostgreSQL branching. Git-style semantics (merge, diff, rebase) go beyond what Neon or Turso offer. The Helm-chart-deployable storage layer means this _could_ run on independent EU infrastructure (Hetzner K8s, UpCloud K8s) — no hyperscaler dependency required. BYOC means data stays wherever you deploy it.

**Concerns:** Young project (~5 months old). Unproven at any scale. Running simplyblock + Vela on self-managed K8s is significant operational overhead for a solo dev. Whether the storage layer alone (without Vela) provides usable database branching for PostgreSQL is unclear — needs investigation. Apache-2.0 licensing mitigates the "company folds" risk, but the project is young and the community is small.

### Providers Without Copy-on-Write Branching

These are established EU-headquartered providers that offer managed PostgreSQL but **not** instant database branching.

#### Exoscale (Switzerland)

[Exoscale](https://www.exoscale.com/dbaas/postgresql/) offers managed PostgreSQL powered by Aiven. Data centers in Austria, Germany, Switzerland, and Bulgaria. Supports database **forking** (full copy, not copy-on-write). Forking is useful for creating a one-time copy for testing, but it's not instant and copies all data.

#### Scaleway (France)

[Scaleway](https://www.scaleway.com/en/managed-postgresql-mysql/) offers managed PostgreSQL and MySQL. Data centers in Paris, Amsterdam, and Warsaw. French subsidiary of Iliad group. Protected from the US Cloud Act. No branching or forking feature.

#### UpCloud (Finland)

[UpCloud](https://upcloud.com/postgresql-managed-databases/) offers managed PostgreSQL with PITR (7-day retention). Data centers in Helsinki, Frankfurt, Amsterdam, London, and more. Finnish company. No branching.

#### Hetzner (Germany)

[Hetzner](https://www.hetzner.com/cloud/) does not offer managed databases at all. Pure VPS/compute provider. Third-party managed PostgreSQL is available via [Ubicloud on Hetzner](https://www.ubicloud.com/use-cases/postgres-and-k8s-on-hetzner) (from $12/mo, no branching).

### Summary

| Provider             | HQ              | Type       | Branching                       | EU Regions                       | Maturity               |
| -------------------- | --------------- | ---------- | ------------------------------- | -------------------------------- | ---------------------- |
| **Neon**             | US (Databricks) | PostgreSQL | CoW, instant                    | Frankfurt, London (AWS/Azure)    | Mature                 |
| **Turso**            | Canada/US       | libSQL     | CoW per-database (managed only) | 8 EU cities (on AWS since 2025)  | Mature                 |
| **Simplyblock/Vela** | **Germany**     | PostgreSQL | Git-style                       | Helm (any K8s) / BYOC            | Beta (since ~Sep 2025) |
| **Exoscale**         | Switzerland     | PostgreSQL | Fork only                       | AT, DE, CH, BG                   | Mature                 |
| **Scaleway**         | France          | PostgreSQL | No                              | Paris, Amsterdam, Warsaw         | Mature                 |
| **UpCloud**          | Finland         | PostgreSQL | No                              | Helsinki, Frankfurt, Amsterdam   | Mature                 |
| **Hetzner**          | Germany         | None (VPS) | No                              | Falkenstein, Nuremberg, Helsinki | Mature                 |

**Key finding:** No established EU-headquartered provider offers copy-on-write database branching. The gap is structural — if branch deployments require database branching, you need either a non-EU provider running on hyperscaler infrastructure (Neon on AWS/Azure, Turso on AWS) or the brand-new Vela (BYOC, so you choose the infra). Both Neon and Turso are US/Canadian companies on AWS — neither satisfies EU sovereignty or hyperscaler avoidance preferences.

---

## App Deployment Options

Loreweaver has 4 apps with different deployment lifecycles (see [SPA project structure](../../plans/2026-02-14-project-structure-spa-design.md)). The deployment tool needs to handle:

- **4 independent services** in a monorepo (web, api, collab, worker)
- **PR preview environments** — deploy a preview for every PR, with its own URL and database branch
- **Zero-downtime deploys** to production
- **WebSocket support** for Hocuspocus (collab)
- **Long-running processes** — the worker runs 10+ minute AI jobs

### Coolify

[Coolify](https://coolify.io/) is an open-source, self-hosted PaaS. Install it on a VPS and it provides a web UI for deploying applications, databases, and services.

| Aspect                 | Details                                                                                                                                                                   |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **PR preview deploys** | Built-in. Configure a GitHub/GitLab webhook; PRs automatically deploy to `pr-{id}.example.com`. [Docs](https://coolify.io/docs/applications/ci-cd/github/preview-deploy). |
| **Multiple apps**      | Each app is a separate "resource" in the UI. Configure build context/Dockerfile per app.                                                                                  |
| **Reverse proxy**      | Built-in Traefik with automatic SSL. Route by domain/path.                                                                                                                |
| **Zero-downtime**      | Docker rolling deploys.                                                                                                                                                   |
| **WebSocket support**  | Traefik handles WebSocket proxying natively.                                                                                                                              |
| **Server overhead**    | ~500MB RAM for Coolify itself (API, UI, database).                                                                                                                        |
| **Learning curve**     | Lower — web UI, visual deployment status, log viewer.                                                                                                                     |
| **Runs on**            | Any VPS with Docker. [Hetzner Terraform template](https://github.com/Ujstor/coolify-hetzner-terraform) available.                                                         |

**Strengths:** PR preview deploys are the standout — this is the Vercel-like experience on self-hosted EU infrastructure. Web UI makes operations visible. Good for a solo developer or small team.

**Concerns:** The Coolify platform itself consumes resources on your server. It's another piece of software to maintain (though it self-updates). If Coolify has a bug, it can affect all your deployments.

### Kamal

[Kamal](https://kamal-deploy.org/) (by 37signals/Basecamp) is a CLI tool that deploys Docker containers to any server via SSH. No daemon, no platform — just a deploy command.

| Aspect                 | Details                                                                                                                                                                                                                                                                                                     |
| ---------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **PR preview deploys** | Not built-in. Achievable via ["destinations"](https://dennmart.com/articles/review-apps-with-kamal-part-2-configuring-destinations/) + CI scripting — you create a destination config per PR and deploy to it. [Guide](https://guillaumebriday.fr/deploying-review-apps-automatically-with-kamal-on-ci-cd). |
| **Multiple apps**      | Each app needs its own deploy config. [Multiple apps share one Kamal Proxy](https://www.honeybadger.io/blog/new-in-kamal-2/).                                                                                                                                                                               |
| **Reverse proxy**      | Kamal Proxy — lightweight, auto-SSL, gapless switchover between deploys.                                                                                                                                                                                                                                    |
| **Zero-downtime**      | Built-in via Kamal Proxy's gapless switchover.                                                                                                                                                                                                                                                              |
| **WebSocket support**  | Kamal Proxy handles WebSocket connections.                                                                                                                                                                                                                                                                  |
| **Server overhead**    | Near zero — only your containers + tiny reverse proxy.                                                                                                                                                                                                                                                      |
| **Learning curve**     | Higher — YAML configuration, CLI-driven, no web UI.                                                                                                                                                                                                                                                         |
| **Runs on**            | Any server with SSH + Docker. Language-agnostic despite Ruby origins.                                                                                                                                                                                                                                       |

**Strengths:** Zero platform overhead. Nothing runs on your server except your apps. Battle-tested at 37signals (HEY, Basecamp). Clean, minimal.

**Concerns:** PR preview deploys require significant CI scripting — you're building what Coolify gives you out of the box. No web UI for visibility. Designed around Ruby/Rails conventions, though it works with any Docker image.

### Other Options

**Dokploy** — Another open-source self-hosted PaaS, similar to Coolify. Smaller community. [Some prefer it over Coolify](https://medium.com/@shubhthewriter/coolify-vs-dokploy-why-i-chose-dokploy-for-vps-deployment-in-2026-ea935c2fe9b5) for simpler UX.

**Raw Docker Compose + CI** — Maximum control, most scripting. No built-in PR previews, no web UI, no zero-downtime out of the box. Viable if you want to control every detail and don't mind maintaining deploy scripts.

**Scaleway Serverless Containers** — Managed container platform from a French company. No PR preview feature, but could be scripted via their API. Adds a managed-service dependency.

### App Deploy Comparison

|                      | Coolify                | Kamal                 | Docker Compose + CI             |
| -------------------- | ---------------------- | --------------------- | ------------------------------- |
| **PR preview**       | Built-in               | CI scripting required | CI scripting required           |
| **Web UI**           | Yes                    | No                    | No                              |
| **Server overhead**  | ~500MB                 | Near zero             | Near zero                       |
| **Zero-downtime**    | Yes                    | Yes                   | Manual (blue/green scripting)   |
| **Monorepo support** | Good                   | Good                  | Good                            |
| **WebSocket**        | Yes (Traefik)          | Yes (Kamal Proxy)     | Manual (nginx config)           |
| **Maintenance**      | Coolify updates itself | Nothing to maintain   | Everything is yours to maintain |
| **EU provider**      | N/A (self-hosted)      | N/A (self-hosted)     | N/A (self-hosted)               |

---

## EU Compute Providers

Where to run the app containers. All EU-headquartered.

| Provider     | HQ          | What they offer                          | Compute pricing (approx.) | Managed PG?                      | Notes                                                           |
| ------------ | ----------- | ---------------------------------------- | ------------------------- | -------------------------------- | --------------------------------------------------------------- |
| **Hetzner**  | Germany     | VPS, dedicated servers, cloud            | 2 vCPU / 4GB: ~€5/mo      | No (use Ubicloud or self-manage) | Cheapest EU option. Falkenstein, Nuremberg (DE), Helsinki (FI). |
| **UpCloud**  | Finland     | VPS, managed DBs, object storage         | 2 vCPU / 4GB: ~€13/mo     | Yes (~€15/mo)                    | Good perf/price. Helsinki, Frankfurt, Amsterdam, London, etc.   |
| **Scaleway** | France      | VPS, containers, serverless, managed DBs | 2 vCPU / 4GB: ~€10/mo     | Yes (~€11/mo)                    | Broadest EU service portfolio. Paris, Amsterdam, Warsaw.        |
| **Exoscale** | Switzerland | VPS, managed DBs (Aiven), K8s            | 2 vCPU / 4GB: ~€16/mo     | Yes (via Aiven)                  | Swiss data sovereignty. AT, DE, CH, BG.                         |

For a development-phase deployment (single VPS running Coolify or Kamal with all 4 apps), Hetzner at ~€5/mo is the most cost-effective option. Scaling up means either a larger Hetzner VPS or splitting services across multiple VPS.

---

## Latency Considerations

The TipTap editor makes many small round trips to the API (save operations, mention resolution, search). The collab server (Hocuspocus) maintains persistent WebSocket connections. Both need low-latency database access.

### Estimated App Server → Database Latencies

These are network round-trip estimates. Actual query latency adds database processing time. **Turso rows assume the city endpoints map to nearby infrastructure — this needs validation since Turso's move from Fly.io to AWS (see note in Turso section above).**

| App server location      | Database location    | Estimated RTT |
| ------------------------ | -------------------- | ------------- |
| Hetzner Falkenstein (DE) | Neon Frankfurt (DE)  | ~3-5ms        |
| Hetzner Nuremberg (DE)   | Neon Frankfurt (DE)  | ~3-5ms        |
| Hetzner Falkenstein (DE) | Turso Frankfurt (DE) | ~3-5ms        |
| Hetzner Falkenstein (DE) | Turso Amsterdam (NL) | ~8-12ms       |
| Hetzner Helsinki (FI)    | Turso Stockholm (SE) | ~5-8ms        |
| UpCloud Frankfurt (DE)   | Neon Frankfurt (DE)  | ~1-3ms        |
| Scaleway Paris (FR)      | Neon Frankfurt (DE)  | ~10-15ms      |

**Key observations:**

- **Co-locating app server and database in the same region** is what matters — not the end user's location. The Vite SPA is static and served from the user's nearest edge; only API calls hit the server.
- **Hetzner Germany → Neon Frankfurt** is the lowest-latency combination for PostgreSQL. Both are in the Frankfurt metro area.
- **Hetzner Helsinki → Turso Stockholm** is a strong Nordic combination for libSQL, with ~5-8ms RTT and both providers in EU/EEA jurisdictions.

---

## How CD-to-main Could Work

The goal: every PR gets an isolated preview environment with its own database state. When merged to main, production is updated automatically. No staging environment.

### With Neon (PostgreSQL path)

```
                              ┌─────────────┐
                              │ GitHub repo  │
                              └──────┬───────┘
                                     │ PR opened
                                     ▼
                         ┌───────────────────────┐
                         │  GitHub Actions CI     │
                         │                       │
                         │  1. turbo build/test   │
                         │  2. neonctl branches   │
                         │     create pr-${N}     │
                         │  3. Run migrations     │
                         │  4. Trigger Coolify    │
                         │     preview deploy     │
                         └───────────┬───────────┘
                                     │
                    ┌────────────────┼────────────────┐
                    ▼                                 ▼
          ┌─────────────────┐              ┌──────────────────┐
          │ Coolify preview  │              │ Neon branch      │
          │ pr-123.dev.app  │──────────────│ pr-123           │
          │ (Hetzner DE)    │  DATABASE_URL│ (Frankfurt)      │
          └─────────────────┘              └──────────────────┘

    PR merged to main:
      - CI runs build/test
      - Migrations on production Neon
      - Coolify deploys to production
      - neonctl branches delete pr-${N}
```

**One branch command, one connection string.** Neon branching is a single operation — the branch is a complete PostgreSQL database with all tables, data, and extensions. The preview app just needs a different `DATABASE_URL`.

### With Turso (libSQL path)

```
                              ┌─────────────┐
                              │ GitHub repo  │
                              └──────┬───────┘
                                     │ PR opened
                                     ▼
                         ┌───────────────────────┐
                         │  GitHub Actions CI     │
                         │                       │
                         │  1. turbo build/test   │
                         │  2. turso db create    │
                         │     platform-pr-${N}   │
                         │     --from-db platform  │
                         │  3. turso db create    │
                         │     camp-test-pr-${N}   │
                         │     --from-db camp-test │
                         │  4. Update platform DB  │
                         │     to point camp refs  │
                         │     to branched DBs     │
                         │  5. Run migrations      │
                         │  6. Trigger Coolify     │
                         │     preview deploy      │
                         └───────────┬────────────┘
                                     │
                    ┌────────────────┼────────────────┐
                    ▼                                 ▼
          ┌─────────────────┐    ┌──────────────────────────┐
          │ Coolify preview  │    │ Turso branches                │
          │ pr-123.dev.app  │    │ platform-pr-123 (EU region)   │
          │ (Hetzner DE)    │────│ camp-test-pr-123 (EU region)  │
          └─────────────────┘    └──────────────────────────┘

    PR merged to main:
      - CI runs build/test
      - Migrations on production DBs
      - Coolify deploys to production
      - turso db destroy platform-pr-${N}
      - turso db destroy camp-test-pr-${N}
```

**Multiple branch commands, pointer rewriting.** Each campaign database is branched separately. The platform database branch needs its `db_name` references updated to point to the branched campaign databases (see [postgres_vs_turso.md](./2026-02-18-postgres-vs-turso.md#database-branching-for-development) for the rewrite pattern). More orchestration, but granular — you branch only the campaigns needed for the PR.

### Without Database Branching (EU-only PostgreSQL path)

If using an EU-only provider without branching (UpCloud, Scaleway, Exoscale):

```
    PR opened → CI creates fresh DB → runs migrations → seeds with fixtures
    PR review → tests against fixture data (not production-like)
    PR merged → CI runs migrations on production → deploys
```

This works but loses the ability to test against realistic data. Loreweaver's test data is expensive to create (audio transcription + AI entity extraction costs $2+ per session), so fixture-based testing covers fewer real-world scenarios.

---

## Self-Hosted Deployment Model

A key Loreweaver requirement is that the same codebase runs on customer infrastructure. How does each combination work for a self-hoster?

### PostgreSQL path (self-hosted)

```
Self-hosted (Docker Compose or similar)
├── docker-compose.yml
│   ├── web       → nginx serving Vite build (port 80/443)
│   ├── api       → Hono container (port 3001)
│   ├── collab    → Hocuspocus container (port 3002)
│   ├── worker    → job consumer container
│   └── postgres  → PostgreSQL container (or connect to managed PG)
```

The self-hoster runs PostgreSQL (Docker container, managed service, or existing instance). Same schema, same migrations, same Drizzle queries. No Neon branching — that's a development/CI feature, not a production requirement.

**Complexity:** Medium. PostgreSQL is universal knowledge, but the self-hoster must manage a database server (backups, updates, connection pooling).

### Turso/libSQL path (self-hosted)

```
Self-hosted (Docker Compose or similar)
├── docker-compose.yml
│   ├── web       → nginx serving Vite build (port 80/443)
│   ├── api       → Hono container (port 3001)
│   ├── collab    → Hocuspocus container (port 3002)
│   └── worker    → job consumer container
├── data/
│   ├── platform.db          → SQLite file (users, campaigns, jobs)
│   └── campaigns/
│       ├── campaign-abc.db  → SQLite file (campaign graph data)
│       └── campaign-def.db  → SQLite file
```

No database server process needed. The application reads/writes SQLite files directly (or via a libSQL server if multi-process access is needed). Backup = copy files.

**Complexity:** Lower. No database server to install, configure, or maintain. The tradeoff is that the self-hoster needs to understand the two-tier data model (platform DB + campaign DBs).

---

## Cost Sketches (Development Phase)

These assume a solo developer or tiny team, not production scale.

### Combination 1: Neon + Hetzner + Coolify

| Item                           | Monthly     |
| ------------------------------ | ----------- |
| Hetzner CX22 (2 vCPU, 4GB)     | ~€5         |
| Neon Launch plan (10 branches) | ~$5 (€4.60) |
| Domain + DNS                   | ~€1         |
| **Total**                      | **~€11/mo** |

### Combination 2: Turso + Hetzner + Coolify

| Item                           | Monthly    |
| ------------------------------ | ---------- |
| Hetzner CX22 (2 vCPU, 4GB)     | ~€5        |
| Turso free tier (500 DBs, 9GB) | $0         |
| Domain + DNS                   | ~€1        |
| **Total**                      | **~€6/mo** |

### Combination 3: UpCloud managed PG + UpCloud VPS + Coolify

| Item                               | Monthly     |
| ---------------------------------- | ----------- |
| UpCloud VPS (2 vCPU, 4GB)          | ~€13        |
| UpCloud managed PostgreSQL (small) | ~€15        |
| Domain + DNS                       | ~€1         |
| **Total**                          | **~€29/mo** |

No database branching. PR previews use fixture data or a shared dev database.

### Combination 4: Scaleway managed PG + Scaleway VPS + Coolify

| Item                                | Monthly     |
| ----------------------------------- | ----------- |
| Scaleway DEV1-M (3 vCPU, 4GB)       | ~€10        |
| Scaleway managed PostgreSQL (small) | ~€11        |
| Domain + DNS                        | ~€1         |
| **Total**                           | **~€22/mo** |

No database branching. French company, French/Dutch data centers.

---

## Open Questions

1. **Coolify's monorepo story in practice.** How does Coolify handle 4 apps from one repo? Is each app a separate "resource" pointing to the same repo with different build contexts and Dockerfiles? Does the PR preview deploy all 4 apps, or just the changed ones?

2. **Hocuspocus storage adapter.** Hocuspocus needs to persist Y.Doc state. It has a PostgreSQL adapter — does it have a SQLite/libSQL adapter? If not, and we go with Turso, does the collab data live in the platform database or in the campaign database?

3. **Turso branching automation.** The multi-database branching workflow (platform + N campaign DBs) needs a script. How complex is this in practice? Is there a Turso CI integration or GitHub Action?

4. **Neon branch limits.** The Launch plan includes 10 branches. With a team of 3, each with 2-3 open PRs, you could hit this quickly. Does the Scale plan (25 branches, $69/mo) become necessary? Or are branches created and deleted fast enough that the limit rarely applies?

5. **Latency validation.** The estimates above are based on geographic distance. Actual benchmarks (Hetzner → Neon, Hetzner → Turso) should be measured with a simple ping/query test before committing to a combination.

6. **Simplyblock/Vela maturity.** Already ~5 months old (repo since Sep 2025, Apache-2.0). Worth evaluating now rather than waiting. Track: stability reports, customer case studies, whether the Helm-chart path works on Hetzner/UpCloud K8s without Vela Enterprise.

7. **Coolify vs Kamal preference.** Both work. Coolify has built-in PR previews; Kamal is lighter. Worth trying both during initial setup to develop a personal preference.

8. **GitHub Actions runner location.** If CI creates database branches and runs migrations, the CI runner's location affects latency to the database during setup. GitHub's standard runners are in the US. EU-based runners (larger runners in specific regions, or self-hosted runners on Hetzner) would speed up the branch-creation step.

---

## Sources

### Database Providers

- [Neon Regions](https://neon.com/docs/introduction/regions) — EU regions: Frankfurt (AWS/Azure), London (AWS)
- [Neon Pricing](https://neon.com/pricing) — Launch plan $5/mo, 10 branches
- [Neon GitHub (open source)](https://github.com/neondatabase/neon)
- [Neon self-hosted Kubernetes operator](https://molnett.com/blog/25-08-05-neon-operator-self-host-serverless-postgres)
- [Turso](https://turso.tech/) — libSQL, database-per-tenant, broad EU region coverage
- [Turso Locations API](https://docs.turso.tech/api-reference/locations/list)
- [Simplyblock](https://www.simplyblock.io/) — NVMe-first K8s storage platform (German HQ)
- [Simplyblock Docs](https://docs.simplyblock.io/) — Helm chart deployment, architecture
- [Simplyblock Database Branching](https://www.simplyblock.io/use-cases/database-branching/) — branching use case
- [Vela](https://vela.simplyblock.io/) — PostgreSQL platform built on simplyblock (beta)
- [Vela launch announcement](https://blocksandfiles.com/2026/02/10/simplyblock-provides-postgres-git-style-branching/)
- [Exoscale Managed PostgreSQL](https://www.exoscale.com/dbaas/postgresql/) — Switzerland, powered by Aiven
- [Scaleway Managed Databases](https://www.scaleway.com/en/managed-postgresql-mysql/) — France
- [UpCloud Managed PostgreSQL](https://upcloud.com/postgresql-managed-databases/) — Finland

### App Deployment

- [Coolify](https://coolify.io/) — self-hosted PaaS
- [Coolify GitHub Preview Deploy docs](https://coolify.io/docs/applications/ci-cd/github/preview-deploy)
- [Coolify on Hetzner (Terraform)](https://github.com/Ujstor/coolify-hetzner-terraform)
- [Kamal 2](https://kamal-deploy.org/) — CLI Docker deploys via SSH
- [Kamal 2: deploying multiple apps](https://www.honeybadger.io/blog/new-in-kamal-2/)
- [Kamal review apps with destinations](https://dennmart.com/articles/review-apps-with-kamal-part-2-configuring-destinations/)
- [Kamal review apps: CI/CD automation](https://guillaumebriday.fr/deploying-review-apps-automatically-with-kamal-on-ci-cd)
- [Self-hosted deployment tools compared](https://haloy.dev/blog/self-hosted-deployment-tools-compared)

### Infrastructure

- [Hetzner Cloud](https://www.hetzner.com/cloud/) — German VPS provider
- [Ubicloud PostgreSQL on Hetzner](https://www.ubicloud.com/use-cases/postgres-and-k8s-on-hetzner)
- [European Alternatives](https://european-alternatives.eu/) — directory of EU software/services
