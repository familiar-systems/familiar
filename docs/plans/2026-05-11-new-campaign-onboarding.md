# New Campaign Onboarding

**Status**: design. The platform tier (`apps/platform`) is live in prod at `app.familiar.systems` and on every PR preview, serving `/health` and `/me` against a SQLite-backed `users` entity. This design adds the campaigns subsystem to it. The campaign tier (`apps/campaign`) exists as a workspace binary but is not yet containerized, not in the CI matrix, and not in Pulumi; this design also stands it up for the first time as the fifth deploy target.

This document is the authoritative design for:

1. The new-campaign wizard (mockup at `tmp/NewCampaignOnboarding/Campaign Onboarding.html`).
2. The catalog of game systems and the bundled templates that ship with each.
3. The on-disk template DSL and YAML→Loro compiler.
4. The campaign-creation flow across platform and campaign tiers, from the hub's "new campaign" click through a campaign that has its game system, templates, and name configured.

The architectural shape: **the platform tier is domain-blind.** It knows what a `CampaignId` is, who owns it, and which shard hosts it. It does not parse templates, validate slugs, or interpret game-system or locale identifiers. The campaign tier owns the catalog endpoint, slug validation, the compiler, template instantiation, and the source of truth for campaign metadata. The platform mirrors `name`, `tagline`, `game_system`, and `content_locale` as opaque strings for hub-listing performance only.

## Reading list (fresh-context agent, in order)

