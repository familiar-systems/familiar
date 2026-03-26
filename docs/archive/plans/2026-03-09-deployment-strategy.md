# Loreweaver — Deployment Strategy

## Decision

**Coolify on Hetzner, libSQL database-per-campaign files on a Hetzner Volume, Pulumi for IaC.**

This supersedes the [previous deployment strategy](./2026-02-18-deployment-strategy.md), which assumed PostgreSQL and deferred provider/tool decisions. Those decisions are now made. See the [libSQL over PostgreSQL decision](../../discovery/2026-03-09-sqlite-over-postgres-decision.md) for why the database changed.

---

## Infrastructure

| Component               | Choice                             | Notes                                                  |
| ----------------------- | ---------------------------------- | ------------------------------------------------------ |
| **Compute**             | Hetzner Cloud VPS                  | German data center (Falkenstein/Nuremberg)             |
| **Storage**             | Hetzner Volume mounted at `/data/` | libSQL database files, survives VPS replacement        |
| **Backups**             | Hetzner Object Storage             | libSQL files synced on schedule                        |
| **Deployment tool**     | Coolify (self-hosted on VPS)       | Web UI, PR preview deploys, auto-SSL                   |
| **Reverse proxy**       | Traefik (via Coolify)              | Path-based routing, WebSocket support, SSL termination |
| **IaC**                 | Pulumi                             | At `infra/pulumi-cloud/`                               |
| **Authentication**      | Hanko                              | Same instance for production and previews              |
| **Long-term direction** | k3s                                | When single-VPS becomes a bottleneck                   |

---

## Environments

### Local (development)

Everything runs on the developer's machine:

- **5 apps** via `turbo dev`: `site` (Astro dev server), `web` (Vite dev server), `api` (Hono), `collab` (Hocuspocus), `worker` (job consumer)
- **No Docker, no database server.** libSQL files on disk. `:memory:` databases for tests.
- **No remote dependencies** — development works fully offline.

### Production

A single Hetzner VPS running Coolify with 5 containerized apps. All apps mount the Hetzner Volume at `/data/` for database access.

- **Deploy method:** Coolify git-push deploys (GitHub webhook). Each app is a separate Coolify "resource" pointing to the same monorepo with different build contexts/Dockerfiles.
- **Reverse proxy:** Traefik (managed by Coolify) handles SSL, path-based routing, and WebSocket upgrade for the collab server.
- **Database:** libSQL files on the Hetzner Volume. No database server process.

### PR preview environments

Coolify's built-in PR preview feature deploys a preview for every PR at `pr-{id}.preview.loreweaver.no`.

**Database branching via file copy:**

```bash
# Copy platform database
cp /data/platform.db /data/previews/pr-${PR_ID}/platform.db

# Copy campaign databases needed for the preview
cp /data/campaigns/campaign-abc.db /data/previews/pr-${PR_ID}/campaigns/campaign-abc.db

# Clean up platform DB to contain only contributors
sqlite3 /data/previews/pr-${PR_ID}/platform.db < contributors.sql

# Cleanup on PR close
rm -rf /data/previews/pr-${PR_ID}/
```

**Access control (three layers):**

1. **Traefik basic auth** on preview subdomains — shared credentials all contributors know. Outer gate.
2. **Hanko authentication** — preview runs same app code, same Hanko instance. Contributors authenticate with real accounts.
3. **Platform DB filtering** — cleanup script deletes all users except contributors. Contributor emails in version-controlled `contributors.sql`.

---

## Storage Layout

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

**Required PRAGMAs** (set once per connection in `@loreweaver/db`):

```sql
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 5000;
```

See the [libSQL decision doc](../../discovery/2026-03-09-sqlite-over-postgres-decision.md) for the defense-in-depth concurrency analysis.

---

## Routing

Traefik (via Coolify) routes all traffic through a single domain. Order matters — specific paths match first:

- `/app/api/*` → `apps/api` (port 3001)
- `/app/collab/*` → `apps/collab` (port 3002, WebSocket upgrade)
- `/app/*` → `apps/web` static files (SPA fallback: unknown paths serve `/app/index.html`)
- `/*` → `apps/site` static files (landing page, blog, public campaign pages)

