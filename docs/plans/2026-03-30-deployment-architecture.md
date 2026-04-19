# ADR: Deployment Architecture

**Status:** Draft
**Date:** 2026-03-30
**Supersedes:** [k3s + Pulumi Infrastructure (deployment strategy)](../archive/plans/2026-03-12-deployment-strategy.md) — jointly with [Infrastructure](./2026-03-30-infrastructure.md). The superseded document covered both infrastructure primitives and deployment concerns as one plan. This ADR covers service topology, deployment lifecycle, and preview environments. The [Infrastructure](./2026-03-30-infrastructure.md) doc covers the cluster, storage, certificates, and CI/CD pipeline.
**Related decisions:** [Campaign Collaboration Architecture](./2026-03-25-campaign-collaboration-architecture.md), [Campaign Actor Domain Design](./2026-03-25-campaign-actor-domain-design.md), [Project Structure](./2026-03-26-project-structure-design.md), [Infrastructure](./2026-03-30-infrastructure.md)

---

## Context

The [Project Structure](./2026-03-26-project-structure-design.md) defines four deployment targets with different lifecycles: public site (Astro static), frontend (Vite SPA), server (Rust: Axum + kameo), and ML workers (Python). The [Infrastructure](./2026-03-30-infrastructure.md) doc defines the k3s cluster, Hetzner Volume, Pulumi project structure, and CI/CD pipeline.

This ADR decides how the Rust server is split, how services discover each other, how deployments happen without disrupting active sessions, and how preview environments work. The decisions here are driven by three properties of the system:

1. **The campaign server is stateful.** It holds WebSocket connections, in-memory LoroDocs, and actor trees. Restarting it is not transparent — connected clients lose their sessions.
2. **The platform surface grows over time.** Today it's campaign CRUD and the routing table. The roadmap includes a community marketplace (starter packs, templates, community contributions). The platform has its own feature velocity and shouldn't be coupled to campaign server churn.
3. **Infrastructure changes are high-risk and must be tested before they matter.** Preview environments that exercise the real deployment topology catch operational failures while the stakes are zero.

### Why this decision can't wait

Extracting a service boundary after launch means performing infrastructure surgery under load. Infrastructure changes are unpredictable — even deploying a static site on k3s took a weekend of 12-hour days. Every infra failure absorbed pre-launch with zero users is one that doesn't happen post-launch with real users. The operational experience of deploying, monitoring, and debugging two services only comes from running two services.

---

## Decision

### Three services, one workspace

The Rust server splits into two services: the **platform** and the **campaign server**. Python **ML workers** are the third. All Rust code lives in one Cargo workspace with shared crates that define the interfaces between services.

```
apps/
  platform/      # Platform service binary
  campaign/      # Campaign service binary, takes platform URL as config

crates/
  shared/        # Types, interface traits, auth (JWT validation), libSQL helpers
  platform/      # Axum routes: auth, campaign CRUD, discover, lease management
  campaign/      # Axum + kameo + loro: actor hierarchy, WebSocket, compiler
```

Both binaries compile from the same workspace. The crate boundaries enforce separation at the compiler level.

### One topology everywhere

There is no standalone "all-in-one" binary. Local development, preview environments, and production all run the same split topology: platform and campaign server as separate processes communicating over HTTP, fronted by a reverse proxy that enforces the path-based URL contract. The `Remote*` trait implementation (HTTP calls to the platform) is the only implementation; it's always tested because it's always used. No `Local*` implementations, no "does standalone exercise the same paths as split?" invariant to maintain.

**Local dev** runs via `mise run dev`, which launches five processes in parallel:

| Task | Command | Port |
|---|---|---|
| `dev:site` | `pnpm --filter @familiar-systems/site dev` (Astro) | 4321 |
| `dev:web` | `pnpm --filter @familiar-systems/web dev` (Vite, `base=/app/`) | 5173 |
| `dev:platform` | `cargo run -p familiar-systems-platform` | 3000 |
| `dev:campaign` | `cargo run -p familiar-systems-campaign` | 3001 |
| `dev:proxy` | `caddy run --config Caddyfile.dev` | **8080** |

