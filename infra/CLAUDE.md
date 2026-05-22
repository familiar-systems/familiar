# CLAUDE.md -- infra

## What This Is

Infrastructure for familiar.systems: Pulumi Python project (`pulumi-cloud/`), Kustomize overlays (`k8s/`), and CI workflows. Targets Hetzner Cloud + Scaleway Container Registry + Scaleway Secrets Manager. Pulumi state is stored in Scaleway Object Storage.

Single deployment target: **k3s cluster** serving `familiar.systems` + `app.familiar.systems` (production) and `preview.familiar.systems` + `app.preview.familiar.systems` (PR previews), plus the legacy `loreweaver.no` apex set until it retires. The two-apex layout (marketing vs app) is documented in [Deployment Architecture §URL routing](../docs/plans/2026-03-30-deployment-architecture.md#url-routing).

## Key Files

| File                   | Purpose                                                                                                                   |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `__main__.py`          | Pulumi entrypoint. Wires together modules, declares exports.                                                              |
| `config.py`            | Shared constants: `LOCATION`, `SERVER_TYPE`, `IMAGE`, `LABELS`, `config` object.                                          |
| `cloud.py`             | Shared Hetzner resources (floating IP, SSH keys, firewall) + Scaleway resources (registry, secrets).                      |
| `object_storage.py`    | Bucket policies + lifecycle + versioning on Hetzner Object Storage, applied via the `pulumi-minio` provider. Buckets themselves are pre-created in `bootstrap-object-storage.sh` and adopted via `import_=`; see the "Bucket existence" section below. |
| `k3s_cluster.py`       | `K3sCluster` ComponentResource: provisions k3s server with automated kubeconfig extraction via `pulumi-command`.          |
| `k8s.py`               | Kubernetes resources on the k3s cluster: cert-manager, webhook-bunny, ClusterIssuer, TLS cert, site deployment + ingress. |
| `Pulumi.yaml`          | Project config. Runtime is Python via uv toolchain.                                                                       |
| `Pulumi.prod.yaml`     | Stack config for `prod`. Contains encrypted secrets + SSH public keys.                                                    |
| `pyproject.toml`       | Python deps: pulumi, pulumi-hcloud, pulumi-command, pulumi-kubernetes, pulumiverse-scaleway.                              |
| `scripts/bootstrap.sh` | One-time setup: creates Scaleway bucket + passphrase secret.                                                              |
| `scripts/bootstrap-object-storage.sh` | Operator-bootstrapped Hetzner Object Storage state: prompts for the five console-generated S3 credential pairs and writes them as JSON to Scaleway SM, then creates the two buckets via AWS CLI. Idempotent. |
| `scripts/setup.sh`     | Per-machine setup: generates `.envrc` from existing Scaleway resources.                                                   |
| `scripts/nuke-k8s.sh`  | Emergency recovery: removes k8s resources from Pulumi state (and optionally wipes k3s).                                   |

## Architecture

### k3s (k3s_cluster.py + k8s.py)

The **Floating IP** (`cloud.py`) is the public entry point: DNS A records for every apex served by the cluster (the four familiar.systems apexes plus the legacy loreweaver.no apexes) point here. It is a top-level resource, not owned by any cluster. The IP is passed into `K3sCluster` as an input.

`K3sCluster` ComponentResource encapsulates:

- Floating IP assignment (binds the external IP to the server)
- Volume (10GB, `/data/k3s`, `/data/campaigns`, `/data/preview`)
- Server with k3s cloud-init (which auto-applies `pulumi-admin` SA + RBAC + token-Secret manifest at first boot)

`k8s.py` declares all Kubernetes resources via a `kubernetes.Provider` whose kubeconfig is constructed in `__main__.py` from byte-stable SM inputs (see "Auth model" below):

- cert-manager (Jetstack Helm chart, v1.17.2)
- cert-manager-webhook-bunny (DNS-01 for bunny.net)
- ClusterIssuer (prod + staging), Certificate covering the aggregated `PRODUCTION_DOMAINS` + `PREVIEW_DOMAINS` from `config.py` (marketing + app apexes for each environment; the cert is not a wildcard - SAN list is the exact apex set)
- Site Deployment + Service + Ingress bound to marketing apexes only (`MARKETING_*_DOMAINS` in `config.py`); the SPA + platform + campaign bind to the app apexes separately

### Provider cascade hazard

Pulumi manages both the Hetzner server and the k8s workloads running on it. The k8s Provider's `kubeconfig` field is `replaceOnChanges`, so any change to it cascades through every k8s resource parented to the provider (delete-and-recreate, which momentarily breaks cert-manager, the TLS cert, and the site ingress).

**Two fronts of mitigation:**

- **`ignoreChanges: ["user_data"]` on the server resource** (`k3s_cluster.py`) prevents cloud-init edits from replacing the server. Cloud-init only runs at first boot anyway, so changes to it are meaningless on an existing server. **Note:** if you DO need to apply cloud-init changes (e.g., updating the auto-applied SA manifest), you'll have to replace the server, which is a planned-downtime event with cascade implications.
- **The k8s Provider's `kubeconfig` is built from byte-stable SM inputs**, not from an SSH-extracted Output that can change. See "Auth model" below. This means routine `pulumi up` calls (image bumps, helm upgrades, new manifests) cannot trigger the cascade as a side effect - only deliberate SA token rotation or cluster CA rotation can, and both are planned events.
- **`scripts/nuke-k8s.sh` for recovery** when the cascade happens anyway (e.g. intentional server resize). It removes k8s resources from Pulumi state so `pulumi up` can recreate them from scratch.

**If you need to intentionally replace the server** (resize, OS upgrade, etc.):

1. Run `./scripts/nuke-k8s.sh --wipe-k3s` (wipes k3s datastore on the Volume, cleans Pulumi state)
2. Temporarily remove `ignore_changes=["user_data"]` if cloud-init changes are needed
3. `pulumi up` (creates new server, reinstalls k3s, recreates all k8s resources)
4. Restore `ignore_changes=["user_data"]`

k3s state on the Volume (`/data/k3s`) persists across server replacements. A new server will inherit corrupted k3s state unless `/data/k3s` is wiped first. Campaign data (`/data/campaigns`, `/data/preview`) is unaffected.

### Object Storage (object_storage.py)

Two Hetzner Object Storage buckets in `hel1`, generic per-env namespaces:

- `familiar-systems-prod` — production data. Each campaign gets a prefix at `campaigns/<id>/`, with the libSQL file at `campaigns/<id>/campaign.db`. Per-campaign sidecars (exports, pre-migration snapshots, audit dumps) colocate under the same prefix as they're added. GDPR deletion is `aws s3 rm s3://.../campaigns/<id>/ --recursive`. Future workloads get their own top-level prefixes (`platform-backups/`, ...).
- `familiar-systems-preview` — preview-environment data. Per-PR campaign databases at `campaigns/pr-<N>/<id>/campaign.db`.

Local dev does not touch the buckets — campaign-server's `CampaignStore` has a local-filesystem implementation for that mode.

#### Why pulumi-minio (not pulumi-hcloud)

`pulumi_hcloud` exposes only Hetzner Cloud's older `StorageBox` resource, not Object Storage. Per Hetzner's own docs, the recommended IaC path is the [aminueza/minio Terraform provider](https://docs.hetzner.com/storage/object-storage/getting-started/creating-a-bucket-minio-terraform); the Pulumi-bridged equivalent (`pulumi-minio`, currently pinned to `0.16.x`) targets the same S3-compatible endpoint and exposes `S3Bucket`, `S3BucketPolicy`, `S3BucketVersioning`, and `IlmPolicy` (lifecycle rules).

Configured against `https://hel1.your-objectstorage.com`. The provider authenticates with the `familiar-systems-pulumi-key` credential pair, read at apply time from Scaleway SM.

#### Bucket existence: created in bootstrap, adopted by Pulumi

The two buckets themselves are **not** created by `pulumi up`. They're created by `scripts/bootstrap-object-storage.sh` (against Hetzner's S3 endpoint via the AWS CLI, authenticated with the `pulumi-key` credentials), and the `minio.S3Bucket` resources in `object_storage.py` carry `pulumi.ResourceOptions(import_=...)` so Pulumi adopts the pre-created buckets on first apply.

This works around an upstream bug. `pulumi-minio 0.16.9` (latest) pins `aminueza/terraform-provider-minio v1.20.1` — a 2023-11-08 tag that put the v1 line into maintenance mode the same day. The v1.20.1 bucket-Create flow does an immediate read-after-create that races with Hetzner's eventually-consistent bucket index: `MakeBucket` succeeds on Hetzner, the subsequent `BucketExists` polls before the index has propagated, the provider clears the resource ID and returns `(nil state, nil error)`, and the Pulumi bridge surfaces this as `expected non-nil error with nil state during Create`. The race was fixed in aminueza v3.28.1 (March 2026), but the pulumi-minio bridge has never moved off v1.x. See [pulumi-minio#754](https://github.com/pulumi/pulumi-minio/issues/754) and [aminueza/terraform-provider-minio#839](https://github.com/aminueza/terraform-provider-minio/issues/839).

`S3BucketPolicy`, `S3BucketVersioning`, and `IlmPolicy` remain Pulumi-managed and target the already-propagated buckets, so they don't hit the race either.

**Edge case — `pulumi destroy` + re-apply:** `pulumi destroy` deletes the bucket from Hetzner *and* removes it from Pulumi state. The next `pulumi up` would attempt to import a bucket that no longer exists and fail. Fix: comment out the `import_=` line on each `S3Bucket` resource for one apply (and re-run `./scripts/bootstrap-object-storage.sh` first so the bucket exists before Pulumi tries to adopt it — or remove `import_=` entirely and let Pulumi hit the eventual-consistency race once). For production, destroy is rare-to-never; the one-line code edit is acceptable when it happens.

#### Credential model (five pairs, all operator-bootstrapped)

Hetzner has **no public API** for creating S3 credentials — they can only be generated through the Hetzner Console (Security → S3 Credentials → Generate). Five pairs are bootstrapped via `scripts/bootstrap-object-storage.sh`, which prompts for each pair and writes JSON (`{"access_key_id", "secret_access_key"}`) to a Scaleway SM secret:

| Credential | Used by | Prod bucket access | Preview bucket access |
|---|---|---|---|
| `familiar-systems-prod-key` | campaign-server (prod) | read+write | denied |
| `familiar-systems-preview-key` | campaign-server (preview) | denied | read+write |
| `familiar-systems-preview-seed-key` | CI (`deploy-preview.yml`) | read-only (Get + List) | write-only (PutObject) |
| `familiar-systems-pulumi-key` | Pulumi (configures `pulumi-minio` provider) | full | full |
| `familiar-systems-operator-key` | Human ad-hoc data access (Cyberduck, AWS CLI, `mc`) | full | full |

Access enforcement is per-bucket policy. Each bucket carries a two-statement policy: (1) deny anyone whose access-key isn't in the allow list; (2) further restrict the seed key to read-only on prod / PutObject-only on preview. A leaked seed key cannot corrupt prod or exfiltrate preview content.

The operator key is the **escape hatch for direct data access**: pulling a campaign DB for offline inspection, listing what's in a bucket, copying between prefixes for an ad-hoc migration. It's deliberately separate from the pulumi-key so that "do an ops task" doesn't require touching the credential Pulumi itself authenticates with, and it rotates without restarts because no pod consumes it. Fetch via:

```bash
SM_ID=$(scw secret secret list name=familiar-systems-operator-key region=fr-par -o json | jq -r '.[0].id')
eval "$(scw secret version access "$SM_ID" revision=latest region=fr-par -o json \
  | jq -r '.data' | base64 -d \
  | jq -r '"export AWS_ACCESS_KEY_ID=\(.access_key_id) AWS_SECRET_ACCESS_KEY=\(.secret_access_key)"')"

aws s3 ls --endpoint-url https://hel1.your-objectstorage.com s3://familiar-systems-prod/campaigns/
```

Or Cyberduck: profile = `S3 (HTTPS)`, server = `hel1.your-objectstorage.com`, paste the access-key ID and secret.

**Hetzner's bucket-policy ARNs are cosmetic AWS-SDK wrappers**, not references to any IAM principal: `arn:aws:iam:::user/p<project_id>:<access_key_id>`. The `<project_id>` is the numeric Hetzner Cloud project ID, set via `pulumi config set hetzner-project-id <number>` (non-secret; find it under the project menu in the Hetzner Console).

#### Lockout protection

The `pulumi-key` access-key ID **must remain in every bucket policy's allow list**. If it's removed, Pulumi loses the ability to update those policies (Hetzner enforces bucket policies against project-scoped credentials including the one Pulumi is using). The construction in `object_storage.py` always includes it. Recovery if it does happen: project owner regenerates a new credential in the Console, edits the policy out-of-band via `mc admin policy set` or `aws s3api put-bucket-policy` with project-owner credentials.

#### Console UI caveat

Hetzner's Console bucket browser uses its own internal credentials, which are not in any of our four allow lists. So the Console's "Browse bucket" view returns empty for these buckets — that's expected, not a bug. Use Cyberduck or `aws s3 ls --endpoint-url ...` for visual exploration.

#### Lifecycle and versioning

- Both buckets: orphaned multipart parts accumulate over time because `pulumi-minio`'s `IlmPolicy` doesn't expose `AbortIncompleteMultipartUpload`. Hetzner has no per-request charges so the cost leak is GBs of storage only — negligible for campaign DB sizes. If it ever matters, add a periodic `mc ilm rule add` step.
- `familiar-systems-prod`: versioning **enabled** + `noncurrent_version_expiration_days: 7`. Soft-delete safety net — overwrites and deletes are reversible within 7 days, then auto-pruned.
- `familiar-systems-preview`: `expiration: "7d"` bucket-wide. S3 lifecycle uses last-modified semantics, so an active PR with writeback-every-30s keeps its DB perpetually fresh; the 7-day clock effectively only starts when writebacks stop (PR closed, namespace torn down). PRs that sit idle for >7 days lose their preview data and re-seed from prod on next access.

#### Bootstrap and rotation

Bootstrap (one-time, takes ~2 minutes once you have five credential pairs ready):

```bash
# 1. In Hetzner Console, generate five S3 credential pairs:
#    Security -> S3 Credentials -> Generate credentials
#    (Repeat five times. Name each one to match the table above.)
# 2. Run the bootstrap script (prompts for each pair, then creates both
#    buckets on Hetzner using the pulumi-key credentials):
./scripts/bootstrap-object-storage.sh
# 3. Set the Hetzner project ID:
pulumi config set hetzner-project-id <numeric-id-from-console>
# 4. Apply (adopts the pre-created buckets into Pulumi state and creates
#    bucket policies, versioning, lifecycle):
pulumi up
```

Rotation (option A — planned-maintenance, single key per role):

1. In Hetzner Console, generate a replacement S3 credential pair for the role you're rotating (e.g. a new `familiar-systems-prod-key`). Delete the old one in the Console.
2. Re-run `./scripts/bootstrap-object-storage.sh` and paste the new pair when prompted for that role (skip the others with Ctrl-D).
3. `pulumi up` — the access-key ID in the bucket policy's allow list updates to match the new credential.
4. `kubectl rollout restart deployment/campaign-server` if rotating prod-key or preview-key (so pods pick up the new SM value via their k8s Secret). Seed-key rotation needs no pod restart (next GHA preview-deploy run picks it up). Pulumi-key rotation needs nothing else — the provider re-authenticates on the next `pulumi up`.

There's a brief window between step 3 and step 4 where running pods still hold the old secret-key but the bucket policy has already swapped to the new ID, so their S3 requests get 403. For zero-downtime rotation, graduate to a two-key design (active + standby in the allow list, atomic config swap).

#### State recovery

Pulumi state lives in Scaleway, independent of Hetzner. If state is lost:

1. Project owner can always list buckets and credentials via the Hetzner Cloud API (bucket policies enforce on the S3 endpoint, not on project management).
2. Bucket names are deterministic. Access-key IDs are recoverable from Scaleway SM (the four bootstrapped secrets) or from the Hetzner Console.
3. `pulumi import` each resource by its identifier.

## Reference Documentation

**MANDATORY: Read the relevant docs before writing or modifying any resource. Do not guess at API shapes, field names, or encoding requirements.**

### Pulumi Providers

- pulumiverse-scaleway registry: https://www.pulumi.com/registry/packages/scaleway/api-docs/
    - Secret: https://www.pulumi.com/registry/packages/scaleway/api-docs/secret/
    - SecretVersion: https://www.pulumi.com/registry/packages/scaleway/api-docs/secretversion/
    - RegistryNamespace: https://www.pulumi.com/registry/packages/scaleway/api-docs/registrynamespace/
- pulumi-kubernetes: https://www.pulumi.com/registry/packages/kubernetes/api-docs/
- pulumi-hcloud: https://www.pulumi.com/registry/packages/hcloud/api-docs/
- pulumi-minio: https://www.pulumi.com/registry/packages/minio/api-docs/
    - S3Bucket: https://www.pulumi.com/registry/packages/minio/api-docs/s3bucket/
    - S3BucketPolicy: https://www.pulumi.com/registry/packages/minio/api-docs/s3bucketpolicy/
    - S3BucketVersioning: https://www.pulumi.com/registry/packages/minio/api-docs/s3bucketversioning/
    - IlmPolicy: https://www.pulumi.com/registry/packages/minio/api-docs/ilmpolicy/

### Hetzner Object Storage

- Overview: https://docs.hetzner.com/storage/object-storage/overview
- Per-key bucket-policy FAQ: https://docs.hetzner.com/storage/object-storage/faq/s3-credentials#how-do-i-restrict-access-per-key
- Lifecycle rules: https://docs.hetzner.com/storage/object-storage/howto-protect-objects/manage-lifecycle
- Versioning: https://docs.hetzner.com/storage/object-storage/howto-protect-objects/protect-versioning
- Credentials are Console-only (no API): https://docs.hetzner.com/storage/object-storage/getting-started/generating-s3-keys

### Scaleway

- CLI reference (`scw`): https://github.com/scaleway/scaleway-cli/blob/master/docs/commands/
    - `scw registry`: https://github.com/scaleway/scaleway-cli/blob/master/docs/commands/registry.md
    - `scw secret`: https://github.com/scaleway/scaleway-cli/blob/master/docs/commands/secret.md
    - CLI `data=` arg for secrets handles base64 internally. Never manually encode before passing.
- Container Registry docs: https://www.scaleway.com/en/docs/containers/container-registry/
- Secrets Manager docs: https://www.scaleway.com/en/docs/identity-and-access-management/secret-manager/

### GitHub Actions

- scaleway/action-scw: https://github.com/scaleway/action-scw (accepts `version`, `repo-token` inputs)
- docker/build-push-action: https://github.com/docker/build-push-action

### Kubernetes / Helm Charts

- cert-manager CRDs: https://cert-manager.io/docs/reference/api-docs/
- cert-manager-webhook-bunny (source + config schema): https://github.com/davidhidvegi/cert-manager-webhook-bunny
- cert-manager-webhook-bunny Helm values: `helm show values cert-manager-webhook-bunny --repo https://davidhidvegi.github.io/cert-manager-webhook-bunny/charts/`
- The webhook's solver `config` block is opaque to cert-manager -- each webhook defines its own schema. Do NOT assume cert-manager conventions (e.g. `apiKeySecretRef`) apply to webhook configs.

## Credentials Architecture

- **Scaleway Secrets Manager** is the single source of truth for all application secrets. Never store secret values in Pulumi config or GitHub Actions secrets.
- **Scaleway** is the control plane: stores Pulumi state (Object Storage), secrets (Secrets Manager), and container registry.
- **Hetzner** is the data plane: where infrastructure is provisioned.
- Scaleway credentials come from `~/.config/scw/config.yaml` (via `scw init`).
- `.envrc` (generated by `setup.sh`, gitignored) maps Scaleway creds to `AWS_*` env vars for Pulumi's S3 backend and exports `SCW_ACCESS_KEY`/`SCW_SECRET_KEY` for the Scaleway provider.
- GitHub Actions secrets: `SCW_ACCESS_KEY`, `SCW_SECRET_KEY`, `SCW_DEFAULT_ORGANIZATION_ID`, `SCW_DEFAULT_PROJECT_ID` (provider credentials only).

### How secrets flow

Two paths:

1. **Pulumi** reads SM at apply time via `config.read_secret(name)` (wraps `pulumiverse_scaleway.secrets.get_version_output()`). Used for infrastructure-level secrets (kubeconfig, cluster CA).
2. **External Secrets Operator (ESO)** runs in-cluster and syncs SM secrets into k8s Secrets. All application secrets (bearer tokens, registry pull credentials, DNS API keys) flow through ESO. No application secrets transit GitHub Actions.

GHA reads `k3s-kubeconfig` via `.github/actions/fetch-scw-secret/` for kubectl access only.

### Required Scaleway SM Secrets

| Secret Name                            | Purpose                                                                      |
| -------------------------------------- | ---------------------------------------------------------------------------- |
| `loreweaver-deploy-ssh-key`            | Break-glass SSH private key for direct server access (rare, manual ops)      |
| `bunny-api-key`                        | bunny.net API key for DNS-01 ACME. Synced into `cert-manager` namespace by ESO (`infra/k8s/eso/bunny-api-key.yaml`, applied by `bootstrap-helm.sh`). |
| `k3s-kubeconfig`                       | Token-based kubeconfig for GHA deploys + local kubectl (operator-managed)    |
| `k3s-pulumi-admin-token`               | `pulumi-admin` ServiceAccount bearer token (cluster-admin, operator-managed) |
| `k3s-cluster-ca`                       | k3s cluster CA cert, base64 PEM (operator-managed, rarely rotates)           |
| `familiar-systems-prod-key`            | Hetzner Object Storage credential pair for campaign-server prod (JSON: `{"access_key_id", "secret_access_key"}`) |
| `familiar-systems-preview-key`         | Hetzner Object Storage credential pair for campaign-server preview (same JSON shape) |
| `familiar-systems-preview-seed-key`    | Hetzner Object Storage credential pair for the CI seed step: read prod, write preview only |
| `familiar-systems-pulumi-key`          | Hetzner Object Storage admin credential pair used by the `pulumi-minio` provider for bucket management |
| `familiar-systems-operator-key`        | Hetzner Object Storage credential pair for human ad-hoc data access (Cyberduck, AWS CLI). Full access to both buckets; not bound to any pod. |
| `eso-scaleway-credential`              | ESO's Scaleway API key for SM read access (JSON: `{"access_key", "secret_key"}`). Pulumi-managed end-to-end (IAM app + policy + key + SM version in `cloud.py`). Read by `bootstrap-helm.sh` to seed the initial k8s Secret. |
| `registry-pull-credential`             | Scaleway registry pull credential (JSON: `{"access_key", "secret_key"}`). Pulumi writes `registry_pull_api_key` to SM. ESO reads this and constructs the `kubernetes.io/dockerconfigjson` secret via template. |
| `internal-bearer-prod`                 | Shared bearer for prod platform <-> campaign `/internal/*`. SM container is Pulumi-owned; value is operator-managed. Synced into k8s by ESO's `ExternalSecret` CRD in the prod kustomize overlay. |
| `internal-bearer-preview`              | Same role, scoped to every preview namespace. Identical value across all open PRs (preview is shared trust). Synced by ESO's `ExternalSecret` CRD in the preview kustomize overlay. |

The `k3s-*` secrets are populated by `scripts/bootstrap-pulumi-admin.sh`, not by Pulumi. Pulumi reads them at deploy time via `config.read_secret(name)`. Re-running the bootstrap script is safe (idempotent) and writes new SM versions for all three.

The `familiar-systems-*-key` secrets are populated by `scripts/bootstrap-object-storage.sh`. The script also creates the SM containers (these five are not Pulumi-managed, mirroring how `pulumi-config-passphrase` is handled in `scripts/bootstrap.sh`). The five credentials themselves are generated by hand in the Hetzner Console -- there is no API for credential creation.

The `internal-bearer-*` SM containers are Pulumi-owned; their values are operator-managed via `scw secret version create`.

The `eso-scaleway-credential` and `registry-pull-credential` secrets are fully Pulumi-managed: Pulumi creates the IAM credentials and writes them to SM as `SecretVersion` resources. No operator intervention needed.

### Rotation: `internal-bearer-prod` / `internal-bearer-preview`

```bash
openssl rand -base64 32 | scw secret version create internal-bearer-prod data=- region=fr-par
```

ESO picks up the new SM version on its next `refreshInterval` (1h default) or on-demand:

```bash
kubectl annotate externalsecret internal-bearer force-sync=$(date +%s) --overwrite
```

Pods using `envFrom` don't auto-restart when the underlying Secret changes. After rotation, restart the consuming deployments:

```bash
kubectl rollout restart deployment/platform deployment/campaign
```

Preview rotation is the same shape against `internal-bearer-preview`. ESO syncs it into every active preview namespace on `refreshInterval`. Restart preview pods if needed.

### Registry pull credential (Pulumi-managed, ESO-synced)

Pulumi owns the IAM credential lifecycle end-to-end: `iam.Application` (`k3s-registry-puller`) + `iam.Policy` (scoped to `ContainerRegistryReadOnly` on the registry project) + `iam.ApiKey` (see `cloud.py`). Pulumi writes the API key to SM as `registry-pull-credential` (JSON with `access_key` + `secret_key`).

ESO reads this SM secret and constructs the `kubernetes.io/dockerconfigjson` k8s Secret via its template feature (`infra/k8s/overlays/prod/secrets/scaleway-registry.yaml` and the equivalent preview overlay).

**Rotation:** If the IAM key is replaced (e.g., `pulumi up` after deleting the old key), Pulumi writes a new SM version. ESO picks it up on the next `refreshInterval` and updates the k8s Secret in-place. No delete-and-recreate window.

## Auth model (k8s Provider)

The Pulumi `kubernetes.Provider` authenticates with a **static long-lived ServiceAccount bearer token** in a byte-stable kubeconfig. The kubeconfig is constructed in `__main__.py` from three SM-sourced inputs (floating IP, cluster CA, SA token), all of which are stable across `pulumi up` runs unless deliberately rotated. Pulumi's diff sees the same kubeconfig string every time, so the Provider is never replaced and the k8s resource graph never cascades.

**Why this matters:** The `kubernetes.Provider.kubeconfig` field is `replaceOnChanges`. Wiring it to any Pulumi-tracked Output that can change (e.g., the SSH-extracted `command.remote.Command.stdout` we used before) means any rotation event causes the provider to be replaced, which deletes-and-recreates every k8s resource parented to it (10+ resources including cert-manager, the wildcard Cert, the site Deployment/Service/Ingress, etc.). The static-SA-token pattern eliminates this entirely. **Do not regress to "convenience" patterns that wire credentials directly to the Provider.** See `~/.claude/projects/.../memory/feedback_iac_credential_decoupling.md` for the full pathology.

**Bootstrap flows:**

- **Fresh cluster:** Cloud-init drops a manifest into `/var/lib/rancher/k3s/server/manifests/pulumi-admin.yaml` (k3s's auto-apply directory), creating the ServiceAccount + ClusterRoleBinding + token-Secret at first boot. The operator then runs `scripts/bootstrap-pulumi-admin.sh` once to capture the populated token + CA from the cluster and write them to SM. After that, `pulumi up` works normally.
- **Existing cluster:** Run `scripts/bootstrap-pulumi-admin.sh` directly. It applies the manifest via the current kubeconfig, captures the token + CA, and writes them to SM. Same end state as the fresh-cluster path.

**Rotation:** The SA token has no expiry and is valid as long as the SA exists. To rotate, re-run the bootstrap script - it pushes a new SM version, and the next `pulumi up` picks it up. **This DOES trigger the cascade** (kubeconfig string changes), so plan accordingly with `letsencrypt-staging-dns` as the active issuer (see `k8s.py` constants). Cluster CA rotation is a separate planned-downtime event; the CA is valid for ~10 years by default.

`make rotate-certs` was deleted because it was the kind of "convenience" routine that triggered the very cascade this auth model exists to prevent. If you ever genuinely need to rotate the CA, do it as a planned event with explicit operator action.

## Domains

Production and preview domains are listed in `config.py` as `PRODUCTION_DOMAINS` and `PREVIEW_DOMAINS`. Both lists are imported by `k8s.py` to construct the wildcard Certificate's `dnsNames` and the site Ingress's `tls`/`rules` blocks. To add a domain:

1. Append the apex (e.g., `example.com`) to `PRODUCTION_DOMAINS`.
2. Append the preview prefix (e.g., `preview.example.com`) to `PREVIEW_DOMAINS`.
3. Confirm the bunny.net account managing `bunny-api-key` controls the DNS zone for the new domain (DNS-01 ACME requires this).
4. `pulumi up` - the cert re-issues with the new SANs.

The wildcard Certificate covers, for each preview domain `D`: `D` itself and `*.D`. So `pr-42.preview.example.com` is covered automatically.

When migrating away from a domain, remove it from both lists and `pulumi up`. The cert re-issues without the old SANs; old DNS records can be deleted afterward.

## Preview environments (PR previews)

Per-PR preview deployments live in namespaces named `preview-pr-${PR_NUMBER}`, one per open PR. They are **not** Pulumi-managed -- they're created and destroyed by two GitHub Actions workflows:

- `.github/workflows/ci_cd_preview.yml` -- builds the PR's images, fetches the `k3s-kubeconfig` SM secret (for kubectl access), and applies the kustomize overlay from `infra/k8s/overlays/preview/`. Secrets are `ExternalSecret` CRDs resolved by ESO in-cluster; no application secrets transit GHA. Runs on PR `opened`/`synchronize`.
- `.github/workflows/cleanup-preview.yml` -- deletes the namespace (which cascade-removes all resources inside, including ESO-managed secrets) and cleans up the registry image tag. Runs on PR `closed`.

The preview manifests live in `infra/k8s/overlays/preview/` as Kustomize overlays. Deployment patches and ingresses use `${VAR}` placeholders for per-PR values; secret manifests are static `ExternalSecret` CRDs (no envsubst). The templating variables are:

| Variable                                                       | Source                                                                                                                           | Example                                                                                                  |
| -------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `NAMESPACE`                                                    | workflow: `preview-pr-${PR_NUMBER}`                                                                                              | `preview-pr-42`                                                                                          |
| `PR_NUMBER`                                                    | workflow: the PR number; consumed by Ingress manifests to build `/pr-${N}` path prefixes and by the web build's `VITE_BASE_PATH` | `42`                                                                                                     |
| `SITE_IMAGE` / `WEB_IMAGE` / `PLATFORM_IMAGE` / `CAMPAIGN_IMAGE` | workflow: built image tags                                                                                                    | `rg.fr-par.scw.cloud/loreweaver/site:pr-42-abc1234`                                                     |
| `HANKO_API_URL_DEV`                                            | workflow env: Hanko tenant URL for preview                                                                                       | `https://auth.preview.familiar.systems`                                                                  |

**To edit preview behavior**, edit the YAML files directly -- the workflow only handles substitution and apply. **To validate changes**, run `mise run lint:k8s` (kubeconform).

**Why this split, rather than Pulumi-managing the preview resources:** PR previews are ephemeral (seconds of creation, minutes of lifetime) and per-PR (one namespace per open PR, potentially dozens at once). Pulumi's state model is designed for long-lived, named resources; spinning up a Pulumi stack per PR would be wildly over-engineered. GitHub Actions can apply raw YAML in ~2s per resource, which is the right tool. The separation is: **Pulumi for permanent cluster state** (cert-manager, the TLS cert, IAM credentials, SM secrets), **Kustomize-applied-by-CI for ephemeral per-PR resources**.

## Commands

```bash
# Always source .envrc first (direnv does this automatically)
source .envrc

pulumi preview        # Dry-run
pulumi up             # Apply
pulumi stack output k3s_floating_ip    # Get k3s IP for DNS
make kubeconfig                         # Fetch kubeconfig from SM and merge into ~/.kube/config

pulumi config set --secret <key> <value>  # Add encrypted config
```

## Logging

Kubelet log retention is configured in cloud-init (`k3s_cluster.py`): 10 files x 50 MiB per container. Logs live at `/var/log/pods/` on the node.

- **Humans:** `make logs` opens the Kubetail browser dashboard for log viewing.
- **Claude:** Use `stern` for log access -- it's CLI-native and requires no browser. Example: `stern . --context loreweaver-preview` tails all pods, `stern api --context loreweaver-preview` tails the API pod.

## Rules

- Never commit `.env`, `.envrc`, or any file containing raw credentials.
- All application secrets live in Scaleway Secrets Manager. Only provider credentials (e.g. `hcloud:token`) belong in Pulumi config.
- The `encryptionsalt` in `Pulumi.prod.yaml` is safe to commit -- it's not a secret.
- **Lint/format/check**: `mise run lint:infra && mise run format:infra && mise run typecheck:infra`.