1. **`~/git/familiar-systems/experiment-single-campaign-editor/tiptap-loro-kameo-rust/CLAUDE.md`** — the prototype the campaign server is being built from (lives outside this repo's worktree, hence the absolute path). Read its Architecture section to understand the actor topology (CampaignSupervisor + ThingActor + TocActor + DatabaseActor) and the CRDT sync flow. The campaign tier in this design is essentially this prototype plus a registration call back to the platform and a catalog route.
2. **[`docs/plans/2026-02-20-templates-as-prototype-pages.md`](2026-02-20-templates-as-prototype-pages.md)** — the templates-are-Things model. Why `Template`/`TemplateField` was rejected, what `prototypeId` means, and how categorization works through the graph.
3. **[`docs/plans/2026-05-04-campaign-actor-domain-design.md`](2026-05-04-campaign-actor-domain-design.md)** §Campaign Startup Lifecycle — how a fresh checkout transitions through `Starting → Restoring → Ready`. New-campaign init is a minimal variant of that flow.
4. **[`docs/plans/2026-04-11-app-server-prd.md`](2026-04-11-app-server-prd.md)** §URL architecture and §Campaign CRUD — the routing-table model and the platform's role.
5. **[`docs/plans/2026-02-22-ai-prd.md`](2026-02-22-ai-prd.md)** — the AI works in ProseMirror-shaped Loro state, which is why the template compiler is Rust (not TypeScript).
6. **[`docs/plans/2026-03-26-project-structure-design.md`](2026-03-26-project-structure-design.md)** — shared crate rules (`app-shared` vs `campaign-shared`), ts-rs codegen pattern.

Existing code worth reading before touching:

- [`apps/platform/src/main.rs`](../../apps/platform/src/main.rs): live Axum entrypoint. Routes mounted today are `/health` and `/me`. New routes (`/api/campaigns`, `/internal/*`) register here.
- [`apps/platform/src/openapi.rs`](../../apps/platform/src/openapi.rs): schema exports (`CampaignId`, `UserId`, `MeResponse`). New routes' request/response types register here.
- [`apps/platform/src/middleware/auth.rs`](../../apps/platform/src/middleware/auth.rs): `AuthenticatedUser` extractor against the Hanko session. `POST /api/campaigns` and `GET /api/campaigns` reuse this; no new auth wiring.
- [`apps/platform/src/migrations/m20260417_000001_create_users.rs`](../../apps/platform/src/migrations/m20260417_000001_create_users.rs): the only platform migration today. The new `campaigns` and `create_attempts` migrations follow it in date order in the same migrator.
- [`apps/platform/src/entities/users.rs`](../../apps/platform/src/entities/users.rs): the only platform entity today. `campaigns.owner_user_id` references its primary key.
- [`infra/pulumi-cloud/k8s.py`](../../infra/pulumi-cloud/k8s.py) (around L349 to L540): existing prod platform manifests (`platform-pv`, `platform-pvc`, `platform-deployment`, `platform-service`, `platform-strip-api-prefix`, `platform-ingress`). Modifications go on the existing resources; campaign manifests are added as net-new.
- [`infra/k8s/preview/platform-deployment.yaml`](../../infra/k8s/preview/platform-deployment.yaml) and [`platform-pvc.yaml`](../../infra/k8s/preview/platform-pvc.yaml): live preview manifests. Distroless nonroot UID 65532, chown init-container pattern, `/data/platform/platform.db` on a 1Gi HostPath PVC. Campaign-side manifests mirror this shape.
- [`crates/app-shared/src/id.rs`](../../crates/app-shared/src/id.rs): `CampaignId(pub Nanoid)`. `UserId` is `Uuid` (UUIDv7). Both stay as they are.
- [`crates/fs-id/src/lib.rs`](../../crates/fs-id/src/lib.rs) — branded ID infrastructure; no changes needed.
- [`crates/campaign-shared/src/loro/prosemirror.rs`](../../crates/campaign-shared/src/loro/prosemirror.rs) — `ROOT_DOC_KEY`, `NODE_NAME_KEY`, `ATTRIBUTES_KEY`, `CHILDREN_KEY`. The structural constants the compiler uses.
- [`crates/campaign-shared/src/loro/toc.rs`](../../crates/campaign-shared/src/loro/toc.rs) — `TocEntry` enum; instantiated templates become `TocEntry::Thing` entries.
- [`crates/campaign-shared/src/loro/thing.rs`](../../crates/campaign-shared/src/loro/thing.rs) — `ThingHandle`. The concrete `LoroThingDoc` wrapper lives in `apps/campaign/src/loro/thing.rs`.
- [`apps/campaign/src/entities/things.rs`](../../apps/campaign/src/entities/things.rs) — `prototype_id` already exists; `is_template`, `seeded_from`, `seeded_locale`, `seeded_structure_hash`, `seeded_content_hash` get added.
- [`apps/campaign/src/entities/campaign_metadata.rs`](../../apps/campaign/src/entities/campaign_metadata.rs) — gets `name`, `tagline`, `game_system`, `content_locale` columns.
- [`apps/web/src/routes/_authed/index.tsx`](../../apps/web/src/routes/_authed/index.tsx) — hub page hardcodes `hasCampaigns = false`; this design replaces that with a real list and a "create campaign" button.
- [`Caddyfile.dev`](../../Caddyfile.dev) — local Traefik-equivalent. Gets a new `/catalog/*` route to the campaign tier.
- Mockup files in [`tmp/NewCampaignOnboarding/`](../../tmp/NewCampaignOnboarding/): `Campaign Onboarding.html`, `data.js`, `onboarding.jsx`, `tweaks-panel.jsx`, `wax_seal.jsx` — the wizard surface to port.

## Context

The platform binary is live in prod and previews today. It authenticates users against Hanko, persists the `users` entity to a `/data/platform/platform.db` SQLite file on a 1Gi PVC, and exposes `/health` and `/me`. It has no campaign surface yet. The campaign binary exists as a kameo-actor workspace member but is not in the CI matrix or in Pulumi. This design's scope is therefore (a) **extend** the platform with campaign CRUD, routing, and the internal-mirror surface; (b) **stand up** the campaign tier as the fifth deploy target end-to-end (Dockerfile, CI action, preview manifests, prod Pulumi); (c) define the catalog and template system that lives on the campaign tier.

The mockup at `tmp/NewCampaignOnboarding/` introduces two concepts the codebase doesn't yet model:

1. **Game systems** as first-class objects (D&D 5e, PF2e, Blades in the Dark, Mothership, etc.). The campaign tier's `campaign_metadata` doesn't have a `game_system` field today.
2. **Bundled templates** that ship with each system (NPC, Location, Quest, Clock, Score, Investigator, etc.). The codebase has the underpinning for templates-as-Things (`prototype_id` column on the things table) but no `is_template` discriminant, no starter content, no instantiation flow.

The current campaign-creation surface is an empty hub page. This design defines: the catalog of game systems, the on-disk template format, the YAML→Loro compiler, the minimal create-call to allocate a campaign, the in-campaign wizard that walks the GM through configuration, and the schemas on both tiers.

## Decisions

| Decision                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | Why                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Platform tier is domain-blind.** No template parsing, no slug validation, no catalog awareness, no compiler. `game_system` and `content_locale` are stored as opaque mirrored strings, never interpreted.                                                                                                                                                                                                                                                                                                                                                                       | Domain knowledge in the platform binary leaks operational coupling: every new system, template, or locale becomes a platform deploy. The platform's legitimate role is auth, routing, and billing.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| **Catalog lives on the campaign tier.** `GET /catalog/systems` is answered by any shard via Traefik prefix `/catalog/*` round-robin.                                                                                                                                                                                                                                                                                                                                                                                                                                              | The campaign binary already embeds `content/`. Putting the catalog there avoids duplicate embedding and keeps the platform domain-blind. Stale-bundle risk is why this is a route, not a static SPA asset.                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| **`POST /api/campaigns` is minimal.** Body is just an idempotency token; the platform allocates a `CampaignId`, asks a shard to init the campaign, writes the routing row, returns `{ campaign_id }`.                                                                                                                                                                                                                                                                                                                                                                             | Keep the create call platform-only so the platform stays domain-blind. Bundling configuration into create would force the platform to gateway domain-shaped payloads (template slugs, locale, game system) it has no business interpreting. Configuration is the campaign tier's concern: it owns `/initialize`, which atomically commits everything when the GM seals the wizard.                                                                                                                                                                                                                                                                                                     |
| **The wizard runs in-campaign, not pre-campaign.** `POST /api/campaigns` creates a blank campaign and the SPA redirects into it; the wizard overlay renders while `wizard_completed_at IS NULL`.                                                                                                                                                                                                                                                                                                                                                                                  | The campaign tier owns the wizard's single write (`/initialize`); the SPA renders the campaign route with the wizard overlaid; the redirect target is the actual campaign URL. Putting the wizard at a pre-campaign route would force the platform to gateway domain-shaped payloads it should not interpret.                                                                                                                                                                                                                                                                                                                                                                          |
| **The wizard is atomic at Seal, not incremental.** Wizard state is client-only across all steps. On Seal, the SPA fires one call to the campaign tier: `POST /campaign/<id>/initialize` carrying the full payload (game_system, content_locale, name, tagline, template_slugs, wizard_completed_at). The handler runs everything in one SQLite transaction.                                                                                                                                                                                                                       | Modeling the wizard as N incremental writes leaks implementation convenience into the data model. A wizard is an initialization ceremony, not a stream of edits — atomicity matches the user's mental model ("I'm setting things up, then committing"). Incremental writes also create a real UX bug: a user toggling templates on and off would accumulate orphan prototype Things in the DB. Atomicity erases sticky-field race conditions, template idempotency edge cases, and partially-configured intermediate states. Abandonment leaves the campaign blank, which the user can re-walk from scratch — a few clicks lost, which the user has explicitly endorsed as acceptable. |
| **The campaign WebSocket opens on entering the campaign route, not on first editor mount.** Joined to zero rooms during the wizard, but subscribed to supervisor-level pushes (`CampaignPhase`, `server_restarting`, `PersistenceDegraded`). It is the supervisor's campaign-level activity signal: the checkout/checkin lifecycle gates on "active WebSocket connections for this campaign." When the last connection closes, an idle timer starts; if no new connection arrives within the window, the supervisor begins checkin to object storage. Wizard writes stay on REST. | The supervisor needs a campaign-level activity signal to drive checkin. Connection lifecycle is unambiguous and server-observable; the REST-activity-timestamp alternative needs per-handler activity classification, a startup grace window to cover the wizard's read-heavy first 30 seconds, and a second transport for any supervisor-level push during the wizard. The WebSocket is on the critical path post-wizard anyway; opening it on landing avoids defining two presence regimes.                                                                                                                                                                                          |
| **Platform mints `CampaignId`.** Single source of truth for ID allocation.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | With single-source minting, the routing table's PK is the consistency boundary. Collision (1-in-2^126 with Nanoid) is caught by the unique constraint and surfaces as a 5xx the SPA retries with a fresh token. Retry safety comes from three PK constraints (`create_attempts.idempotency_token`, `campaign_metadata.id`, `routing_table.id`); no extra status columns or reconciliation pass.                                                                                                                                                                                                                                                                                        |
| **`CampaignId` is a Nanoid** (21-char URL-safe).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | Short URLs for sharing. With single-source platform minting, 126 bits of entropy plus the routing-table PK constraint make collision a non-design-concern: any improbable conflict surfaces as a 5xx that the SPA retries with a fresh token. `created_at` on the routing row is the authority for time ordering.                                                                                                                                                                                                                                                                                                                                                                      |
| **Idempotency at every retry boundary.** Create-flow: the platform's `create_attempts` table maps `(idempotency_token → campaign_id)`; the shard's `/internal/init` is idempotent via `INSERT OR IGNORE` on `campaign_metadata.id`; the routing-table insert is idempotent via PK. Init-flow: the campaign tier's `/initialize` is guarded by a `wizard_completed_at IS NULL` precondition — a retry against an already-committed campaign returns `409`.                                                                                                                         | Two retryable client calls (`/api/campaigns` and `/initialize`), two idempotency strategies. The create flow uses three PK constraints so retries no-op cleanly. The init flow uses a precondition gate: the SPA's Seal retry on 5xx is safe because the only way a retry can land "again" is if the first attempt did not commit, in which case `wizard_completed_at` is still NULL and the second call proceeds. If the first attempt did commit, the second returns 409 and the SPA dismisses. No status columns, no reconciliation pass.                                                                                                                                           |
| **No reaper.**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | Under atomic Seal, there is no special "abandoned" state. A campaign whose GM clicked "new" and walked away mid-wizard is a fully-formed blank campaign: real ID, empty SQLite file, routing row. Its lifecycle is identical to any other idle campaign — the supervisor checks it in to object storage on idle, the GM picks back up when they're ready (perhaps after the baby has been put to sleep). The user can delete it from the hub like any other campaign. The vanishingly rare "shard wrote but platform routing-insert failed" orphan still costs cents-per-million in object storage; not worth a scheduled job.                                                         |
| **Platform mirrors `name`, `tagline`, `game_system`, `content_locale`** on the routing row as `Option<String>`. Populated by the campaign actor in one mirror call on `/initialize` commit and on every subsequent `Ready` transition / settings edit via `POST /internal/campaigns/<id>/metadata`.                                                                                                                                                                                                                                                                               | Hub listing is a single platform query, no per-shard fan-out. `Option` models the source-of-truth honestly: `NULL` means "campaign hasn't initialized yet" (newly created, wizard not sealed) rather than "campaign has no value." Atomic Seal means the mirror transitions a campaign's row from "all-NULL" to "fully populated" in one update — no partially-filled intermediate state is observable from the hub.                                                                                                                                                                                                                                                                   |
| **Internal-API auth is a shared bearer token, sourced from Scaleway Secrets Manager.** Both `/internal/init` (platform → campaign) and `/internal/campaigns/<id>/metadata` (campaign → platform) are protected by middleware that constant-time-compares the `Authorization: Bearer <token>` header against a process-startup-read secret. The token is symmetric (same value used in both directions) and distinct per environment (prod, preview, local-dev). Pulumi references the secret by path, never by value. Each service accepts a set of valid bearers (`INTERNAL_BEARER_PRIMARY` + optional `INTERNAL_BEARER_SECONDARY`) and sends only the primary; rotation is "deploy with new value as SECONDARY, swap primary/secondary in SM, deploy with old value removed." | Two services, low call volume, layer-3 threat boundary (Ingress and NetworkPolicy carry layers 1 and 2; see §Internal-API defense layers). Bearer is honest about its role as a backstop: catches Ingress drift and namespace misconfiguration, not in-cluster east-west adversaries. One secret, two readers, constant-time compare on the receive side. Symmetric (one token, both directions) trades a small blast-radius increase against doubling the secret count and rotation pages; at two services with reviewable call sites, the trade is right. The two-bearer rotation contract eliminates the rolling-deploy window where some pods hold the old value and others the new. mTLS via cert-manager is the textbook alternative but pays in local-dev pain (Issuer + per-pod certs in `mise run dev`) and operational overhead the threat model does not warrant. Kubernetes ServiceAccount tokens via `TokenReview` are a meaningful middle ground (per-pod identity, automatic rotation, no shared secret) and would land before KMS-signed JWTs if a third internal service joins or call volume grows. |
| **Ingress and NetworkPolicy gate `/internal/*` before the bearer ever runs.** `/internal/*` is never registered in any `Ingress` resource on either tier; external callers cannot reach it via 443. `NetworkPolicy` on each Service allow-lists only the legitimate peer pod label (platform's policy allows `app=campaign`; campaign's policy allows `app=platform`), plus Traefik for the genuinely public paths. Default deny on everything else. | The bearer alone is not a credible defense in a multi-tenant cluster: any pod in any namespace can dial any Service unless policy says otherwise. Ingress is the cheapest control ("don't expose what you don't want reachable") and NetworkPolicy is the cheapest in-cluster control (k3s ships a built-in NetworkPolicy controller alongside Flannel; no CNI swap needed). The bearer is the third layer, scoped to what the first two cannot catch (a future contributor adding a debug Ingress, an Ingress drift after Traefik config edits). |
| **Starter content lives as a module under `apps/campaign/src/`, not a separate crate.**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | The campaign binary is its only consumer. A crate boundary buys nothing without external consumers or a shared dependency.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| **Templates live as `.yaml` files in `content/templates/`** with `common/` and per-system folders. Flat slug namespace across systems. No override mechanic.                                                                                                                                                                                                                                                                                                                                                                                                                      | Community-authorable, schema-validatable, version-controlled. A "Clock" or "Monster" is not owned by any system. A D&D campaign borrowing a Blades clock post-onboarding is a real combination.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| **YAML, schema-validated by JSON Schema.** Compiler is Rust. Compiler is content-agnostic about widget vocabulary.                                                                                                                                                                                                                                                                                                                                                                                                                                                                | v0 templates are slot-and-placeholder structure, not prose; YAML+JSON Schema closes the schema-authority loop with `packages/editor`. Rust compiler shares Loro primitives with the AI. `packages/editor` is the single source of truth for widget vocabulary; the compiler stays out of that contract.                                                                                                                                                                                                                                                                                                                                                                                |
| **Build-time embedded content (`include_dir!`).**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | Templates change with code. Runtime parsing failures in prod are strictly worse than CI rejection.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| **Wizard cannot mint new template kinds.**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | Drop the mockup's "Invent your own" affordance from `TemplatesEditor`. Replacement copy must not promise a feature v0 does not ship: in v0 there is no in-campaign UI for promoting a Thing to `is_template = true` (see §Out of scope). Acceptable replacement: "Missing a template? Pick the closest fit; you can create freeform Things inside the campaign."                                                                                                                                                                                                                                                                                                                       |
| **Co-located localized strings in YAML.** Every translatable field is a `LocalizedString` map. Schema requires `en`.                                                                                                                                                                                                                                                                                                                                                                                                                                                              | Preserves the schema-authority loop (one template = one tree validated against the TipTap extension list). Per-locale-file split drifts in tree structure; externalized message catalogs are indirection without payoff at ~150 strings. Migration to externalized catalogs later is a mechanical extractor pass.                                                                                                                                                                                                                                                                                                                                                                      |
| **UI locale and content locale travel separately.**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | UI locale is a property of the user. Content locale is a property of the campaign, set during the wizard and locked thereafter. A French-speaking GM running an English D&D 5e campaign for English players is a real combination.                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| **Capture template-instantiation hashes** (`seeded_structure_hash`, `seeded_content_hash`, `seeded_locale`) on every Thing in v0. Ship no upgrade UI in v0.                                                                                                                                                                                                                                                                                                                                                                                                                       | Forward-compatible signal. Per-Thing instantiation hashes cannot be reconstructed after the fact; capturing now is a 3-column cost on the Things row that keeps a future upgrade flow possible.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| **No `description` field on campaigns.**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | Surfaced in the mockup but unused product-wise. Less surface, less to sync.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |

## Architecture

### Topology

```
SPA
  ├─ POST /api/campaigns ──► Platform
  │   { idempotency_token }    │
  │                            ├─ upsert create_attempts(token, campaign_id)
  │                            ├─ POST /internal/init ──► Shard (round-robin)
  │                            │   { campaign_id,         INSERT OR IGNORE INTO
  │                            │     owner_user_id }      campaign_metadata(id, owner)
  │                            ├─ INSERT OR IGNORE INTO routing(id, owner, shard_url, ...)
  │                            └─ 200 { campaign_id }
  │
  ▼ redirect to /c/<campaign_id>
  │
  ├─ GET /catalog/systems ──► Campaign tier (any shard via /catalog/*)
  │                           returns SystemEntry[] (locale-resolved)
  │
  └─ Wizard overlay (shown while campaign_metadata.wizard_completed_at IS NULL).
     All steps are client-side state. Nothing hits the server until Seal:

       step 1: pick system          (client state only)
       step 2: pick/unpick templates (client state only)
       step 3: pick locale          (client state only)
       step 4: name campaign        (client state only)
       seal: POST /campaign/<id>/initialize ──► Campaign tier
                { game_system,                  validates everything, then in one
                  content_locale,               SQLite transaction:
                  name, tagline?,                 - writes campaign_metadata fields
                  template_slugs: [...],          - instantiates each template Thing
                  wizard_completed_at: now() }    - sets wizard_completed_at
                                                COMMIT, then fires the platform mirror
                                                (POST /internal/campaigns/<id>/metadata)
                                                with the mirrored field set
                                                (name, tagline, game_system, content_locale).
                                                wizard_completed_at is not mirrored.

The SPA opens the campaign-level WebSocket to the assigned shard on entering the
campaign route, before the wizard renders. The connection is room-multiplexed;
`JoinRequest` is sent only when the user opens a Thing for editing. The supervisor
uses "active WebSocket connections" as its campaign-level activity signal: when
the last connection for a campaign closes, an idle timer starts, and if no new
connection arrives within that window the supervisor begins checkin to object
storage. Wizard writes stay on REST.
```

### Internal-API defense layers

`/internal/*` endpoints on both tiers (platform's `/internal/campaigns/<id>/metadata`, campaign's `/internal/init`) gate through three layers. Each layer carries its own role and its own failure mode; the bearer in §Decisions is the third, not the load-bearing one.

**Layer 1: Ingress (primary).** `/internal/*` is never registered in any `Ingress` or `IngressRoute` resource on either tier. Traefik routes external traffic only to paths declared in an Ingress; absent an Ingress entry, the path returns 404 at the ingress controller and never reaches the backend pod. The discipline is one rule: no public Ingress uses `path: /` or any wildcard that catches `/internal/*` alongside intended paths. Always specific `PathPrefix`. The existing preview platform-ingress.yaml at `/pr-${PR_NUMBER}/api` is the pattern; new tiers (campaign's `/catalog/*` and `/campaign/<id>/*`) copy that shape and add no `/internal/*` entry. The rule is enforced via conftest policy rather than review discipline alone (see §Verification).

**Layer 2: NetworkPolicy (in-cluster).** Ingress does nothing for east-west traffic between pods. Without a NetworkPolicy, any pod can dial `http://platform.<namespace>.svc.cluster.local:3000/internal/init` directly. Each Service that exposes an internal API carries a `NetworkPolicy` that allow-lists only the legitimate peer:

- **Platform Service**: ingress allowed from Traefik (for `/api/*` and `/`) and from pods labeled `app=campaign` in the same namespace (for `/internal/*`).
- **Campaign Service**: ingress allowed from Traefik (for `/catalog/*` and `/campaign/*`) and from pods labeled `app=platform` in the same namespace (for `/internal/init`).

Default deny on everything else. k3s ships with a built-in NetworkPolicy controller (kube-router-based) that enforces policy alongside Flannel; no CNI swap is required. Pod labels (`app=platform`, `app=campaign`) are load-bearing for this layer and must be set on both Deployments.

**Layer 3: Bearer (backstop).** The shared bearer named in §Decisions catches Ingress drift, namespace misconfiguration, and the case where a future contributor adds a debug Ingress and forgets to exclude `/internal/*`. It is not the primary control. If it were, the threat model would not let it be a symmetric process-startup-read secret with no per-call signature or audit.

**Cluster secret topology.** One Scaleway SM secret per environment holds the bearer:

| SM secret name | Consumers | Lifecycle |
| --- | --- | --- |
| `internal-bearer-prod` | prod platform + campaign pods | Rotated on operator initiative |
| `internal-bearer-preview` | every preview namespace's platform + campaign pods | Same value across all preview PRs (preview is shared trust) |

Prod-side flow: Pulumi reads `internal-bearer-prod` via `config.read_secret(...)`, creates a namespace-scoped k8s `Secret` named `internal-bearer` with keys `INTERNAL_BEARER_PRIMARY` and (during rotation) `INTERNAL_BEARER_SECONDARY`. The platform `Deployment` already exists in `infra/pulumi-cloud/k8s.py`; this change adds an `envFrom: [secretRef: { name: internal-bearer }]` block to its container spec. The campaign `Deployment` is added in the same change with the identical mount.

Preview-side flow: the existing GHA `ci_cd_preview.yml` workflow gains a step that fetches `internal-bearer-preview` from SM and creates the same `internal-bearer` k8s Secret in the per-PR namespace. The Secret YAML is templated like everything else under `infra/k8s/preview/`.

Local-dev: bearer is a fixed string in `mise.toml` (or `.envrc` template) checked into the repo. It is not a secret in any meaningful sense; the local-dev value never appears in any deployed environment, and pulling it from Scaleway at every `mise run dev` would add a Scaleway credential dependency for every dev start.

**What is deferred.** Default-deny NetworkPolicy at the namespace level (catches everything not explicitly allowed, including future workloads) is not in v0; per-app policies covering the real call sites land first, and the namespace-wide default deny is a follow-on. Egress policy (e.g., "platform pods may only reach `kube-dns`, `campaign-service`, and `auth.familiar.systems`") is similarly deferred; ingress-only policy is enough for the current threat model. The upgrade path from shared bearer (k8s SA tokens via `TokenReview`, then KMS-signed JWTs) is named in §Decisions.

**Prod rollout sequencing for NetworkPolicy.** k3s's built-in NetworkPolicy controller does not ship a log-only / dry-run mode, so flipping policies on the live platform tier is enforce-or-nothing. The rollout sequence: (1) apply policies to a preview namespace and run the §Verification probes; (2) once the preview soak is clean, apply to the prod namespace; (3) verify the same probes against prod immediately. If the prod probes fail, rolling back is a single `kubectl delete networkpolicy` per resource. Campaign tier follows the same sequence on its own preview-then-prod path.

### Platform's role in create

Both new routes register in the existing `apps/platform/src/main.rs` alongside `/health` and `/me`. Step 1's "auth the user" uses the existing `AuthenticatedUser` extractor at `apps/platform/src/middleware/auth.rs`; no new auth wiring.

`POST /api/campaigns` does exactly:

1. Auth the user.
2. Look up `idempotency_token` in `create_attempts`. If found, jump to step 6 with the stored `campaign_id`.
3. Mint a fresh `CampaignId` (Nanoid). `INSERT INTO create_attempts(token, campaign_id, created_at)`. Race on conflict: re-read the row and use its `campaign_id`.
4. Pick a shard (round-robin in v0).
5. `POST <shard>/internal/init { campaign_id, owner_user_id }`. Shard does `INSERT OR IGNORE INTO campaign_metadata(id, owner_user_id)` and returns 200. Idempotent.
6. `INSERT OR IGNORE INTO routing_table(id, owner_user_id, shard_url, ...)`. Idempotent on PK.
7. Return `200 { campaign_id }`.

The platform never sees `game_system`, `content_locale`, `template_slugs`, `name`, or `tagline` at create time. Those flow in later via the wizard's normal in-campaign writes and the campaign actor's metadata push.

**Failure paths:**

- **Any step fails before step 7.** Platform returns 5xx. SPA retries with the same idempotency token. The `create_attempts` row may or may not exist; the upsert handles both. Steps 5 and 6 are idempotent on PKs. Eventually the retry walks all the way to step 7 successfully.
- **Vanishing Nanoid collision at step 6.** PK conflict on `routing_table.id`. Return 5xx; SPA retries with a fresh idempotency token; new Nanoid gets minted; conflict effectively impossible to repeat (1-in-2^126).

`GET /api/campaigns` does exactly:

1. Auth the user.
2. `SELECT id, name, tagline, game_system, content_locale, created_at FROM campaigns WHERE owner_user_id = ? ORDER BY created_at DESC`.
3. Return `200 [{ id, name, tagline, game_system, content_locale, created_at }, ...]`.

Rows whose mirrored fields are still `NULL` (campaign created, wizard not yet sealed) appear in the list as-is; the SPA renders an "Untitled campaign" placeholder for those rows. The route does not fan out to shards: the mirrored columns on `campaigns` are the only data needed for the hub.

### Repo layout

The slugs and counts here are illustrative.

```
content/
  systems.yaml
  templates/
    common/{npc,location,faction,quest,clock,session-log,note,lore,scene,magic-item,map,recap}.yaml
    dnd-5e/{monster,spell,magic-item}.yaml
    pf2e/{monster,hazard}.yaml
    blades/{score,crew,heat,coin}.yaml
    scum/{job,ship,sector,cargo}.yaml
    apocalypse/{front,threat,hold,move}.yaml
    motw/{hunter,mystery,monster,move}.yaml
    coc/{investigator,mythos,clue,tome,sanity}.yaml
    mothership/{ship,anomaly,module,hold}.yaml
```

System folder names are slugs (kebab-case), matching the `id` field in `systems.yaml`. CI rule (post-v0): every directory under `templates/` other than `common/` must correspond to a `systems[]` entry whose `id` matches the folder name.

### `systems.yaml` shape

YAML throughout `content/` keeps the parser, schema, and authoring story uniform: one `serde_yaml` dependency, one JSON Schema toolchain.

```yaml
# yaml-language-server: $schema=../systems-schema.json
systems:
    - id: dnd-5e
      name: D&D 5e
      tagline: "Heroic high fantasy. Spell slots, milestone leveling, long rests."
      color: "#a23a2a"
      popular: true
      bundle:
          - common/npc
          - common/location
          - common/quest
          - common/clock
          - common/session-log
          - dnd-5e/monster
          - dnd-5e/magic-item
```

`bundle` references templates by full path-slug. The compiler fails at build time if a referenced slug doesn't resolve to a `.yaml` file.

### Source format: YAML, schema-validated

v0 templates are slot-and-placeholder structure, not prose:

- NPC: portrait slot, relationship list, statblock transclusion, a paragraph or two of placeholder prose.
- Location, Faction, Quest, Clock, Score, Crew, Investigator: widget slots plus a paragraph of placeholder prose per section.
- Note: a single placeholder paragraph.
- Session-log: a few labeled headings ("Attendees", "Recap") followed by placeholder paragraphs.

The GM's content (Graydalf is married to Sabrina; the Crew is in debt to the Bluecoats) flows in after the template is instantiated, via the editor at runtime. The template source format only needs to express structure plus short placeholder strings.

The example below is **illustrative**: every node name other than `heading` and `paragraph` (TipTap defaults) is a placeholder. Real names come from the TipTap extensions in `packages/editor`. Strings are shown in non-localized shorthand for readability; the actual on-disk shape wraps every translatable string in a `LocalizedString` map (see [Localization](#localization)).

```yaml
# yaml-language-server: $schema=../../starter-content-schema.json
meta:
    name: NPC
    description: A person and their leverage.

body:
    - node: heading
      attrs: { level: 1 }
      text: "{{ name }}"
    - node: portrait # placeholder name
    - node: relationship-list # placeholder name
    - node: transclude # placeholder name
      attrs: { target: "prototype:statblock" }
    - node: heading
      attrs: { level: 2 }
      text: Notes
    - node: paragraph
      text: "Write about this NPC..."
```

Each `node:` value is a TipTap node type name. The schema (generated from `packages/editor`'s extension list) enumerates valid names and their attribute shapes. `{{ name }}` substitution is **not v0**; templates render with their literal placeholder text on first instantiation.

**Inline marks (bold, italic, links).** v0 templates don't author rich inline content. If a template ever needs marks inside a paragraph, the escape hatch is a `markdown:` node type whose `text` field is a markdown string the compiler parses into a paragraph subtree. Adding this later does not require touching existing templates.

### Widget vocabulary (v0) and schema authority

The TipTap extension list in `packages/editor` is **the single source of truth for document structure that both browser and campaign server must agree on**. The Rust side mirrors a subset of that schema only for nodes Rust _originates_ (mention extraction, suggestion application, both produced by the AI). The existing constants at `crates/campaign-shared/src/loro/prosemirror.rs` declare this: `// These must match the TipTap node specs in packages/editor/`.

The template compiler is content-agnostic about widget vocabulary. It does not import `NODE_MENTION` or any per-widget constant; it does not maintain a widget allowlist. It uses only the structural convention constants already in `prosemirror.rs` (`ROOT_DOC_KEY`, `NODE_NAME_KEY`, `ATTRIBUTES_KEY`, `CHILDREN_KEY`) to lay out the Loro tree. A YAML entry like `- node: portrait` becomes a Loro map with `nodeName = "portrait"` regardless of whether `portrait` is a real TipTap node type. Whether that string corresponds to a renderable extension is the TS side's concern.

**Adding a new widget is a two-place change:**

| Where                       | Change                                                                                                                    |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `packages/editor`           | Add the TipTap extension (a node spec with a name, attribute schema, and renderer).                                       |
| Schema codegen              | Regenerate `thing-template-schema.json` from the extension list (or hand-add the node entry if codegen is not yet wired). |
| `content/templates/**.yaml` | Use the new node type in any template that needs it.                                                                      |

No Rust constant. No Rust compiler change.

**Drift catching, two layers:**

1. **Author time:** the JSON Schema flags an invalid node name in VS Code before anyone hits CI.
2. **Integration test (the drift catcher):** compile every template under `content/templates/`, mount each output document in a real TipTap editor (via the existing test harness used for the `loro-prosemirror` roundtrip), fail if any node falls back to "unknown."

[`docs/plans/2026-02-20-templates-as-prototype-pages.md`](2026-02-20-templates-as-prototype-pages.md) walks through the NPC template ("Graydalf the Wisened") and names the widgets that page needs: portrait widget, relationship list widget, transclusion slot. These are conceptual; no TipTap extensions exist for them in `packages/editor` today. The editor team owns the actual node names, attribute shapes, and rendering. This design's only contract is: once those extensions exist, the YAML's `node:` values match their names and the schema validates.

### Compiler

The campaign server stores all Thing content as ProseMirror-shaped Loro CRDT trees. The AI in Rust reads and writes those trees directly via the constants in `crates/campaign-shared/src/loro/prosemirror.rs`. Every existing Thing in a running campaign is one such tree.

The compiler's contract:

- **Input**: a parsed template body (a typed tree of nodes, post-`serde_yaml::from_str`) plus a target locale.
- **Output**: a `LoroDoc` whose `doc` container holds a ProseMirror-shaped tree representing the template's body with `LocalizedString` maps resolved to the chosen locale (with `en` fallback for missing keys), ready to persist as the initial state of a new Thing.

It uses only the structural constants in `prosemirror.rs` (`ROOT_DOC_KEY`, `NODE_NAME_KEY`, `ATTRIBUTES_KEY`, `CHILDREN_KEY`). It does not import any per-widget constant; it does not validate widget names; it does not know what TipTap extensions exist. It writes whatever node names the template author put in the YAML, trusting the JSON Schema and the integration test to catch typos.

Why this lives in Rust, not TypeScript:

- The Loro write side already lives in Rust (`apps/campaign/src/loro/`). A TS-side compiler would re-derive the same Loro layout, creating a second source of schema knowledge.
- Template instantiation happens in-process on the campaign tier when the wizard picks a template. Calling out to a Node subprocess per instantiation is operational complexity for no win.
- The AI already produces and consumes Loro state in Rust. The instantiator does the same thing the AI does, from a static authored input instead of a model output.

### Module layout

`apps/campaign/src/starter_content/` holds the catalog parser, template parser, compiler, and build-time hash table.

```
apps/campaign/src/starter_content/
  mod.rs
  catalog.rs            # SystemEntry, SystemId; parses systems.yaml
  template.rs           # TemplateMeta, TemplateBody; parses .yaml
  schema.rs             # JSON Schema codegen for the template DSL
  compile.rs            # TemplateBody tree -> LoroDoc (structural constants only)
  hashes.rs             # build-time (slug, locale) -> (structure_hash, content_hash)
```

- Consumers: the catalog route (`GET /catalog/systems`) and the template-instantiation route (`POST /api/things/from-template` or similar; see [Template instantiation](#template-instantiation)).
- The Loro write happens via `LoroThingDoc` in `apps/campaign/src/loro/thing.rs`. The module constructs the `LoroDoc`; the instantiation route persists it as a Thing.
- Embeds the `content/` directory at build time via `include_dir!`. `cargo build` fails if `systems.yaml` references an unresolved slug or a template fails to deserialize.

### Localization

Two locales travel together but stay independent:

- **UI locale** is a property of the user (account preference or browser fallback). Drives wizard chrome, editor toolbar, button labels.
- **Content locale** is a property of the campaign, set during the wizard and locked thereafter. Drives the language of instantiated templates. A French-speaking GM running an English-language D&D 5e campaign for English-speaking players is a real combination, so the campaign's content language is not derivable from the GM's UI locale.

**Source format: localized strings co-located in YAML.** Every translatable field is a locale map. The schema requires `en`; other locales are optional and fall back to `en` at compile time.

```yaml
meta:
    name: { en: NPC, fr: PNJ, de: NSC }
    description:
        en: A person and their leverage.
        fr: Une personne et son emprise.
    icon: person-standing # Lucide-react icon
body:
    - node: heading
      attrs: { level: 2 }
      text: { en: Notes, fr: Notes, de: Notizen }
    - node: paragraph
      text:
          en: "Write about this NPC..."
          fr: "Décrivez ce PNJ..."
```

**Compiler signature.** The compiler takes the locale as a parameter and resolves `LocalizedString` maps inline:

```rust
fn compile(body: &TemplateBody, locale: &Locale) -> LoroDoc
```

`Locale` is a newtype around a BCP-47 tag. Snapshot tests run per `(template, locale)` pair so locale-fallback paths are visible in CI.

**Catalog endpoint.** `GET /catalog/systems` honors `Accept-Language` (or an explicit `?locale=` for testability), returns locale-resolved template `meta` per system. System names (proper nouns: "D&D 5e", "Blades in the Dark") and taglines stay English in `systems.yaml` for v0.

**Wizard.** Two pickers:

- UI language is account-level, set once per user.
- Content language is exposed during the wizard, defaults to UI language, locked once written into `campaign_metadata`. Surfaced in campaign settings as informational thereafter.

**v0 corpus.** v0 may ship with only `en` populated across templates. The schema, compiler, and DB are locale-aware so adding FR/DE/etc. later is a YAML-only edit.

**Out of scope.**

- Re-localizing an existing campaign. Content locale is sticky once written.
- ICU MessageFormat or runtime pluralization.
- Localizing TipTap node names (`portrait`, `relationship-list`). They are stable identifiers in `packages/editor`, not user-facing strings.
- RTL editor layout.

### Template evolution: forward-compatible signals

The new-campaign onboarding flow is not the place to ship a template-upgrade UI. But the choice of what to record at template-instantiation time governs whether a future upgrade flow is possible at all, so v0 captures the signals dormantly even though no UI consumes them yet.

**Four hashes, only two stored.** At instantiation, the compiler computes two hashes per `(template, locale)` pair:

- `structure_hash`: sha256 over a canonical-serialized form of the parsed `TemplateBody` tree (node types, attributes, nesting). Locale-independent.
- `content_hash`: sha256 over `(structure, locale-resolved strings)`. Locale-specific.

Both are computed over `serde::Serialize` to a stable JSON form, not over raw YAML bytes, so whitespace and comments don't trigger false-positive drift. The instantiator writes the instantiation-time pair onto the Things row.

At upgrade trigger (v1+), the upgrade UI computes a third and fourth hash on demand, not stored:

- `current_structure_hash` = `canonical(materialize(Thing.LoroDoc)).structure`
- `current_content_hash` = `canonical(materialize(Thing.LoroDoc)).full`

**The 2x2 of upgrade outcomes.**

| GM modified Thing? | Catalog template changed? | Action                       |
| ------------------ | ------------------------- | ---------------------------- |
| no                 | no                        | nothing                      |
| no                 | yes                       | safe auto-replace (v1)       |
| yes                | no                        | nothing (GM is just editing) |
| yes                | yes                       | manual diff + port (v2)      |

v0 ships the instantiation-time hashes only. v1 lights up the top-right cell. v2 lights up the bottom-right.

**Canonicalization invariant.** Two paths must produce the same canonical form for an unmodified Thing:

```
YAML --parse--> TemplateBody --compile(locale)--> CanonicalDoc --sha256--> seeded_*_hash
LoroDoc --materialize--> CanonicalDoc --sha256--> current_*_hash
```

If the materializer reorders attributes, normalizes inline marks differently than the compiler, or includes Loro internal metadata, every unmodified Thing reports modified at upgrade time. v0 ships a property test that round-trips every `(template, locale)` pair through compile-then-materialize and asserts hash equality.

### Wizard surface (web)

The wizard is an **in-campaign experience**, not a pre-campaign route. After `POST /api/campaigns` returns, the SPA redirects to `/c/<campaign_id>` (or wherever the campaign route lives). On entering the campaign route, the SPA opens the campaign-level WebSocket (joined to zero rooms, subscribed to supervisor-level pushes only). The campaign view detects that `campaign_metadata.wizard_completed_at IS NULL` and renders the wizard overlay, which sits modally over the empty campaign view. The WebSocket is the supervisor's presence signal that keeps the campaign checked out, not a write channel for the wizard.

**Wizard state is client-side until Seal.** Every step of the wizard mutates SPA-local state only. Picking a system, toggling template selections, choosing a locale, typing a name — none of these write to the server. The user can flip choices back and forth (the "I want Clocks. Wait, no I don't." case) without accumulating server state. The wizard is a one-shot initialization ceremony from the data model's perspective; the steps are choices, not commitments.

**Seal is one atomic call.** When the user clicks Seal, the SPA fires:

```
POST /campaign/<id>/initialize
{
  game_system: "<slug>",
  content_locale: "<bcp-47>",
  name: "...",
  tagline?: "...",
  template_slugs: ["common/npc", "common/clock", "dnd-5e/monster", ...],
  wizard_completed_at: <now>
}
```

The handler validates everything (slugs resolve against the catalog, locale is a known locale, payload schema is well-formed) and then, in one SQLite transaction:

1. Writes `name`, `tagline`, `game_system`, `content_locale`, `wizard_completed_at` to `campaign_metadata`.
2. For each `template_slug`, runs the compiler, persists a Thing with `is_template = true` + instantiation hashes, appends a `TocEntry::Thing` to the ToC.
3. Commits.

After commit, the campaign actor fires the platform mirror once with the new mirrored-field state. `wizard_completed_at` is not mirrored.

**Idempotency / retry.** The endpoint is guarded by the `wizard_completed_at IS NULL` precondition. A second call after a successful Seal returns `409 Conflict` (the campaign is already initialized). A retry after a network failure that didn't commit will succeed because `wizard_completed_at` is still NULL. The SPA's retry policy on Seal is: on 5xx, retry the same payload; on 4xx (other than 409), surface the error; on 2xx, dismiss the overlay. **On 409, treat as success and dismiss the overlay** — the only way a 409 reaches the SPA on Seal is if a prior attempt committed (whether this tab or another), so the campaign is already initialized and the wizard is done.

**Validation failures.** If any template slug fails to resolve, or `game_system` is not in the catalog, the entire call returns `400` with a structured error pointing at the offending field. Nothing is written. The user sees one toast and stays on the wizard.

**Abandonment.** Closing the tab mid-wizard discards client-side state. The campaign on the server is still `wizard_completed_at IS NULL` with no templates, no name, no system. On return, the overlay renders fresh; the user re-walks from step 1. This is the intentional UX cost of the atomic model.

**Post-Seal view is intentionally minimal in v0.** Once `/initialize` commits and the overlay dismisses, the campaign view just renders what's in the database as text: name, tagline, game system, content locale, and the ToC's Things by name. No metadata editing UI, no template-adding UI, no in-place renaming. Settings, post-wizard template addition, and any edit-in-place behavior are deferred (see §Out of scope). A future session builds on top of this.

**Threat model note.** Server-side enforcement of "wizard must be complete before X" is not added. A user bypassing the wizard via direct API and writing partial state into their own campaign is layer 1 of the threat model (within-campaign cooperative trust) — their problem, not ours. Auth and per-user authorization on every endpoint are layer 2 and apply as always: a user cannot touch another user's campaign.

**Wizard surface details:**

- **Catalog fetch:** `GET /catalog/systems` on the campaign tier. Returns `SystemEntry[]` with each entry's resolved bundle templates (slug, name, description, source flavor for the `(D&D 5e)` parenthetical that disambiguates same-named templates from different systems). Honors `Accept-Language` (or `?locale=`) so template `meta` fields come back locale-resolved.
- Replace the data inlined in `tmp/NewCampaignOnboarding/data.js` with a fetch from `/catalog/systems`. The fuzzy-match function moves into the React side using a Damerau-Levenshtein on `[name, full]`.
- Drop the "Invent your own" input from `TemplatesEditor` (see `onboarding.jsx:593-609`). Replace with: "Missing a template? Pick the closest fit; you can create freeform Things inside the campaign." (v0 has no in-campaign UI for promoting Things to templates — see §Out of scope.)
- The content-locale picker sits at the system-selection step. Defaults to the GM's UI locale, available locales drawn from the catalog response.
- Mount the wizard as a component conditionally rendered inside the campaign route at `apps/web/src/routes/_authed/c/$campaignId.tsx` (or equivalent). No dedicated `/campaigns/new` route.

### Template instantiation

Template instantiation in v0 has exactly one caller: the Seal handler (`POST /campaign/<id>/initialize`), which invokes it once per `template_slug` inside the init transaction. There is no in-campaign "add a template later" route in v0 — that affordance is deferred (see §Out of scope).

The core operation, called per slug from inside the init transaction:

- Resolves the slug via `starter_content`, parses YAML, runs the compiler with the locale to produce a `LoroDoc`.
- Persists as a Thing with:
    - `is_template = true`
    - `seeded_from = "<slug>"`
    - `seeded_locale = <locale>`
    - `seeded_structure_hash` and `seeded_content_hash` from the build-time hash table
    - `name` resolved from `meta.name` for the locale
    - `prototype_id = NULL` (this Thing is itself a prototype)
- Adds a `TocEntry::Thing` for the new Thing.

The Seal handler deduplicates `template_slugs` client-side before submission and runs everything in one SQLite transaction, so there is one writer and no race. No unique index is required in v0; adding it later if a second writer is introduced is a one-line migration.

### Schema changes

Platform migrations follow the existing `m20260417_000001_create_users.rs`. The new `campaigns` and `create_attempts` migrations adopt date-based names (`m{YYYYMMDD}_*` matching the implementation date) and register in the existing migrator after the users-table migration. `campaigns.owner_user_id` has a foreign-key reference to `users.id`.

**`apps/platform/src/entities/campaigns.rs`** (new entity):

- `id: CampaignId` (PK, Nanoid stored as TEXT)
- `owner_user_id: UserId`
- `shard_url: String` (which shard hosts this campaign)
- `name: Option<String>` (mirrored from campaign tier; `NULL` until the wizard names it)
- `tagline: Option<String>` (mirrored from campaign tier)
- `game_system: Option<String>` (mirrored from campaign tier; opaque, never interpreted; `NULL` until the wizard picks it)
- `content_locale: Option<String>` (mirrored from campaign tier; opaque, never interpreted; `NULL` until the wizard picks it)
- `created_at`, `updated_at`

**`apps/platform/src/entities/create_attempts.rs`** (new entity):

- `idempotency_token: String` (PK, SPA-minted)
- `campaign_id: CampaignId`
- `created_at: DateTime<Utc>`

A routine vacuum job can prune rows older than e.g. 30 days; not load-bearing for correctness.

**`apps/campaign/src/entities/things.rs`** (additive):

- `is_template: bool` (default `false`)
- `seeded_from: Option<String>` (the slug, e.g., `"dnd-5e/monster"`)
- `seeded_locale: Option<String>` (BCP-47)
- `seeded_structure_hash: Option<String>` (sha256, locale-independent)
- `seeded_content_hash: Option<String>` (sha256, locale-specific)

**`apps/campaign/src/entities/campaign_metadata.rs`** (additive):

- `name: Option<String>` (source of truth; mirrored to platform; written by `/initialize`)
- `tagline: Option<String>` (source of truth; mirrored to platform; written by `/initialize`)
- `game_system: Option<String>` (slug; source of truth; mirrored to platform; written by `/initialize`)
- `content_locale: Option<String>` (BCP-47; source of truth; mirrored to platform; written by `/initialize`; sticky — see §Localization)
- `wizard_completed_at: Option<DateTime<Utc>>` (NULL until `/initialize` commits; sticky once set; gates the wizard overlay; not mirrored to platform)

### Implementation Time Questions

- **Campaign idle time.** How long should a campaign remain idle before being checked back in?
    - _With an open websocket_: 5 minutes? An hour? Forever?
    - _Without an open websocket_: 5 minutes? An hour? Not forever.

### Out of scope (deferred)

- **Template evolution UI.** v0 captures instantiation-time hashes and a canonicalization round-trip test that keep the signals meaningful for a future upgrade flow. v1 lights up auto-replace for unmodified Things; v2 adds the manual-port path for modified ones.
- **Re-localizing an existing campaign.** Content locale is sticky.
- **Localized system names and taglines in `systems.yaml`.** v0 keeps these English; migration to `LocalizedString` later is a small change.
- **Adding templates after the wizard.** In v0, templates are only instantiated through the atomic `/initialize` call. There is no in-campaign "add a template from the catalog" affordance — neither for templates inside the campaign's chosen system's bundle nor for cross-system browsing. The data model supports it (Things with `is_template = true` can be created at any time), but no route, no UI, and no test exercises it in v0.
- **Custom user templates promoted to `is_template: true`.** Trivial flag toggle once the column exists; UI work to expose it.
- **Community uploads.** Templates contributed via PR are easy; a registry where users upload without a PR is not v0.
- **Hot-swappable templates.** Loading from object storage at runtime is a follow-on; templates currently change with code via `include_dir!`.
- **Multi-shard scheduling and load-aware shard assignment.** v0 uses round-robin; v1+ adds region affinity, least-loaded, etc.
- **Public-site catalog rendering.** The public Astro site does not need a "browse game systems" page; the catalog is wizard-only.
- **Scheduled sweeping of inactive campaigns.** Users delete their own from the hub; there is no notion of "abandoned" to sweep, since a never-sealed campaign is just an idle campaign in object storage. No janitor in v0.
- **Post-Seal campaign UI beyond text.** The campaign view in v0 reads `campaign_metadata` and the ToC and renders them as text. No settings UI, no in-place editing of `name`/`tagline`/`game_system`, no template-adding affordance, no Thing editor wiring. A future session builds on top of this.
- **Wizard state persistence across reloads.** Wizard state lives in React component state and dies on tab close. No localStorage or session-storage backing. Adding it later is a small change but introduces cache-invalidation surface we don't want while iterating.

## Critical files to modify

| Path                                                            | Change                                                                                                                                                                                                                                                       |
| --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `content/systems.yaml`                                          | new                                                                                                                                                                                                                                                          |
| `content/templates/**/*.yaml`                                   | new (~30 files for v0)                                                                                                                                                                                                                                       |
| `apps/campaign/src/starter_content/`                            | new module; `LocalizedString` type, locale-aware compiler signature, build-time `(slug, locale) → (structure_hash, content_hash)` table                                                                                                                      |
| `packages/editor/`                                              | add TipTap extensions for the widgets the design doc names (portrait, relationship list, transclusion slot); schema authority lives here                                                                                                                     |
| `apps/platform/src/entities/campaigns.rs`                       | new entity (`id, owner_user_id, shard_url, name, tagline, game_system, content_locale, timestamps`)                                                                                                                                                          |
| `apps/platform/src/entities/create_attempts.rs`                 | new entity (`idempotency_token, campaign_id, created_at`)                                                                                                                                                                                                    |
| `apps/platform/src/routes/campaigns.rs`                         | `POST /api/campaigns`: upsert `create_attempts`, mint `CampaignId`, call shard `/internal/init`, insert routing row, return `{ campaign_id }`. `GET /api/campaigns`: return the authenticated user's campaign rows from the mirrored columns. Both register in `apps/platform/src/main.rs` alongside the existing `/health` and `/me` mounts; both use the existing `AuthenticatedUser` extractor. |
| `apps/platform/src/routes/internal.rs`                          | `POST /internal/campaigns/<id>/metadata` receiving the mirror update from the campaign tier; bearer-token middleware on `/internal/*` (see Decisions). Mounted as a nested router in `main.rs` so the middleware only applies to `/internal/*`. |
| `apps/platform/src/migrations/`                                 | new migrations `m{YYYYMMDD}_create_campaigns.rs` and `m{YYYYMMDD}_create_create_attempts.rs`; both register in the existing migrator after the user-table migration. |
| `apps/platform/src/shard_assigner.rs`                           | new module: round-robin in v0; pluggable for v1                                                                                                                                                                                                              |
| `apps/campaign/src/routes/catalog.rs`                           | new: `GET /catalog/systems`                                                                                                                                                                                                                                  |
| `apps/campaign/src/routes/internal.rs`                          | new: `POST /internal/init { campaign_id, owner_user_id }`; idempotent via `INSERT OR IGNORE` on `campaign_metadata.id`; bearer-token middleware on `/internal/*` (see §Internal-API defense layers)                                                          |
| `apps/platform/src/middleware/internal_auth.rs`                 | new: bearer middleware that reads `INTERNAL_BEARER_PRIMARY` (and optional `INTERNAL_BEARER_SECONDARY`) from env at startup; constant-time compare against `Authorization: Bearer <token>`; mounted on `/internal/*` only; 401 on absent/mismatched           |
| `apps/campaign/src/middleware/internal_auth.rs`                 | new: same middleware, same env vars                                                                                                                                                                                                                          |
| `apps/campaign/src/routes/initialize.rs`                        | new: `POST /campaign/<id>/initialize`; validates payload, runs init SQLite transaction (metadata writes + per-slug template instantiation + `wizard_completed_at`), fires platform mirror once on commit; rejects `409` if `wizard_completed_at IS NOT NULL` |
| `apps/campaign/src/entities/things.rs`                          | add `is_template`, `seeded_from`, `seeded_locale`, `seeded_structure_hash`, `seeded_content_hash`                                                                                                                                                            |
| `apps/campaign/src/entities/campaign_metadata.rs`               | add `name`, `tagline`, `game_system`, `content_locale`, `wizard_completed_at`                                                                                                                                                                                |
| `apps/campaign/src/migrations/`                                 | new migration                                                                                                                                                                                                                                                |
| `apps/campaign/src/supervisor/`                                 | on `Ready` transition and on `campaign_metadata` write commit, fire-and-forget metadata mirror to platform                                                                                                                                                   |
| `apps/web/src/routes/_authed/c/$campaignId.tsx` (or equivalent) | conditionally render the wizard overlay when `campaign_metadata.wizard_completed_at IS NULL`; ports the wizard component from `tmp/NewCampaignOnboarding/`                                                                                                   |
| `apps/web/src/lib/api.ts`                                       | add campaign list client (GETs `/api/campaigns`), campaign-creation client (mints idempotency token, POSTs to `/api/campaigns`), and catalog fetch (to `/catalog/systems`) |
| `apps/web/src/routes/_authed/index.tsx`                         | replace the hardcoded `hasCampaigns = false` with a GET against `/api/campaigns`; render the list when populated, the existing `EmptyHubCard` when empty; both paths include a "create campaign" button that POSTs to `/api/campaigns` and redirects to the returned campaign |
| `Caddyfile.dev`                                                 | add `/catalog/*` → campaign-tier routing                                                                                                                                                                                                                     |
| `infra/k8s/preview/platform-deployment.yaml`                    | add pod label `app: platform`; add `envFrom: [secretRef: { name: internal-bearer }]` to source `INTERNAL_BEARER_PRIMARY` and (during rotation) `INTERNAL_BEARER_SECONDARY`                                                                                  |
| `infra/k8s/preview/campaign-deployment.yaml`                    | new: campaign-server Deployment for previews; pod label `app: campaign`; ports 3000; `envFrom` for `internal-bearer`; HostPath volume for campaign sqlite under `/data/campaigns/pr-${PR_NUMBER}`; same nonroot UID + chown init-container pattern as platform-deployment.yaml |
| `infra/k8s/preview/campaign-service.yaml`                       | new: ClusterIP on port 3000, selector `app=campaign`                                                                                                                                                                                                         |
| `infra/k8s/preview/campaign-ingress.yaml`                       | new: Traefik Ingress for `app.preview.familiar.systems/pr-${PR_NUMBER}/catalog/*` and `/pr-${PR_NUMBER}/campaign/*`; StripPrefix middleware mirrors the platform-ingress.yaml pattern; **must not list `/internal/*` under any path**                          |
| `infra/k8s/preview/platform-networkpolicy.yaml`                 | new: ingress on port 3000 allowed from Traefik (kube-system, `app.kubernetes.io/name=traefik`) and from same-namespace pods labeled `app=campaign`; default deny on everything else                                                                          |
| `infra/k8s/preview/campaign-networkpolicy.yaml`                 | new: ingress on port 3000 allowed from Traefik and from same-namespace pods labeled `app=platform`; default deny on everything else                                                                                                                          |
| `infra/k8s/preview/internal-bearer-secret.yaml`                 | new: templated `kind: Secret` named `internal-bearer` with key `INTERNAL_BEARER_PRIMARY` (value substituted by the preview workflow from `internal-bearer-preview` SM secret); optional `INTERNAL_BEARER_SECONDARY` present only during rotation              |
| `infra/pulumi-cloud/k8s.py`                                     | Prod platform manifests already exist in this file (`platform-pv`, `platform-pvc`, `platform-deployment`, `platform-service`, `platform-strip-api-prefix`, `platform-ingress`). **Modify** the existing `platform-deployment` to add pod label `app=platform` and the `internal-bearer` envFrom. **Add** new resources: `platform-networkpolicy`; campaign-side `campaign-pv` (HostPath at `/data/campaigns`, already provisioned on the cluster volume), `campaign-pvc`, `campaign-deployment` (mirroring platform: distroless nonroot, chown init-container, pod label `app=campaign`, `internal-bearer` envFrom), `campaign-service`, `campaign-strip-prefix` Middleware, `campaign-ingress` for `/catalog/*` and `/campaign/*` on the app apex, `campaign-networkpolicy`, and the prod `internal-bearer` k8s Secret backed by SM `internal-bearer-prod`. Prod rollout of the campaign tier is gated on a preview soak (see §Verification). |
| `infra/pulumi-cloud/CLAUDE.md`                                  | document the new SM secrets `internal-bearer-prod` and `internal-bearer-preview`, their consumers, and the two-bearer rotation contract                                                                                                                      |
| `.github/workflows/ci_cd_preview.yml`                           | add a fetch step for `internal-bearer-preview` and a `kubectl apply` of the templated `internal-bearer-secret.yaml` before the Deployment manifests; add `campaign` to the build matrix (currently `[site, web, platform]`) with its own `paths:` filter (`apps/campaign/**`, `crates/campaign-shared/**`, `crates/app-shared/**`, `crates/fs-id/**`, `Cargo.toml`, `Cargo.lock`, workflow + actions) |
| `.github/workflows/ci_cd_main.yml`                              | add `campaign` to the build matrix with the same `paths:` filter as preview; reuse the existing `wait-for-infrastructure` and registry-cleanup steps unchanged |
| `.github/actions/build-campaign/`                               | new composite action mirroring `.github/actions/build-platform/` (cargo build, docker buildx, push to Scaleway registry) |
| `apps/campaign/Dockerfile`                                      | new, mirroring `apps/platform/Dockerfile`: distroless nonroot final stage, UID 65532, same chown-init pattern compatible with the k8s manifests |
| `mise.toml` (local-dev `[env]` block)                           | add `INTERNAL_BEARER_PRIMARY = "dev-internal-bearer-not-a-secret"` (or similar fixed string); document in the comment that this is local-only and unrelated to deployed values                                                                                |
| `tmp/NewCampaignOnboarding/onboarding.jsx` (when porting)       | drop "Invent your own" input                                                                                                                                                                                                                                 |

## Verification

**Unit tests:**

- `apps/campaign/src/starter_content` snapshot tests: each `(template, locale)` pair compiles to a checked-in canonical Loro snapshot. PR review of template changes shows the snapshot diff per locale.
- `apps/campaign/src/starter_content` catalog parser: rejects unresolved slugs, malformed YAML, schema-invalid templates (via JSON Schema at deserialization, including the requirement that every `LocalizedString` carry an `en` entry). Does _not_ validate that node names correspond to real TipTap extensions; that's the integration test's job.
- **Canonicalization round-trip** (forward-compat for the upgrade flow): for every `(template, locale)` pair, compile to a `LoroDoc`, materialize back, recompute canonical hashes. Assert the recomputed `(structure_hash, content_hash)` matches the build-time table.
- `cargo build` fails if any v0 template fails to parse.

**Integration tests:**

- `apps/campaign` `/initialize` happy-path test: `POST /campaign/<id>/initialize` with a valid payload (game_system, content_locale, name, tagline, template_slugs, wizard_completed_at). Assert `campaign_metadata` rows are set, one Thing per slug exists with `is_template = true`, `seeded_locale` matches, `seeded_structure_hash` and `seeded_content_hash` match the build-time table, ToC has one new entry per slug, `wizard_completed_at` is set, and the platform receives one metadata-mirror POST.
- `apps/campaign` `/initialize` atomicity test: inject a per-slug compile failure on the second of three template_slugs; assert the transaction rolls back — no `campaign_metadata` writes, no Things, no ToC entries, `wizard_completed_at` remains NULL — and the call returns a structured `400`.
- `apps/campaign` `/initialize` precondition test: call `/initialize` twice on the same campaign with the same payload; assert the second call returns `409` and no state changes.
- **TipTap render test (the schema-authority drift catcher):** for every template under `content/templates/`, compile via `starter_content`, mount the resulting Loro doc in a real TipTap editor (using the existing `loro-prosemirror` test harness), assert no node falls back to "unknown."
- `apps/campaign` init idempotency test: call `/internal/init { campaign_id, owner_user_id }` twice with the same id; assert the second call no-ops and returns 200; only one `campaign_metadata` row exists.
- `apps/platform` create-call test: `POST /api/campaigns` with a stub shard. Verify a `create_attempts` row is written, the shard is called, the routing row is inserted, and `{ campaign_id }` is returned.
- `apps/platform` retry idempotency test: re-POST `/api/campaigns` with the same idempotency token after a simulated mid-flight crash; verify the same `campaign_id` is returned and no duplicate routing row exists.
- `apps/platform` failure-mode test: shard returns 5xx → platform returns 5xx, no routing row written (but `create_attempts` row may exist, harmless).

**Defense layers (cluster):**

- **No Ingress exposes `/internal/*`.** Enforced via a conftest policy invoked from `mise run lint:k8s` (the existing k8s-YAML lint task already runs kubeconform; the policy check appends to that same task). The `no-internal-ingress` rule denies any `Ingress` or Traefik `IngressRoute` whose path matchers contain `/internal`. Policy install (mise pin), the policy directory layout, and the fixture-test harness are the lint-infra plan's concern; this plan assumes that capability is available and asserts only that the rule exists and runs against every manifest under `infra/k8s/`.
- **NetworkPolicy denies the unhappy path.** Spin up a debug pod in a preview namespace with no `app` label: `kubectl run debug --image=alpine -n preview-pr-<N> -- sleep infinity; kubectl exec -it -n preview-pr-<N> debug -- nc -zv platform 3000` times out. From the campaign pod in the same namespace: same command succeeds. From a pod in another namespace (e.g., `kube-system`'s `coredns`): times out, confirming cross-namespace deny. The same three probes run against the prod `default` namespace using the `app=platform` and `app=campaign` pod selectors immediately after the prod NetworkPolicy apply; see §Internal-API defense layers § "What is deferred" for the rollout sequence.
- **Bearer absent → 401.** `kubectl exec -it -n preview-pr-<N> <campaign-pod> -- curl -X POST http://platform:3000/internal/campaigns/abc/metadata -d '{}'` returns 401. With the wrong bearer in the header: 401. With the correct bearer: 200 (or 4xx-on-payload-error, both prove the middleware accepted the bearer).
- **Bearer rotation handshake.** Set `INTERNAL_BEARER_SECONDARY` to a new value via the templated Secret; redeploy; assert calls from both senders (using the still-active primary) succeed. Swap primary/secondary in SM; redeploy; assert calls succeed (now using the new primary, with the old as secondary). Remove the secondary; redeploy; assert calls succeed (old value fully retired). The same exercise on a single pod is sufficient for v0; multi-replica rotation is dual-node concern.
- **Local-dev bearer works without Scaleway credentials.** `mise run dev` on a fresh checkout with no `scw` config; the wizard's Seal call succeeds end-to-end, proving the platform-to-campaign mirror's bearer is sourced from `mise.toml`, not from Scaleway SM, in dev mode.

**End-to-end (manual):**

- `mise run dev`, navigate to the web hub, click "create campaign," verify the SPA redirects into the new campaign with a wizard overlay showing and a campaign-level WebSocket open in the browser's dev tools.
- Walk the wizard: pick D&D 5e, pick a locale, pick templates, name the campaign. Toggle template selections on/off and confirm no server writes happen until Seal.
- Click Seal; verify the overlay dismisses, `campaign_metadata` is populated, the picked templates appear in the ToC as Things with `is_template = true`, and the hub reflects the new name/tagline/system.
- Refresh the campaign URL; verify the wizard does not reappear.
- Abandonment path: create a second campaign, walk halfway through the wizard, close the tab. Re-open the campaign URL; verify the wizard re-renders fresh (no pre-fill, no templates) and the campaign on the server is still in its just-created state.
- Repeat for Blades, Mothership, freeform.

**Lint / typecheck:**

- `mise run typecheck`, `mise run lint`, `mise run test` all pass.
