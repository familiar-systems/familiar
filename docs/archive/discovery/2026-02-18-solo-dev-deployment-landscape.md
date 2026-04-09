> **Superseded.** Deployment decided: Coolify on Hetzner with libSQL database-per-campaign. See [deployment strategy](../plans/2026-03-09-deployment-strategy.md). This document's research informed that decision.

# familiar.systems — Solo-Dev Deployment Landscape

## Context

The [EU deployment landscape](./eu_deployment_landscape.md) mapped deployment options with team-scale assumptions: PR preview environments with database branching, automated CD pipelines, and isolated preview databases per pull request. That document is useful as a reference for where the project _could_ go, but it doesn't reflect the current reality.

**The current reality:** One developer. No team contention. No need for PR-based isolation. The expensive-test-data problem is real (audio transcription + AI entity extraction costs $2+ per session), but the _solution_ doesn't require copy-on-write branching when there's only one person writing code.

This document explores what deployment looks like when you strip away the team-scale assumptions and ask: what's the simplest, most pragmatic setup for a solo developer?

**Driving requirements (revised for solo dev):**

- **Local-first development**: Everything runs on the developer's machine during active development
- **One remote environment**: A single VPS that starts as a sandbox and grows into production. No staging.
- **EU/EEA data sovereignty**: EU-headquartered providers preferred; non-EU providers acceptable where no EU alternative exists for a needed capability
- **The "branching comfort"**: The developer values the ability to experiment freely and roll back to known-good database state — but doesn't necessarily need copy-on-write branching to get it
- **Self-hosting**: The same codebase must run on customer infrastructure (unchanged from before)
- **Low operational overhead**: Solo dev time spent on infrastructure is time not spent on the product

**Decisions already made:**

- SPA architecture with 4 apps: `web` (static), `api` (Hono+tRPC), `collab` (Hocuspocus), `worker` (job consumer). See [SPA project structure](../plans/2026-02-14-project-structure-spa-design.md).
- Drizzle ORM (supports both PostgreSQL and SQLite/libSQL).
- Storage decision: **PostgreSQL**. See [storage overview](./2026-02-14-storage-overview.md) and [PostgreSQL vs Turso](./2026-02-18-postgres-vs-turso.md).

---

## What Deployment Tools Actually Do

Before comparing options, it helps to understand the problem these tools solve. You have code on your machine. You want it running on a server at `https://familiar.systems`. Between those two states:

1. **Build** your code into something runnable (Docker images, static files)
2. **Transfer** that artifact to the server
3. **Run** the new version
4. **Route traffic** to it (reverse proxy: domain → container, SSL termination)
5. **Don't drop requests** during the switchover (zero-downtime deploy)
6. **Manage environment variables** (database URLs, API keys)
7. **Restart if it crashes** (process supervision)
8. **See what's happening** (logs, health checks)

Every tool in this document solves some subset of these steps. The spectrum runs from "do it all yourself" to "pay someone to handle everything":

```
More control, more work                              Less control, less work
◄────────────────────────────────────────────────────────────────────────────►

SSH + Docker     Kamal     Coolify/Dokploy     Railway/Render     Vercel
Compose                    (self-hosted PaaS)  Fly.io             (fully managed)

€5/mo VPS        €5/mo     €5/mo VPS           €25-50/mo          €40+/mo
                 VPS       + 500MB overhead
```

You can move right on this spectrum at any time (add tooling, switch to managed). Moving left is harder — you have to learn what the managed platform was hiding from you.

---

## The Deployment Spectrum

### Tier 1: Self-Managed VPS (~€5-8/mo)

A single VPS (Hetzner, UpCloud, etc.) running Docker. You manage everything: the OS, Docker, PostgreSQL, SSL certificates, backups.

#### Deploy Methods on a VPS

| Method                             | What it adds over SSH                                                                                             | Overhead           | PR previews                     |
| ---------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ------------------ | ------------------------------- |
| `git pull && docker compose up -d` | Nothing. Manual SSH.                                                                                              | Zero               | No                              |
| **Kamal** (37signals)              | Zero-downtime deploys via kamal-proxy, one-command deploy from laptop, rollback, auto-SSL. CLI-only, YAML config. | ~10MB (tiny proxy) | Scriptable via CI, not built-in |
| **Coolify** (open source)          | Web UI for deployments, auto-SSL (Traefik), log viewer, env var management, scheduled DB backups, self-updates.   | ~500MB RAM         | Built-in                        |
| **Dokploy** (open source)          | Similar to Coolify, simpler UI, some prefer it. Built-in monitoring/alerting.                                     | ~400MB RAM         | Built-in                        |

