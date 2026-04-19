# App Server — Technical Product Requirements

**Status:** Draft
**Date:** 2026-04-11

---

## Context

The app server is the central platform server for familiar.systems. It handles platform-level concerns: authentication, campaign metadata, routing, shard coordination, and billing. It does not participate in real-time collaboration, CRDT sync, or AI inference. Those responsibilities belong to campaign servers (shards).

The app server is a conventional CRUD service. Axum + Tokio, a platform database, Hanko JWT verification. The most interesting thing it does is shard health tracking, and even that is straightforward.

### Relationship to campaign servers

Clients interact with two servers independently. The SPA calls the app server for platform operations (REST/JSON over HTTPS), then connects directly to campaign servers for collaboration (WebSocket, loro-protocol binary frames). The app server is never in the CRDT hot path.

Campaign servers are the only processes that touch campaign data. The app server never reads from or writes to campaign libSQL files or object storage. This is structural: "nothing happens to a campaign without checkout" applies to the app server too.

Both binaries verify Hanko JWTs independently. Shared auth code lives in `crates/shared/`. Clients connect to shards via subdomain (`c1.familiar.systems`), so CORS configuration is required on both servers. Auth tokens are bearer tokens and are subdomain-agnostic.

---

## Responsibilities

### Authentication and signup

The platform is **auth-mode-agnostic**. It always validates bearer tokens against an upstream identity provider that speaks the Hanko wire protocol. The platform itself never decides whether auth is enabled, what the tenant URL is, or who the authority is. All of that is configured at deploy time via the `HANKO_API_URL` environment variable.

**Implications:**

- One code path. The auth middleware does not branch on "is auth enabled?" There is no sentinel value, no `Option<HankoConfig>`, no special-case logic. A misconfigured deployment fails closed (token validation fails, requests are rejected) rather than failing open.
- Pluggable upstream. Production points at a managed Hanko tenant (`auth.familiar.systems`); contributor preview points at a separate Hanko tenant restricted to registered contributors (`auth.preview.familiar.systems`). A future self-host configuration will point at a locally-run fake auth provider: a small, separate binary that speaks the Hanko wire protocol and accepts any email with no password. Self-hosters opt in by running that binary; the platform code does not change.
- Hanko JWT verification middleware on all authenticated endpoints.
- User registration flow handled by the upstream provider.
- User profile storage in the platform database.

**Why this shape:**

A self-hoster does not configure "no auth"; they configure "a different auth." Because the platform sees every deployment mode as an ordinary auth flow, every downstream feature (campaign ownership, suggestion provenance, audit trails, billing) works identically across all three modes. The fake auth provider also makes the system trivially scriptable for LLM agents that need a real platform identity without a Hanko account.

The fake auth provider is **not implemented today**. Today the only path is the Hanko-backed flow (preview tenant for contributor dev, prod tenant for production). The architecture above is the chosen shape; the self-host fake-provider work is a separate future deliverable.

### Campaign metadata CRUD

- Create, read, update campaign metadata: name, description, game system, thumbnail
- Campaign metadata lives in the platform database, not in the campaign libSQL file
- Creating a campaign creates a platform DB record. The campaign libSQL file does not exist until first checkout on a shard.

### Campaign membership and access control

- Three roles per campaign: owner, GM, player. Owner is the billing target — their subscription and credits are charged. Owner can be any member, including a player. Exactly one owner per campaign.
- GM and player are functional roles controlling tool availability and edit permissions on the campaign server. Owner is a billing role and is orthogonal — an owner who is a player has player-level permissions.
- Invite flow (generate invite, accept invite)
- Campaign join/leave
- Ownership transfer
- Membership data is the authority for "who is allowed to connect" — campaign servers verify membership on WebSocket connection

### Campaign list

- Return all campaigns a user has access to, with metadata and role
- Served from the platform database without involving any shard

### Routing table

- Maps campaign ID → shard address
- Consulted by the SPA to determine which shard to connect to
- Updated when campaigns are checked out, released, or reassigned
- Single-server ownership enforced by leases

### Shard registry

- Campaign servers register with the app server on startup
- Shards report capacity and current load
- The app server selects the least-loaded shard when assigning a campaign for checkout

### Campaign checkout orchestration

- Client requests access to a campaign
- App server checks routing table: if already checked out, return shard address
- If not checked out, select a shard, instruct it to check out the campaign, update routing table, return shard address
- The SPA waits for checkout to complete before opening a WebSocket to the shard

### Shard heartbeat and lease management

- Shards send periodic heartbeats
- Heartbeat confirms liveness and renews leases on checked-out campaigns
- If a shard stops heartbeating, leases expire after a timeout
- Expired leases trigger campaign reclaim: routing table entry removed, campaign available for checkout on another shard

