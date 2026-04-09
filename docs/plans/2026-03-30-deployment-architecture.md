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

### Docker Compose as the local and self-hosting topology

There is no standalone "all-in-one" binary. Local development, preview environments, production, and self-hosting all run the same split topology: platform and campaign server as separate processes communicating over HTTP.

```yaml
# docker-compose.yml (local dev / self-hosting)
services:
    platform:
        build: { context: ., dockerfile: Dockerfile.platform }
        ports: ["3000:3000"]
        volumes: ["./data:/data"]
        environment:
            DATABASE_URL: /data/platform.db

    campaign:
        build: { context: ., dockerfile: Dockerfile.campaign }
        ports: ["3001:3001"]
        volumes: ["./data:/data"]
        environment:
            PLATFORM_URL: http://platform:3000
            CAMPAIGN_DATA_DIR: /data/campaigns
```

This eliminates an entire class of code: no `Local*` trait implementations, no standalone binary, no "does standalone exercise the same paths as split?" invariant to maintain. The `Remote*` trait implementation (HTTP calls to the platform) is the only implementation. It's always tested because it's always used.

Docker Compose is also the self-hosting story: `docker compose up` runs the full stack without k8s, Pulumi, or any cloud infrastructure. Users mount a data directory with their libSQL files and they're running.

> **Deferred: self-hosting without object storage.** The managed hosting architecture treats object storage as the source of truth with local disk as cache. A self-hosted user on a home server doesn't have S3-compatible object storage. Local disk needs to be a valid "source of truth" mode, configured by environment variable, where the object storage writeback is simply disabled. This is straightforward (a config flag and an if-else in the writeback path) but the exact semantics need design when self-hosting is prioritized.

### The platform service

The platform owns everything that exists before a campaign is checked out and independent of which server a campaign lives on:

- **Authentication.** Hanko JWT verification. Token validation happens on both sides of the boundary (the platform verifies tokens for its own routes; the campaign server verifies tokens on WebSocket upgrade), but user identity, profiles, and session management live on the platform.
- **Campaign CRUD.** Create, list, delete, transfer ownership. Campaign metadata lives in platform.db. The campaign's libSQL data file lives in object storage — the platform never opens it.
- **The routing table.** Maps campaign ID → campaign server address. Lease-based: each checkout is a lease with a heartbeat. The platform is the single source of truth for "which server owns which campaign."
- **The discover endpoint.** `GET /api/campaigns/:id/connect` → `{ websocket: "wss://c1.familiar.systems/ws", api: "https://c1.familiar.systems/api", token: "..." }`. The SPA calls this to find its campaign server. If the campaign isn't checked out, the platform assigns it to the least-loaded campaign server. The SPA uses the returned `api` URL for all campaign-scoped REST (suggestion review, entity queries, conversation messages) and the `websocket` URL for CRDT sync. Each campaign server has its own routable subdomain (`c1`, `c2`, ...), which scales to multi-server without changing the routing pattern.
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

### URL routing and subdomain model

Each service class gets its own subdomain:

- `familiar.systems` → CDN (Astro static site)
- `app.familiar.systems` → CDN (Vite SPA)
- `api.familiar.systems` → platform pod
- `c1.familiar.systems` → campaign server pod (in single-server mode, there's one; in multi-server, `c1`, `c2`, `c3`, etc.)

The SPA talks to the platform for authentication, campaign listing, and discover. After discover, the SPA talks directly to the campaign server for everything campaign-scoped (WebSocket sync, REST queries, conversation messages). No path-based routing ambiguity between platform and campaign traffic.

#### Bookmarked links and cold checkout

When a user hits a bookmarked link like `/campaign/123/page/456`:

1. **CDN serves the SPA.** The URL is a client-side route. `app.familiar.systems` returns `index.html` for all paths. The SPA boots instantly.
2. **SPA calls discover.** `GET api.familiar.systems/api/campaigns/123/connect`. The platform checks the routing table.
3. **If the campaign is already checked out:** Platform returns the campaign server's address. SPA connects.
4. **If the campaign is not checked out (cold start):** Platform assigns it to the least-loaded campaign server and records the assignment. Returns that server's address. The campaign server receives the WebSocket connection, sees it doesn't have campaign 123 locally, acquires the lease from the platform, pulls the libSQL file from object storage (or opens it from local disk if cached), spawns the CampaignSupervisor and actors. The SPA shows a loading skeleton during checkout. When actors are ready, the SPA syncs page 456 via loro-dev/protocol and renders.

The cold checkout path is the same async flow described in the [Campaign Collaboration Architecture](./2026-03-25-campaign-collaboration-architecture.md). The bookmark just works — it's a few seconds slower on a cold start while the libSQL file downloads from object storage.

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
2. Clients reconnect via the SPA's standard reconnection logic. The SPA calls the discover endpoint, gets the (same or new) server address, opens a new WebSocket.
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
2. Clients reconnect. The discover endpoint assigns their campaigns to server B (or C). Campaigns check out from object storage onto the new server.
3. Server A restarts with the new binary. New campaigns and rebalanced campaigns begin routing to it.

The platform service is not restarted during campaign server rolls. Login, campaign listing, and discovery stay available throughout.

---

### Preview environments

Every PR gets a full preview environment that exercises the production deployment topology.

#### What's deployed per PR

- **Platform pod + campaign server pod** in split mode (two pods). Running branch builds of both binaries. The platform pod is ~64MB RAM for SQLite CRUD — negligible overhead for the confidence of testing the real service topology on every PR.
- **SPA** serving the branch frontend build.
- **ML workers** available as k8s Jobs (branch build of the Python worker container).
- **Real LLM inference** via Nebius. The product is AI-assisted — excluding AI from preview means not testing the product.
- **Copied campaign data.** Real campaign files, not fixtures.

All deployed in a k8s namespace `preview-pr-${PR_NUMBER}` with subdomain routing: `app-pr-N.preview.familiar.systems` (SPA), `api-pr-N.preview.familiar.systems` (platform), `c1-pr-N.preview.familiar.systems` (campaign server). Hyphens instead of dots because wildcard TLS certs only cover one subdomain level.

#### Data setup

The preview data pipeline runs as a k8s Job at namespace creation:

1. **Copy platform.db** from the production volume (or a known-good snapshot).
2. **Scrub:** delete all users who are not contributors. Wipe the routing table and campaign checkout state.
3. **Copy contributor campaign files** from production object storage to a preview-scoped prefix.
4. **Run branch migrations** on the copied platform.db and all copied campaign files. This tests the migration path, not just the post-migration state.

The scrubbed platform.db is the access control mechanism. Non-contributors authenticate against the shared Hanko instance (valid JWT) but find no user record in the preview's platform.db. The server returns 403. No separate Hanko configuration needed.

#### Authentication in preview

Hanko's `allowed_redirect_urls` supports wildcard globbing: `https://*.preview.familiar.systems` covers all PR preview environments. This is configured once alongside the production URL. Contributors log in with their normal credentials — same Hanko instance, same passkeys. The preview SPA hides the registration UI to prevent accidental account creation by non-contributors.

#### Lifecycle

**PR open/push:**

1. GHA builds images, tags with `pr-${PR_NUMBER}`, pushes to Scaleway CR.
2. Create the namespace and deploy all services:

```bash
# Create namespace
kubectl create namespace preview-pr-${PR_NUMBER} --dry-run=client -o yaml | kubectl apply -f -

# Run data setup job (copy + scrub platform.db, copy campaign files, run migrations)
kubectl -n preview-pr-${PR_NUMBER} apply -f preview-data-setup-job.yaml
kubectl -n preview-pr-${PR_NUMBER} wait --for=condition=complete job/data-setup --timeout=120s

# Deploy platform pod, campaign pod, SPA, and ingress
kubectl -n preview-pr-${PR_NUMBER} apply -f - <<EOF
# Platform Deployment
apiVersion: apps/v1
kind: Deployment
metadata:
  name: platform
spec:
  replicas: 1
  selector:
    matchLabels:
      app: platform
  template:
    metadata:
      labels:
        app: platform
    spec:
      containers:
      - name: platform
        image: ${REGISTRY_ENDPOINT}/platform:pr-${PR_NUMBER}
        ports:
        - containerPort: 3000
        env:
        - name: DATABASE_URL
          value: /data/preview/pr-${PR_NUMBER}/platform.db
      imagePullSecrets:
      - name: scaleway-cr
---
# Campaign Server Deployment
apiVersion: apps/v1
kind: Deployment
metadata:
  name: campaign
spec:
  replicas: 1
  selector:
    matchLabels:
      app: campaign
  template:
    metadata:
      labels:
        app: campaign
    spec:
      containers:
      - name: campaign
        image: ${REGISTRY_ENDPOINT}/campaign:pr-${PR_NUMBER}
        ports:
        - containerPort: 3001
        env:
        - name: PLATFORM_URL
          value: http://platform:3000
        - name: CAMPAIGN_DATA_DIR
          value: /data/preview/pr-${PR_NUMBER}/campaigns
      imagePullSecrets:
      - name: scaleway-cr
---
# SPA Deployment
apiVersion: apps/v1
kind: Deployment
metadata:
  name: web
spec:
  replicas: 1
  selector:
    matchLabels:
      app: web
  template:
    metadata:
      labels:
        app: web
    spec:
      containers:
      - name: web
        image: ${REGISTRY_ENDPOINT}/web:pr-${PR_NUMBER}
        ports:
        - containerPort: 80
      imagePullSecrets:
      - name: scaleway-cr
---
# Services
apiVersion: v1
kind: Service
metadata:
  name: platform
spec:
  selector:
    app: platform
  ports:
  - port: 3000
---
apiVersion: v1
kind: Service
metadata:
  name: campaign
spec:
  selector:
    app: campaign
  ports:
  - port: 3001
---
apiVersion: v1
kind: Service
metadata:
  name: web
spec:
  selector:
    app: web
  ports:
  - port: 80
---
# Ingress — subdomain routing matches production topology
# Note: subdomains use hyphens (app-pr-N) not dots (app.pr-N) because
# wildcard certs only cover one level. *.preview.familiar.systems covers
# app-pr-1.preview.familiar.systems but NOT app.pr-1.preview.familiar.systems.
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: preview
spec:
  tls:
  - hosts:
    - app-pr-${PR_NUMBER}.preview.familiar.systems
    - api-pr-${PR_NUMBER}.preview.familiar.systems
    - c1-pr-${PR_NUMBER}.preview.familiar.systems
    secretName: preview-wildcard-tls
  rules:
  - host: api-pr-${PR_NUMBER}.preview.familiar.systems
    http:
      paths:
      - path: /
        pathType: Prefix
        backend:
          service:
            name: platform
            port:
              number: 3000
  - host: c1-pr-${PR_NUMBER}.preview.familiar.systems
    http:
      paths:
      - path: /
        pathType: Prefix
        backend:
          service:
            name: campaign
            port:
              number: 3001
  - host: app-pr-${PR_NUMBER}.preview.familiar.systems
    http:
      paths:
      - path: /
        pathType: Prefix
        backend:
          service:
            name: web
            port:
              number: 80
EOF
```

The wildcard TLS cert (`*.preview.familiar.systems`) is shared across all preview namespaces, issued once by cert-manager via DNS-01 + bunny.net. See [Infrastructure](./2026-03-30-infrastructure.md) for cert-manager configuration.

**PR close:**

```bash
kubectl delete namespace preview-pr-${PR_NUMBER}
```

Namespace deletion cascades — all Deployments, Services, Ingress, and Jobs are torn down. Preview object storage prefix is cleaned up separately.

#### Why real data, not fixtures

Campaign files are self-contained libSQL databases. Copying them is a file operation. Building a fixture generator that produces realistic campaigns — interconnected entities, relationship graphs, session journals, suggestion histories, blocks with marks — would be harder than copying real files, and the output would be less useful for testing because it wouldn't exercise the edge cases that real campaigns accumulate.

---

## Consequences

### What this architecture gives us

- **Independent deployment lifecycles.** The platform can go weeks without a deploy while the campaign server ships daily. Campaign server restarts don't affect login or campaign discovery. Platform deploys don't disconnect editing sessions.
- **Blast radius isolation.** A panic in the actor hierarchy, a LoroDoc reconstruction bug, or a compiler edge case crashes the campaign server. The platform stays up. Users can log in and see their campaign list. The error is "I can't open my campaign" not "the site is down."
- **One topology everywhere.** Docker Compose locally, k8s in prod and preview — but always the same two services communicating over HTTP. No standalone binary, no `Local*` implementations, no "does dev match prod?" uncertainty. What you run on your laptop is what runs in production.
- **Self-hosting for free.** Docker Compose with a data directory is the self-hosting story. No k8s required. The same images, the same config, the same split.
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
- **One topology everywhere.** Local dev (Docker Compose), preview (k8s), production (k8s), and self-hosting (Docker Compose) all run the same split binaries communicating over HTTP. No standalone-only code paths.
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