**Kamal** is a deployment _script_ with best practices baked in. Nothing runs on your server except your apps and a tiny reverse proxy. It's what you'd do manually over SSH, automated and with zero-downtime as the default. 37signals uses it to deploy Basecamp and HEY.

**Coolify** is "Heroku on your own VPS." It gives you a web dashboard, git-push deploys, PR preview environments, and database management — but it consumes ~500MB RAM on your server and is another piece of software to maintain.

**Dokploy** is similar to Coolify with a slightly different UX. Smaller community (~24k GitHub stars vs Coolify's ~45k) but growing.

#### Monorepo Support

All three handle familiar.systems's 4-app monorepo well:

- **Kamal**: Each app gets its own deploy config. Multiple apps share one kamal-proxy on the same server. Path-based routing supported (`/api/*` → API container, `/collab/*` → WebSocket container).
- **Coolify/Dokploy**: Each app is a separate "resource" pointing to the same repo with different build contexts/Dockerfiles.

#### WebSocket & Long-Running Jobs

- **Kamal**: kamal-proxy handles WebSocket proxying natively.
- **Coolify**: Traefik handles WebSocket proxying natively.
- **Long-running workers**: No constraints. Your container, your rules. The worker can run 10+ minute AI jobs without any platform timeout.

**When to choose Tier 1:** You're comfortable with SSH and Docker. You want maximum control and minimum cost. You don't mind being your own sysadmin.

---

### Tier 1.5: VPS + Managed PostgreSQL (~€18-28/mo)

Same as Tier 1, but you offload database management to the VPS provider. You run your 4 apps on a VPS, but PostgreSQL is managed by the provider with backups, PITR (point-in-time recovery), connection pooling, and sometimes HA (high availability).

This is where the EU-native "mini-cloud" providers shine:

| Provider     | HQ          | VPS (2 vCPU, 4GB) | Managed PostgreSQL  | Total      | EU Regions                                        |
| ------------ | ----------- | ----------------- | ------------------- | ---------- | ------------------------------------------------- |
| **UpCloud**  | Finland     | ~€13/mo           | ~€15/mo             | ~€28/mo    | Helsinki, Frankfurt, Amsterdam, London            |
| **Scaleway** | France      | ~€10/mo           | ~€11/mo             | ~€21/mo    | Paris, Amsterdam, Warsaw                          |
| **OVH**      | France      | ~€7-12/mo         | ~€14/mo             | ~€21-26/mo | Strasbourg, Gravelines, London, Frankfurt, Warsaw |
| **Exoscale** | Switzerland | ~€16/mo           | ~€16/mo (via Aiven) | ~€32/mo    | Vienna, Frankfurt, Zurich, Sofia                  |

None of these offer database branching. You get traditional managed PostgreSQL: backups, PITR, updates handled for you.

**Ubicloud** (US company, YC W24) is an interesting option here. It's an open-source cloud layer that runs on Hetzner bare metal, adding managed PostgreSQL with HA, PITR, and connection pooling. Founded by the team that built Citus Data (distributed PostgreSQL, acquired by Microsoft) and Heroku PostgreSQL. Managed Postgres starts at ~$12.40/mo; burstable VM at ~$6.65/mo. Available in Germany (Hetzner datacenters). The tradeoff: it's a young company (~15 employees, seed-funded) and US-headquartered despite running on EU infrastructure.

**When to choose Tier 1.5:** You want someone else to handle database backups, updates, and failover. The ~€15/mo premium over self-managed PostgreSQL buys you peace of mind and time not spent on database ops.

---

### Tier 2: Managed Platforms (~€25-50/mo)

Push code, it runs. No server to manage. These platforms handle building, deploying, routing, SSL, logging, and scaling.

#### Railway (~$25-45/mo)

| Aspect                 | Details                                                                                                                                                                              |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **HQ**                 | US                                                                                                                                                                                   |
| **EU region**          | Amsterdam (EU West)                                                                                                                                                                  |
| **Monorepo**           | First-class. Auto-detects pnpm/npm workspaces, stages a service per package.                                                                                                         |
| **PR previews**        | Built-in. Isolated environments per PR with all services, fresh databases, unique URLs, automatic cleanup. "Focused PR Environments" only deploy services affected by changed files. |
| **WebSocket**          | Native support (HTTP, TCP, gRPC, WebSocket handled automatically).                                                                                                                   |
| **Long-running jobs**  | No timeout limits on Pro plan. Workers run continuously.                                                                                                                             |
| **Database**           | Managed PostgreSQL included. Very cheap for light usage (~$1-3/mo).                                                                                                                  |
| **Database branching** | No. PR environments get fresh (empty) databases, not snapshots.                                                                                                                      |
| **Pricing**            | Pro plan: $20/mo + usage. 4 services + PostgreSQL likely $25-45/mo depending on utilization. Idle services consume near-zero.                                                        |
| **Lock-in**            | Low-medium. Standard Docker containers underneath. Main lock-in is convenience.                                                                                                      |

**Strengths:** Best monorepo DX. PR preview environments are genuinely useful even for a solo dev (test a branch on your phone without running it locally). Closest to "Vercel for backends" at indie-hacker pricing.

**Concerns:** US company. Amsterdam is the only EU region. Fresh databases in PR environments aren't useful for testing against realistic data.

#### Render (~$28-41/mo)

| Aspect                 | Details                                                                                                                                  |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| **HQ**                 | US                                                                                                                                       |
| **EU region**          | Frankfurt (on AWS eu-central-1)                                                                                                          |
| **Monorepo**           | Supported — configure root directory per service. Less automated than Railway.                                                           |
| **PR previews**        | Yes, via "Preview Environments."                                                                                                         |
| **WebSocket**          | Supported on web services.                                                                                                               |
| **Long-running jobs**  | Background Worker is a first-class service type. Runs continuously on paid plans.                                                        |
| **Database**           | Managed PostgreSQL from ~$7/mo. PITR for 7 days on paid plans.                                                                           |
| **Database branching** | No.                                                                                                                                      |
| **Pricing**            | 3 services at Starter ($7/mo each) + free static site + PostgreSQL (~$7-20/mo) = ~$28-41/mo. Fixed-price tiers — you pay even when idle. |
| **Lock-in**            | Low. Standard Docker containers or buildpack-based deploys.                                                                              |

**Strengths:** Frankfurt region (closer to Hetzner if you ever co-locate). Background workers are a first-class concept. Fixed pricing is predictable.

**Concerns:** Fixed-price tiers mean you pay per service even when idle. Starter tier (512MB, 0.5 vCPU) may be tight for the worker during AI processing — you might need Standard ($25/mo) for that service.

#### Fly.io (~$15-70/mo)

| Aspect                 | Details                                                                                                           |
| ---------------------- | ----------------------------------------------------------------------------------------------------------------- |
| **HQ**                 | US                                                                                                                |
| **EU regions**         | Amsterdam, Frankfurt, London, Paris, Stockholm, Warsaw, Madrid (excellent coverage)                               |
| **Monorepo**           | Manual — each service is a separate Fly app with its own `fly.toml`.                                              |
| **PR previews**        | Not built-in (scriptable via CLI).                                                                                |
| **WebSocket**          | Excellent — Fly was designed for edge-deployed, latency-sensitive apps.                                           |
| **Long-running jobs**  | Machines can run indefinitely. Machines API lets you start/stop workers programmatically (pay only when running). |
| **Database**           | Self-managed Fly Postgres (~$4/mo) or Managed Postgres (starts at $38/mo — expensive).                            |
| **Database branching** | No.                                                                                                               |
| **Pricing**            | 4 small machines (~$8-28/mo) + Postgres ($4-38/mo). Wide range depending on choices.                              |
| **Lock-in**            | Low-medium. Standard Docker containers. `fly.toml` is Fly-specific but trivial to replace.                        |

**Strengths:** Best EU region coverage. Best WebSocket story. Pay-per-use worker machines could save money (spin up for AI jobs, stop when done). Self-managed Postgres at $4/mo is very cheap.

**Concerns:** No built-in PR previews. No git-push deploys (CLI-based). Has had reliability concerns historically. Pricing recently became more complex (Feb 2026 changes to volume snapshots and inter-region networking). Managed Postgres is expensive.

---

### Tier 3: AWS (~$42-110/mo)

#### Option A: EC2 + Docker Compose (simplest AWS path)

One EC2 instance running all 4 services + RDS PostgreSQL.

| Item                             | Monthly     |
| -------------------------------- | ----------- |
| EC2 t4g.medium (2 vCPU, 4GB ARM) | ~$25        |
| RDS db.t4g.micro (2 vCPU, 1GB)   | ~$22        |
| Storage (20GB gp3)               | ~$2         |
| **Total**                        | **~$49/mo** |

This is functionally identical to Tier 1 (Docker Compose on a VPS) but on AWS. You get AWS's EU regions (Frankfurt, Ireland, Stockholm, Paris, Milan, Spain) and can co-locate with Neon if you want database branching later.

#### Option B: ECS Fargate + RDS (managed containers)

| Item                                    | Monthly         |
| --------------------------------------- | --------------- |
| 4 Fargate tasks (0.25 vCPU, 512MB each) | ~$36            |
| RDS db.t4g.micro                        | ~$22            |
| ALB (Application Load Balancer)         | ~$16            |
| NAT Gateway (if private subnets)        | ~$32            |
| **Total**                               | **~$75-110/mo** |

The ALB and NAT Gateway are the cost killers. You can avoid the NAT Gateway with public subnets (less secure) and avoid the ALB with a single Fargate task + nginx sidecar (defeats the purpose of independent deployment).

**SST v3** (Serverless Stack) can deploy this setup from a single `sst.config.ts` file with first-class Hono support. It handles VPC, subnets, security groups, and NAT gateways automatically. SST itself is free — you pay AWS prices.

**When to choose AWS:** You already know AWS. Or you want to co-locate with Neon (which runs on AWS) for sub-millisecond database latency. Otherwise, the pricing and complexity penalty is hard to justify for a solo dev.

---

### IaC Tools (Not a Tier — a Cross-Cutting Concern)

**SST v3**: TypeScript IaC focused on AWS. Deploys Hono to Lambda or ECS Fargate. ~10 lines to define an ECS service. Free tool, AWS pricing applies.

**Pulumi**: General-purpose TypeScript IaC. Works with AWS, GCP, Azure, Hetzner, DigitalOcean, and many others. More verbose than SST (~50-80 lines for the same ECS service) but more flexible. Has a Hetzner provider. Free for individual use.

Neither provides a deployment platform. They deploy _to_ a platform. Relevant if you want infrastructure-as-code from day one, but arguably premature for a single VPS.

---

## EU-Native Provider Landscape

These providers are EU-headquartered. They sit underneath the tiers above — the tier is _how_ you deploy; the provider is _where_ the server lives.

### Compute-Only (VPS / Bare Metal)

| Provider    | HQ      | Managed DB? | Compute (2 vCPU, 4GB) | Regions                                    | Notes                                         |
| ----------- | ------- | ----------- | --------------------- | ------------------------------------------ | --------------------------------------------- |
| **Hetzner** | Germany | No          | ~€5/mo                | Falkenstein, Nuremberg (DE), Helsinki (FI) | Cheapest. Compute only — no managed services. |

### Mini-Clouds (VPS + Managed Services)

| Provider     | HQ          | Managed PostgreSQL       | Compute (2 vCPU, 4GB) | Regions                                           | Notes                                                                                |
| ------------ | ----------- | ------------------------ | --------------------- | ------------------------------------------------- | ------------------------------------------------------------------------------------ |
| **UpCloud**  | Finland     | Yes (~€15/mo, PITR)      | ~€13/mo               | Helsinki, Frankfurt, Amsterdam, London, etc.      | Good perf/price. Finnish data sovereignty.                                           |
| **Scaleway** | France      | Yes (~€11/mo)            | ~€10/mo               | Paris, Amsterdam, Warsaw                          | Broadest EU service portfolio (serverless, K8s, object storage). French/Iliad group. |
| **OVH**      | France      | Yes (~€14/mo)            | ~€7-12/mo             | Strasbourg, Gravelines, London, Frankfurt, Warsaw | Largest EU cloud. Rougher DX than Scaleway.                                          |
| **Exoscale** | Switzerland | Yes (~€16/mo, via Aiven) | ~€16/mo               | Vienna, Frankfurt, Zurich, Sofia                  | Swiss data sovereignty. Premium pricing.                                             |
| **Elastx**   | Sweden      | Yes (managed K8s + PG)   | Varies                | Stockholm                                         | Swedish data sovereignty. More enterprise-focused.                                   |

### Key Distinction

**Hetzner** is compute-only — you manage everything yourself. **UpCloud, Scaleway, OVH, and Exoscale** are mini-clouds — they offer managed PostgreSQL, object storage, sometimes K8s and serverless. This matters because with Hetzner you run PostgreSQL in Docker and manage backups yourself; with UpCloud or Scaleway you can offload database operations while still running your apps on a cheap VPS.

---

## Database Branching Without a Branching Provider

No deployment platform or EU-native provider offers copy-on-write database branching. That's exclusively a Neon (PostgreSQL) or Turso (libSQL) feature — both US/Canadian companies running on AWS.

For a solo developer, the "branching comfort" — the ability to experiment freely and roll back to known-good state — can be achieved with simpler tools:

### PostgreSQL

```bash
# "Create a branch" = snapshot before risky work
pg_dump mydb > snapshots/before-migration-2026-02-15.sql

# Experiment freely...

# "Roll back" = restore the snapshot
dropdb mydb && createdb mydb && psql mydb < snapshots/before-migration-2026-02-15.sql
```

For a dev database with realistic data (50-200MB), this takes **2-5 seconds**. Not instant like CoW branching, but fast enough for a solo workflow where you branch a few times per week, not per PR.

### SQLite/libSQL (Turso path)

```bash
# "Create a branch" = copy a file
cp campaign-abc.db campaign-abc.db.snapshot

# "Roll back" = copy it back
cp campaign-abc.db.snapshot campaign-abc.db
```

**Instant.** The simplest possible "branching" — free, local, zero infrastructure.

### When you'd upgrade to real branching

If `pg_dump`/`cp` starts feeling painful — likely when the database grows past ~1GB or when you're branching multiple times per day — that's the signal to add Neon or Turso. Both can be layered onto any compute tier without changing how you deploy your apps.

---

## Cost Comparison (Solo Dev, Development Phase)

| Setup                             | Monthly  | What you manage                           | DB branching                     |
| --------------------------------- | -------- | ----------------------------------------- | -------------------------------- |
| **Hetzner CX22 + Docker Compose** | ~€5      | Everything (OS, Docker, PG, backups, SSL) | pg_dump/cp                       |
| **Hetzner CX22 + Coolify**        | ~€5      | Server + Coolify                          | pg_dump/cp                       |
| **Hetzner CX22 + Neon free**      | ~€5 + $0 | Server + Coolify/Kamal. Neon manages DB.  | Neon CoW branching               |
| **UpCloud VPS + UpCloud PG**      | ~€28     | Apps only. DB managed by UpCloud.         | pg_dump                          |
| **Scaleway VPS + Scaleway PG**    | ~€21     | Apps only. DB managed by Scaleway.        | pg_dump                          |
| **Railway (all-in)**              | ~$25-45  | Nothing. Push code, it runs.              | Fresh DBs per PR (not snapshots) |
| **Render (all-in)**               | ~$28-41  | Nothing. Push code, it runs.              | No                               |
| **Fly.io (self-managed PG)**      | ~$15-30  | Postgres (backups, updates).              | No                               |
| **AWS EC2 + RDS**                 | ~$49     | EC2 instance, Docker, deploys.            | RDS snapshots                    |
| **AWS Fargate + RDS**             | ~$75-110 | IAM, VPC, task definitions.               | RDS snapshots                    |

---

## Open Questions

1. **Coolify vs Kamal for a solo dev.** Both work well. Coolify's web UI and built-in PR previews are nice; Kamal's zero overhead and CLI-driven approach appeal to infrastructure-as-code mindsets. Worth trying both during initial setup to develop a personal preference.

2. **UpCloud vs Scaleway for Tier 1.5.** If managed PostgreSQL is desired without leaving the EU, both are strong options. UpCloud has better Nordic presence (Helsinki); Scaleway has a broader service catalog. Worth comparing their managed PostgreSQL features (HA, PITR retention, connection pooling, extensions).

3. **When to add Neon.** The Hetzner + Neon free tier combination is interesting — you get real CoW branching for $0 extra, but you're adding a US/AWS dependency for the database. The question is whether the branching comfort is worth the dependency this early.

4. **Railway as the "skip the VPS" option.** At ~$25/mo, Railway is the strongest managed-platform option for a solo dev. The monorepo DX is excellent and PR preview environments work out of the box. The question is whether paying 5x more than a Hetzner VPS is worth eliminating infrastructure management entirely.

5. **Ubicloud maturity tracking.** Founded by the Citus Data / Heroku PostgreSQL team. Managed PostgreSQL with HA and PITR from ~$12.40/mo on Hetzner infrastructure. Worth evaluating when the project reaches production readiness, but the company is young (~15 employees, seed-funded, K8s still in preview).

---

## Sources

### Deployment Tools

- [Kamal](https://kamal-deploy.org/) — CLI Docker deploys via SSH (37signals)
- [Kamal 2: multiple apps on single server](https://www.honeybadger.io/blog/new-in-kamal-2/)
- [Kamal review apps with destinations](https://dennmart.com/articles/review-apps-with-kamal-part-2-configuring-destinations/)
- [Coolify](https://coolify.io/) — self-hosted PaaS
- [Coolify GitHub preview deploy docs](https://coolify.io/docs/applications/ci-cd/github/preview-deploy)
- [Dokploy vs Coolify comparison](https://blog.logrocket.com/dokploy-vs-coolify-production/)

### Managed Platforms

- [Railway pricing](https://railway.com/pricing)
- [Railway monorepo guide](https://docs.railway.com/guides/monorepo)
- [Railway deployment regions](https://docs.railway.com/reference/deployment-regions)
- [Railway PR environments](https://blog.railway.com/p/cicd-for-modern-deployment-from-manual-deploys-to-pr-environments)
- [Render pricing](https://render.com/pricing)
- [Render regions](https://render.com/docs/regions)
- [Fly.io pricing](https://fly.io/docs/about/pricing/)
- [Fly.io managed Postgres](https://fly.io/docs/mpg/)
- [Fly.io regions](https://fly.io/docs/reference/regions/)

### IaC Tools

- [SST v3 — Hono on AWS](https://sst.dev/docs/start/aws/hono/)
- [Pulumi Hetzner provider](https://www.pulumi.com/registry/packages/hcloud/)
- [Terraform vs Pulumi vs SST analysis](https://www.gautierblandin.com/articles/terraform-pulumi-sst-tradeoff-analysis)

### EU Providers

- [Hetzner Cloud](https://www.hetzner.com/cloud/) — German compute provider
- [UpCloud managed PostgreSQL](https://upcloud.com/postgresql-managed-databases/) — Finnish cloud
- [Scaleway managed databases](https://www.scaleway.com/en/managed-postgresql-mysql/) — French cloud
- [OVH Public Cloud](https://www.ovhcloud.com/en/public-cloud/) — French cloud
- [Exoscale managed PostgreSQL](https://www.exoscale.com/dbaas/postgresql/) — Swiss cloud (Aiven-powered)
- [Elastx](https://elastx.se/) — Swedish managed cloud
- [Ubicloud](https://www.ubicloud.com/) — open-source cloud on Hetzner
- [Ubicloud PostgreSQL](https://www.ubicloud.com/use-cases/postgresql)
- [Ubicloud pricing](https://www.ubicloud.com/docs/about/pricing)

### AWS

- [AWS Fargate pricing](https://aws.amazon.com/fargate/pricing/)
- [AWS RDS PostgreSQL pricing](https://aws.amazon.com/rds/postgresql/pricing/)

### Database Branching

- [Neon pricing](https://neon.com/pricing) — PostgreSQL CoW branching
- [Turso](https://turso.tech/) — libSQL CoW branching