No CORS in production. All five deployment targets share one domain.

---

## Five Deployment Targets

Each app has a different lifecycle — deploying one does not affect the others. Each is a separate Coolify resource.

| Target     | Runtime                       | Deploy                                      | Notes                                                                 |
| ---------- | ----------------------------- | ------------------------------------------- | --------------------------------------------------------------------- |
| **site**   | Static files (Traefik serves) | Coolify builds Astro → static HTML          | Content changes deploy independently                                  |
| **web**    | Static files (Traefik serves) | Coolify builds Vite → content-hashed chunks | Served under `/app/`                                                  |
| **api**    | Hono container                | Coolify rolling deploy                      | Stateless, fast restarts                                              |
| **collab** | Hocuspocus container          | Coolify rolling deploy                      | Long-lived WebSocket connections. Must not restart on api deploys.    |
| **worker** | Job consumer container        | Coolify rolling deploy                      | Long-running jobs (10+ min). Must survive deploys of everything else. |

---

## Self-Hosting Story

The same codebase runs on customer infrastructure. No database server needed.

```
docker-compose.yml
├── site      → nginx serving Astro build (port 80/443, /*)
├── web       → nginx serving Vite build (/app/*)
├── api       → Hono container (port 3001)
├── collab    → Hocuspocus container (port 3002)
├── worker    → job consumer container
└── (no database container needed)

volumes:
  ./data:/data    # libSQL files on the host
```

All containers mount the data directory. A self-hoster's experience:

- No PostgreSQL to install, configure, or maintain
- Backup = copy files
- The application code is identical — just file paths

---

## Backup Strategy

libSQL files on the Hetzner Volume are backed up to Hetzner Object Storage.

1. **WAL checkpoint** before backup (`PRAGMA wal_checkpoint(TRUNCATE)`) to ensure the `.db` file contains all committed data
2. **Copy files** to Object Storage on a schedule (daily minimum, configurable)
3. **Retention** — keep daily backups for 30 days, weekly for 6 months

Campaign databases are independent files — individual campaigns can be backed up, restored, or exported without affecting others.

---

## Upgrade Paths

| Upgrade            | How                                                                     | Impact                                                               |
| ------------------ | ----------------------------------------------------------------------- | -------------------------------------------------------------------- |
| **Bigger VPS**     | Detach Volume → attach to new VPS → reassign floating IP                | Zero-downtime cutover. Data lifetime independent of server lifetime. |
| **Turso Database** | Swap `@libsql/client` for `@tursodatabase/database` in `@loreweaver/db` | Same files, better engine. Driver swap, not migration.               |
| **k3s**            | Long-term direction when single-VPS becomes a bottleneck                | Pulumi IaC already in place at `infra/pulumi-cloud/`                 |

---

## What This Strategy Defers

| Decision                      | Deferred until                       |
| ----------------------------- | ------------------------------------ |
| **CDN for static assets**     | User base grows beyond single-region |
| **Multi-server architecture** | Single VPS becomes a bottleneck      |
| **Monitoring/observability**  | First production users               |
| **CI/CD pipeline specifics**  | Implementation phase                 |

---

## References

- [libSQL over PostgreSQL decision](../../discovery/2026-03-09-sqlite-over-postgres-decision.md) — why the database changed
- [SPA project structure](./2026-02-14-project-structure-spa-design.md) — the 5-app architecture this strategy deploys
- [Solo dev deployment landscape (archived)](../discovery/2026-02-18-solo-dev-deployment-landscape.md) — full provider and tooling exploration
- [EU deployment landscape (archived)](../discovery/2026-02-18-eu-deployment-landscape.md) — team-scale deployment options
- [Previous deployment strategy (archived)](./2026-02-18-deployment-strategy.md) — PostgreSQL-era strategy
- [Coolify PR preview deploy docs](https://coolify.io/docs/applications/ci-cd/github/preview-deploy)
- [Coolify Traefik basic auth middleware](https://coolify.io/docs/knowledge-base/proxy/traefik/basic-auth)