Contributors only ever open `http://localhost:8080`. Caddy owns the path-based routing (`/` → site, `/app/*` → SPA, `/api/*` → platform, `/campaign/*` → campaign), identical to what Traefik does in k3s. Same-origin everywhere; cargo's incremental compiler and Vite HMR continue to give sub-second iteration on source changes because the binaries run natively, not inside containers.

**Preview and prod** deploy the same binaries to k3s, as separate pods in either a per-PR namespace (preview) or the default namespace (prod). Traefik replaces Caddy as the reverse proxy, but the path contract is the same. See §URL routing and §Preview environments below for the cluster-side details.

**Self-hosting (deferred)** is planned as `docker compose up` with a mounted data directory: same two binaries, same split, same path contract enforced by a containerized Caddy or Traefik. Neither a committed `docker-compose.yml` nor the self-host fake-auth binary (per [app-server PRD §Authentication](./2026-04-11-app-server-prd.md#authentication-and-signup)) exists yet; both are future deliverables. The Caddy-in-dev approach shortens the path to self-hosting because the same reverse-proxy shape (and config semantics) carries over.

> **Deferred: self-hosting without object storage.** The managed hosting architecture treats object storage as the source of truth with local disk as cache. A self-hosted user on a home server doesn't have S3-compatible object storage. Local disk needs to be a valid "source of truth" mode, configured by environment variable, where the object storage writeback is simply disabled. This is straightforward (a config flag and an if-else in the writeback path) but the exact semantics need design when self-hosting is prioritized.

### The platform service

The platform owns everything that exists before a campaign is checked out and independent of which server a campaign lives on:

- **Authentication.** Hanko JWT verification. Token validation happens on both sides of the boundary (the platform verifies tokens for its own routes; the campaign server verifies tokens on WebSocket upgrade), but user identity, profiles, and session management live on the platform.
- **Campaign CRUD.** Create, list, delete, transfer ownership. Campaign metadata lives in platform.db. The campaign's libSQL data file lives in object storage — the platform never opens it.
- **The routing table.** Maps campaign ID → campaign server address. Lease-based: each checkout is a lease with a heartbeat. The platform is the single source of truth for "which server owns which campaign."
- **The checkout endpoint.** `POST /api/campaigns/:id/checkout` → `{ ws_url: "wss://{apex}/campaign/:id/ws", api_base: "https://{apex}/campaign/:id", token: "..." }`. The SPA calls this to acquire access to a campaign. If the campaign isn't checked out, the platform assigns it to the least-loaded campaign server, instructs that server to check out the campaign, and records the assignment in the routing table. The returned URLs are **shard-agnostic** — they carry `campaign_id` but never a `shard_id`, because shard selection is an ingress-layer concern (see §URL routing). At N=1 shard the Ingress routes `/campaign/*` straight to the single shard; at N>1 shards a dedicated `campaign-router` binary reverse-proxies by consulting the routing table. The SPA never observes shard identity.
- **Campaign server health monitoring.** Receives heartbeats from campaign servers, tracks load, detects failed leases.
- **Future: community marketplace.** Starter packs, community templates, contributor profiles, content curation. This is cross-campaign, stateless, CDN-cacheable — a completely different traffic pattern from the campaign server's long-lived WebSocket connections.

The platform's data store is platform.db — a single libSQL file holding users, campaigns, subscriptions, the routing table, and (eventually) marketplace content. Its traffic pattern is bursty, short-lived HTTP requests. It scales horizontally trivially if needed (multiple instances behind a load balancer, Turso Database for shared state).

### The campaign server

The campaign server owns everything that happens after a campaign is checked out. Its internal architecture is defined in the [Campaign Collaboration Architecture](./2026-03-25-campaign-collaboration-architecture.md) and [Campaign Actor Domain Design](./2026-03-25-campaign-actor-domain-design.md). From a deployment perspective, the relevant properties are:

- **Stateful.** Holds WebSocket connections, in-memory LoroDocs in actor trees, and the local libSQL cache of campaign files.
- **Campaign-pinned.** All traffic for a given campaign routes to the same server. No cross-instance state.
- **Long-lived connections.** A GM's editing session may last hours. Restarting the server disconnects all sessions.
- **Independently scalable.** Add servers, update the routing table, new campaigns go to the new server. Rebalancing is "writeback to object storage, update routing table, re-checkout on the new server."

### ML workers

Audio transcription (faster-whisper) and speaker diarization (pyannote) are long-running jobs on GPU nodes. The GPU infrastructure is decoupled from the application cluster — it could be Hetzner GPU boxes, Nebius, or anything with a GPU. Workers are stateless: they receive audio file references, process them, and return structured transcripts. They deploy as k8s Jobs, giving them independence from server deployments and a natural path for A/B testing (the job record carries a model identifier, the worker reads it).

LLM inference (for AI conversations, journal synthesis, entity extraction) is a separate concern — the campaign server calls Nebius token factory endpoints directly over HTTP. No GPU nodes needed on the application side.

### Job dispatch

Job state lives in platform.db — a `jobs` table tracking audio processing work. The campaign server writes a job record when a user uploads audio; the server dispatches to workers; workers return results via HTTP; the server routes results to actors for campaign-scoped processing (entity extraction, journal drafting).

**Why platform.db and not a dedicated queue?** platform.db is already there. A jobs table is a few SQL statements. The interface is "write job, poll job, update job" — if this needs to become Redis or Valkey later, the switching cost is low because the abstraction surface is small. It also deploys cleanly in preview environments (the jobs table comes for free in the copied platform.db). No additional infrastructure dependency for a feature that doesn't yet exist. Pay the complexity tax when the simple approach proves insufficient.

> **Deferred: job dispatch trait.** Job state in platform.db means the campaign server writes across the service boundary. The exact interface (a `JobDispatch` trait, internal HTTP endpoints on the platform) needs design alongside the audio pipeline implementation.

---

### URL routing

URL structure is governed by [app-server PRD §URL architecture](./2026-04-11-app-server-prd.md#url-architecture). All environments (dev, preview, prod) share one apex per environment with path-based routing for every application service. From a deployment standpoint, the relevant operational properties are:

- **Single apex per environment.** Site, SPA, platform API, and campaign shards share one host (`familiar.systems` in prod, `preview.familiar.systems` in preview, `localhost:8080` in local dev) and are routed by path prefix.
- **Priority-ordered path rules on a single reverse-proxy Ingress per host.** Longer prefixes win: `/app`, `/api`, `/campaign` each land at their service; `/` catches the rest and serves the Astro site.
- **`StripPrefix` middleware** strips the path prefix before requests reach backends, so the platform continues to own `/me`, `/campaigns/:id/checkout` on its own routes without prefix-awareness. In k3s this is a Traefik `Middleware` CRD; in local dev it's Caddy's `handle_path` directive.
- **Shard-agnostic campaign URLs.** The checkout API returns `wss://{apex}/campaign/{campaign_id}/ws`; the SPA opens it verbatim. At N=1 shard the Ingress routes `/campaign/*` straight to the single shard. At N>1 shards a dedicated `campaign-router` binary reverse-proxies by consulting the routing table. The SPA never observes shard identity.

Subdomains outside the application's apex (Hanko tenants at `auth.*`, plus any future docs/status/blog surfaces) live on their own DNS and are not part of this routing contract.

#### Bookmarked links and cold checkout

When a user hits a bookmarked link like `/app/campaigns/123/page/456`:

1. **Reverse proxy routes `/app/*` to the web service**, which serves `index.html` for every subpath (SPA fallback). The SPA boots.
2. **SPA calls checkout.** `POST /api/campaigns/123/checkout` — same-origin fetch, no CORS preflight. The reverse proxy routes `/api/*` to the platform. The platform consults the routing table.
3. **If the campaign is already checked out:** the platform returns `{ws_url: "wss://{apex}/campaign/123/ws", ...}`. The SPA opens the WebSocket.
4. **If the campaign is not checked out (cold start):** the platform picks the least-loaded shard, instructs it to check out campaign 123 (shard downloads the libSQL file from object storage, spawns CampaignSupervisor + actors), writes the routing-table entry, and returns the same URL shape. The SPA shows a loading skeleton during checkout; when actors are ready the WebSocket upgrade succeeds, the SPA subscribes to the room for `page/456`, hydrates the TipTap doc, and scrolls.

The cold-checkout flow is the same async protocol described in the [Campaign Collaboration Architecture](./2026-03-25-campaign-collaboration-architecture.md). Only the URL format changed (path vs subdomain); the protocol is unchanged.

---

### Interface boundaries

The cross-service interface is defined by traits in `crates/shared/`. Each trait has one implementation: `Remote*` (HTTP client calling the platform's internal API). The campaign server code takes `impl Trait`.

#### RoutingTable

```rust
trait RoutingTable {
    async fn acquire_lease(
        &self, campaign_id: CampaignId, server_id: ServerId,
    ) -> Result<LeaseGrant, LeaseConflict>;
    async fn release_lease(
        &self, campaign_id: CampaignId,
    ) -> Result<(), Error>;
    async fn heartbeat(
        &self, server_id: ServerId, campaigns: &[CampaignId], load: f32,
    ) -> Result<(), Error>;
    async fn discover(
        &self, campaign_id: CampaignId,
    ) -> Result<CampaignLocation, Error>;
}
```

`RemoteRoutingTable`: HTTP calls to `POST /internal/leases/acquire`, `POST /internal/leases/release`, `POST /internal/leases/heartbeat`. The platform handles atomicity — concurrent lease acquisitions resolve via `INSERT ... WHERE NOT EXISTS`, loser gets 409.

#### Future marketplace traits (illustrative only)

No real design work has been done on the marketplace. The trait sketches below are included only to show that the service boundary accommodates future cross-campaign features. They should not be considered API designs.

```rust
// Illustrative — not designed, not implemented
trait ContentCatalog {
    async fn list_packs(&self, filter: PackFilter) -> Result<Vec<PackSummary>, Error>;
    async fn get_pack(&self, pack_id: PackId) -> Result<PackManifest, Error>;
    async fn download_pack_data(&self, pack_id: PackId) -> Result<PackBundle, Error>;
}

// Reverse direction: user shares content from their campaign
trait PackPublisher {
    async fn publish_pack(
        &self, contributor: UserId, bundle: PackBundle, metadata: PackMetadata,
    ) -> Result<PackId, Error>;
}
```

---

### Graceful restart protocol

The campaign server holds stateful WebSocket connections and in-memory CRDTs. Restarting it is not transparent. The restart protocol minimizes disruption and data loss.

#### Shutdown sequence (SIGTERM handler)

The shutdown is per-campaign, not a global sequence. Each campaign drains independently. The process exits only after all campaigns have completed their drain.

**Global:** Stop accepting new WebSocket connections. Cancel in-flight AI work — each AgentConversation actor drops its HTTP stream to Nebius (closing the connection cancels generation on the inference side). The conversation persists an "interrupted" marker in the conversation history. No partial tool call results are applied — in-flight compiled suggestions that haven't reached a ThingActor are discarded. **The heartbeat to the platform continues throughout the drain.** It is the first thing to start on boot and the last thing to stop before exit. This prevents the platform from expiring leases while campaigns are still writing back to object storage.

**Per campaign (concurrent across all checked-out campaigns):**

1. **Notify connected clients.** Send `server_restarting` over the campaign's WebSocket connections. The SPA drops to its loading skeleton.
2. **Snapshot and writeback.** The CampaignSupervisor tells its actors to snapshot. Each actor writes its LoroDoc to relational data in the campaign's libSQL file. The campaign file flushes to object storage.
3. **Release the lease.** Only after the writeback to object storage is confirmed does the campaign server release the lease for this campaign via the platform's API. Until the lease is released, no other server can check out this campaign. This ordering prevents split-brain: there is no window where the campaign file is being written to object storage while another server is checking it out.
4. **Campaign done.** This campaign's actors are terminated, its resources freed.

**Global:** Once all campaigns have completed their drain and released their leases, stop the heartbeat and exit. k8s starts the new binary.

The `terminationGracePeriodSeconds` on the k8s pod provides the time budget. 30 seconds is sufficient — the writeback writes small SQLite files to local disk and then to object storage. The expensive operation (LLM inference) was cancelled before the per-campaign drain, not waited for.

#### Reconnection sequence (new binary starts)

1. The new binary starts and registers with the platform.
2. Clients reconnect via the SPA's standard reconnection logic. The SPA calls the checkout endpoint, gets the (same or new) shard-agnostic URL, opens a new WebSocket.
3. Campaign files are still on the local Hetzner Volume — they survived the restart. Checkout from local disk is "open the file" — sub-millisecond. The expensive object storage download only happens on cold start (a campaign not previously on this server).
4. Actors reconstruct LoroDocs from relational data via `restore()`. Clients sync via the loro-dev/protocol's rejoin flow.

**User-visible disruption:** a few seconds of "reconnecting..." in the SPA. For a TTRPG tool where sessions happen weekly and active editing is intermittent, this is acceptable.

#### In-flight LLM work

Three levels of "mid-conversation" during a restart, all handled by the same protocol:

- **Tokens streaming, no tool calls yet.** Nothing persisted beyond the user's last message. On reconnect, the UI shows "your last request was interrupted — would you like to retry?" The partial tokens were display-only.
- **Tool calls compiled but not yet applied to ThingActors.** Compiled suggestions in flight between AgentConversation and ThingActors evaporate. The user never saw them. Same recovery as above — the AI re-runs the turn on retry.
- **Tool calls already applied as marks on ThingActor LoroDocs.** Suggestions are safe — they're in the LoroDoc and will be snapshotted during the per-campaign drain. Only the partial assistant response text is lost. Same "interrupted" recovery.

The cost of an interrupted turn is one LLM call on retry, not data corruption.

#### Multi-server rolling restart

When multiple campaign servers exist, restarts are sequential:

1. Drain server A: run the per-campaign shutdown sequence above. Each campaign independently writes back to object storage and releases its lease.
2. Clients reconnect. The checkout endpoint assigns their campaigns to server B (or C) and returns a shard-agnostic URL; ingress-layer routing points `/campaign/{id}/*` at the new shard. Campaigns check out from object storage onto the new server.
3. Server A restarts with the new binary. New campaigns and rebalanced campaigns begin routing to it.

The platform service is not restarted during campaign server rolls. Login, campaign listing, and discovery stay available throughout.

---

### Preview environments

Every PR gets a full preview environment that exercises the production deployment topology. Manifests live in `infra/k8s/preview/*.yaml`; the CI workflow at `.github/workflows/deploy-preview.yml` substitutes per-PR variables via `envsubst` and applies them.

#### What's deployed per PR

- **Site + SPA + platform + campaign** pods, all in split mode, running branch builds. The platform pod is ~64MB RAM for SQLite CRUD; negligible overhead for the confidence of exercising the real service topology on every PR.
- **ML workers** available as k8s Jobs (branch build of the Python worker container).
- **Real LLM inference** via Nebius. The product is AI-assisted; excluding AI from preview means not testing the product.
- **Copied campaign data.** Real campaign files, not fixtures.

All resources live in a k8s namespace `preview-pr-${PR_NUMBER}` on the shared cluster. Every PR reaches the cluster through a shared preview apex and is distinguished by path prefix.

#### Path-based routing per PR

All PRs share the apex `preview.familiar.systems`. Per-PR routing is a path prefix: `/pr-${PR_NUMBER}`. Each PR namespace contains three `Middleware` + `Ingress` pairs, all bound to that apex:

| Path prefix | Service | `StripPrefix` strips |
|---|---|---|
| `/pr-${N}/app` | `web` (nginx serving built SPA) | `/pr-${N}/app` |
| `/pr-${N}/api` | `platform` (:3000) | `/pr-${N}/api` |
| `/pr-${N}` | `site` (nginx serving Astro build) | `/pr-${N}` |

Traefik merges Ingress rules from every namespace bound to the same host and resolves collisions by rule length (longest path prefix wins). PR 42 and PR 43 coexist on `preview.familiar.systems` with no cross-talk: `/pr-42/*` paths route into PR 42's namespace, `/pr-43/*` paths route into PR 43's.

The SPA for each PR is built with Vite `base = /pr-${PR_NUMBER}/app/`, so asset URLs and internal routes resolve under the PR prefix. API and campaign calls are derived at runtime by `apps/web/src/lib/paths.ts` from `import.meta.env.BASE_URL`, producing `/pr-${N}/api/...` and `/pr-${N}/campaign/...` respectively — same origin as the SPA, so no CORS preflight.

The routing pattern in prod is the same shape without the `/pr-${N}` prefix: `familiar.systems/app/`, `familiar.systems/api/`, `familiar.systems/campaign/`, `familiar.systems/`. See §URL routing above.

#### Data setup

The preview data pipeline runs as a k8s Job at namespace creation:

1. **Copy platform.db** from the production volume (or a known-good snapshot).
2. **Scrub:** delete all users who are not contributors. Wipe the routing table and campaign checkout state.
3. **Copy contributor campaign files** from production object storage to a preview-scoped prefix.
4. **Run branch migrations** on the copied platform.db and all copied campaign files. This tests the migration path, not just the post-migration state.

The scrubbed platform.db is the access-control mechanism. Non-contributors authenticate against the preview Hanko tenant (valid JWT) but find no user record in the preview's platform.db, so the server returns 403.

#### Authentication

Preview uses a **separate Hanko tenant** from production:

| Environment | Hanko tenant URL | Registered origin(s) |
|---|---|---|
| Prod | `auth.familiar.systems` | `https://familiar.systems` |
| Preview (all PRs + local dev) | `auth.preview.familiar.systems` | `https://preview.familiar.systems`, `http://localhost:8080` |

Each tenant registers exactly one apex origin per environment. Hanko Cloud does not accept wildcard origins (`https://*.preview.familiar.systems` is rejected with "Origin is not a valid URL or Android APK key hash"), and it exposes no admin API to register origins from CI. Path-based routing is what makes the preview tenant workable: every PR reuses the same preview apex, so the origin list never changes across PRs.

**Consequences of the shared apex in preview:**

- Cookies, localStorage, and sessionStorage at `preview.familiar.systems` are shared across every PR's SPA. Single sign-in covers all PRs; conversely, a storage bug in one PR can pollute state seen by others until the user clears site data.
- Passkeys are technically viable under a single rpID (`preview.familiar.systems`) but are disabled on the preview tenant for contributor-workflow simplicity. Email/passcode is sufficient.

Contributors are added manually by email on the preview tenant; registration is disabled there. The prod tenant opens registration when the product launches publicly.

#### TLS

A single cert-manager `Certificate` covers both apex domains (`familiar.systems`, `preview.familiar.systems`), issued once via DNS-01 + bunny.net. No per-PR certs, no wildcard SANs. See [Infrastructure](./2026-03-30-infrastructure.md) for the cert-manager configuration.

#### Lifecycle

**PR open / sync:** `.github/workflows/deploy-preview.yml` builds the three images (site, web, platform), runs the data-setup Job, then applies the manifests in `infra/k8s/preview/*.yaml` via `envsubst` templating. Template variables are `NAMESPACE` (`preview-pr-${PR_NUMBER}`), `PR_NUMBER`, and per-image tags. A single PR comment posts one URL to open: `https://preview.familiar.systems/pr-${N}/app/`.

**PR close:** `.github/workflows/cleanup-preview.yml` deletes the namespace, which cascade-removes all resources inside (Deployments, Services, Ingresses, Middlewares, PVCs, Jobs). The preview object-storage prefix is cleaned separately.

Per-PR manifests are plain YAML with `${VAR}` placeholders rather than Pulumi-managed. Pulumi owns permanent cluster state (cert-manager, ClusterIssuers, the TLS cert, prod deployments, RBAC, the k8s Provider itself); CI owns ephemeral per-PR state. Ephemeral resources don't deserve Pulumi state overhead, and `kubectl apply` on raw YAML is ~2s per resource.

#### Why real data, not fixtures

Campaign files are self-contained libSQL databases. Copying them is a file operation. Building a fixture generator that produces realistic campaigns — interconnected entities, relationship graphs, session journals, suggestion histories, blocks with marks — would be harder than copying real files, and the output would be less useful for testing because it wouldn't exercise the edge cases that real campaigns accumulate.

---

## Consequences

### What this architecture gives us

- **Independent deployment lifecycles.** The platform can go weeks without a deploy while the campaign server ships daily. Campaign server restarts don't affect login or campaign discovery. Platform deploys don't disconnect editing sessions.
- **Blast radius isolation.** A panic in the actor hierarchy, a LoroDoc reconstruction bug, or a compiler edge case crashes the campaign server. The platform stays up. Users can log in and see their campaign list. The error is "I can't open my campaign" not "the site is down."
- **One topology everywhere.** Native `mise run dev` locally (with Caddy as the front-door reverse proxy on :8080), k3s in preview and prod — but always the same two binaries communicating over HTTP behind a reverse proxy that enforces the path-based URL contract. No standalone binary, no `Local*` implementations, no "does dev match prod?" uncertainty. What you run on your laptop is URL-shaped identically to what runs in production.
- **Self-hosting on a clear path.** The planned `docker compose up` self-hosting story reuses the same split + path-based shape; the Caddy config in dev is the same kind of reverse-proxy config a self-hoster would run. Not yet shipped.
- **Preview environments that test reality.** Every PR exercises the same service topology, data setup, migration path, and auth flow as production. Infra surprises are absorbed at zero cost, not under load.
- **Clear contributor boundaries.** A contributor working on marketplace features touches `crates/platform/` and never needs to understand the actor hierarchy. A contributor working on the collaboration layer touches `crates/campaign/` and never needs to understand marketplace routing. The Cargo workspace enforces compilation boundaries.
- **Graceful restarts are cheap.** A few seconds of "reconnecting..." per deploy, bounded by the per-campaign writeback time (small SQLite files to local disk, then object storage). No data loss beyond in-flight LLM tokens. No split-brain because lease release happens only after writeback confirms.

### What this architecture costs us

- **Two Deployments, two Dockerfiles, two health probe configurations.** Operational surface area. For a solo dev, this is real maintenance — every k8s manifest change is doubled.
- **The internal API between platform and campaign server.** A handful of HTTP endpoints, but they're on the critical path (lease acquisition, discover). If the internal API has a bug, no campaign can be opened. This is a small, critical surface that needs disproportionate testing.
- **Preview environment data freshness.** Copied campaign data is a point-in-time snapshot. If a contributor needs to test against a specific campaign state that has changed since the last copy, they need to re-run the data setup. This is manual but infrequent.
- **Resource split on a single server.** In the single-server phase, two pods share one machine's resources. The platform pod needs minimal resources (~64MB RAM, negligible CPU for SQLite CRUD). The campaign server pod is where the real work happens. Resource requests should reflect this asymmetry.

---

## Key Invariants

- **The platform is the single source of truth for campaign → server routing.** Campaign servers never communicate with each other. All coordination flows through the platform's routing table.
- **A campaign has at most one owning server at any time.** Enforced by the lease model in the platform. Concurrent lease acquisitions resolve atomically — exactly one succeeds.
- **Lease release happens only after writeback.** During shutdown, each campaign's lease is released only after its data has been confirmed written to object storage. The heartbeat continues throughout the drain — it is the first thing to start on boot and the last thing to stop before exit — preventing the platform from expiring leases during writeback.
- **One topology everywhere.** Local dev (`mise run dev` + Caddy on :8080), preview (k3s preview namespace + Traefik), production (k3s default namespace + Traefik), and future self-hosting (`docker compose up` + containerized reverse proxy) all run the same split binaries communicating over HTTP behind a path-based reverse proxy. No standalone-only code paths.
- **Preview environments always use split mode.** PR previews deploy both binaries as separate pods to test the real service topology, internal API, and lease protocol on every PR.
- **All cross-service interfaces are defined as traits in `crates/shared/`.** No implicit dependencies between platform and campaign server. If it's not in a trait, it doesn't cross the boundary.

---

## Decisions Deferred

- **Platform high availability.** Single-instance platform is a single point of failure for login and campaign discovery (not for active editing sessions — those are direct to the campaign server). HA is two instances behind a load balancer sharing a Turso Database. Deferred until user count justifies multiple campaign servers.
- **The marketplace as a third service.** The service boundary accommodates future marketplace features. Whether the marketplace eventually runs as its own service or stays in the platform is a deployment decision, not an architectural one. No design work has been done on marketplace interfaces.
- **Campaign server auto-scaling.** The routing table and lease model support multiple campaign servers. The policy for when to add/remove servers (load thresholds, time-of-day patterns) is deferred to operational experience.
- **Cross-service observability stack.** Correlated request IDs are needed from day one. The full observability stack (distributed tracing, metrics, alerting) is deferred but the correlation IDs must be present from the start so traces can be stitched retroactively.
- **TranscriptionDispatcher actor vs. k8s Job orchestration.** Whether audio job dispatch is a kameo actor in the campaign server (with crash recovery from platform.db) or pure k8s Job scheduling. Both work; the actor approach keeps job lifecycle in one system, the k8s approach leverages existing orchestration.
- **Dedicated queue infrastructure.** Job state starts in platform.db. If throughput or reliability demands it, migrating to Redis/Valkey is low-cost because the interface surface (write job, poll job, update job) is small and abstracted behind a trait.

---

## Spikes (Post-Alpha, Pre-Launch)

### Fast-path same-server restart

**Goal:** Make campaign server deploys effectively invisible to connected users.

**The observation:** On a same-server rolling update, the new pod has the same Hetzner Volume. The local campaign files _are_ the current state. The object storage writeback that currently blocks shutdown is for durability (surviving server loss), not for handoff to the new binary. The handoff can use local files directly.

**What to investigate:**

Two drain modes based on whether the restart is same-server or cross-server:

- **Same-server restart (k8s rolling update, same Volume):** snapshot actors to local libSQL files → release lease → exit. The new pod opens the local files directly and writes back to object storage in the background at its normal ~30-second cadence. Total handoff gap: sub-second (container start + Rust binary boot).
- **Cross-server drain (rebalancing, multi-server rolling restart):** the current design — snapshot → writeback to object storage → release lease → exit. The other server needs the file in object storage because it doesn't have the local copy.

**k8s mechanics to validate:**

- Deployment rolling update with `maxSurge: 1`, `maxUnavailable: 0`. New pod starts alongside old pod. Both mount the same PVC.
- Readiness probe gates on "leases acquired, actors restored" so Traefik doesn't route to the new pod prematurely.
- Verify that the lease model serializes access correctly: old pod closes file handles and releases lease _before_ new pod acquires lease and opens the same files. No concurrent libSQL access.

**What could go wrong:**

- If the old pod crashes (OOMKilled, node failure) instead of gracefully shutting down, local files may be up to ~30 seconds stale (last debounce writeback). This is the existing crash recovery story — it doesn't get worse. The fast path only applies to graceful restarts.
- If both pods try to open the same libSQL file simultaneously, corruption. The lease is the serialization point — validate that it's airtight under all timing conditions.

**The invariant change:** "Lease release only after object storage writeback" becomes "lease release only after local snapshot confirms (same-server) or after object storage writeback confirms (cross-server)." The drain mode is knowable at shutdown time.

**Why post-alpha:** The current design (always writeback to object storage before lease release) is correct and safe. The fast path is an optimization that makes deploys invisible instead of "a few seconds of reconnecting." Worth doing before launch, not worth blocking alpha on.