### Campaign deletion

- Always goes through a shard, even if the campaign is not currently checked out
- If not checked out: app server triggers checkout on a shard, shard deletes campaign data (local + object storage), shard notifies app server
- If checked out: shard deletes campaign data, notifies app server
- App server removes routing table entry and platform DB records (metadata, membership)

### Public campaign showcase

- Endpoint serving campaign metadata for the Astro static site build
- Limited to what the app server actually has: name, description, game system, GM-written blurb
- No campaign-internal data (entity counts, NPC lists, auto-generated summaries) — that lives in campaign libSQL files and the app server does not access it
- Richer showcase content (if ever needed) would be served directly by the campaign server for checked-out campaigns, not aggregated by the app server

---

## Billing

### Current state (launch)

Pricing is independent of token usage. Flat monthly subscription tiers (Notebook at €5/month, Notebook + Audio at €10/month). No per-token metering required at launch. The billing system needs to track subscription status, not usage.

### Future state (post-launch)

Usage-based billing for LLM tokens and audio processing minutes. The design is established but implementation is deferred.

**Architecture:** Periodic usage reporting from shards to the app server. The shard reports raw usage accumulated since campaign checkout start: token counts per model and diarization seconds. This is a reduce — the shard accumulates, the app server applies pricing.

**Usage report response:** The app server responds with quota status. At minimum: "keep going" or "cut off," remaining audio time before end of billing period, and whether overages apply. This is critical for UX: if a GM uploads a 3-hour session recording and has 1.5 hours remaining, they need to know before processing begins that this will cost them 2 additional hours at the overage rate.

**Post-rate-limiting evolution:** The response expands to include percentage of quota consumed before campaign checkout, percentage consumed since checkout, percentage remaining, and quota reset timestamp. The shard can derive time remaining from the math. The shard never needs to know the billing cycle boundaries — the app server tells it what it needs.

**Usage attribution:** Usage is billed to the campaign owner, not the user who triggered the LLM call. The shard does not need to track per-user usage at launch. Per-user usage breakdown ("this user consumed X% of your cap") is a potential future feature but is not required.

**Key constraints:**

- Pricing formulas always live on the app server. Shards never hardcode rates. Pricing changes do not require shard redeployment.
- There is an over-spend window between usage reports. This is acceptable for the usage patterns of individual GMs (not high-frequency API consumers). The window is bounded by the reporting interval.
- Balance resets (monthly rollover) are managed by the app server. Shards do not track billing cycles. The next usage report response simply reflects the updated balance.
- Usage reporting and heartbeat are separate calls with independent cadences. Heartbeat is frequent (liveness detection). Usage reporting fires at a longer interval or on-demand after large jobs. A dedicated actor on the campaign server manages usage accumulation and reporting.

---

## What the app server does not do

- Real-time collaboration or CRDT sync
- AI inference or LLM calls
- Audio processing or diarization
- Read or write campaign libSQL files
- Access object storage
- Proxy WebSocket connections between clients and shards
- Know about TipTap, Loro, ProseMirror, or document structure

---

## Technology

- **Runtime:** Axum + Tokio
- **Auth:** Hanko JWT verification (shared with campaign server via `crates/shared/`)
- **Database:** Platform database (technology TBD — libSQL, Postgres, or otherwise). Single instance, does not need to scale beyond one for the foreseeable future.
- **Deployment:** k3s on Hetzner (hel1), alongside but independent of campaign server deployments

---

## SPA integration

The SPA manages two independent server connections:

1. **App server** (`api.familiar.systems`): platform CRUD via REST/JSON fetch calls
2. **Campaign server** (`c1.familiar.systems`, etc.): collaboration via WebSocket

The "enter campaign" flow from the SPA's perspective:

1. Call app server: "where is campaign X?"
2. App server returns shard address (triggering checkout if needed)
3. SPA waits for checkout confirmation
4. SPA opens WebSocket directly to the campaign server
5. Collaboration begins

This transition is the one place where the two-server architecture is visible to the frontend. The SPA needs a clear state machine for this flow.

---

## Open questions

- Platform database technology choice (libSQL vs Postgres vs other)
- Heartbeat interval and lease expiry timeout values
- Usage reporting interval and actor design on the campaign server side
- Invite flow mechanics (link-based, code-based, or both)
- Whether campaign metadata updates (name, description) should propagate to checked-out shards or only matter on the platform side
- Fake auth provider for self-hosters: wire protocol coverage, packaging (separate binary vs. embedded dev mode), distribution channel
