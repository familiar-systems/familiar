# CLAUDE.md -- infra

## What This Is

Infrastructure for familiar.systems: OpenTofu HCL project (`tofu/`), Kustomize overlays (`k8s/`), and CI workflows. Targets Hetzner Cloud + Scaleway Container Registry + Scaleway Secrets Manager + Hetzner Object Storage. OpenTofu state is stored in Scaleway Object Storage.

Single deployment target: **k3s cluster** serving `familiar.systems` + `app.familiar.systems` (production) and `preview.familiar.systems` + `app.preview.familiar.systems` (PR previews). The two-apex layout (marketing vs app) is documented in [Deployment Architecture](../docs/plans/2026-03-30-deployment-architecture.md).

## Key Files

| File                          | Purpose                                                                                                                      |
| ----------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `versions.tf`                 | Required providers, S3 backend (Scaleway), state encryption (PBKDF2 + AES-GCM).                                             |
| `providers.tf`                | Provider configs: hcloud (env), scaleway (scw CLI config), minio (env).                                                      |
| `variables.tf`                | Input variables: SSH keys, ACME email, Hetzner project ID, encryption passphrase.                                            |
| `hetzner_compute.tf`          | SSH keys, floating IP, firewall, volume, server (k3s cloud-init), assignments.                                               |
| `hetzner_s3.tf`               | MinIO provider (Hetzner S3 endpoint), buckets, policies, versioning, lifecycle.                                              |
| `scaleway_iam.tf`             | Container registry namespace, IAM applications + policies + API keys.                                                        |
| `scaleway_secrets.tf`         | SM secret containers (operator-filled) + SM secret versions (OpenTofu-written).                                              |
| `outputs.tf`                  | Stack exports consumed by `bootstrap-k8s-admin.sh` (floating IP, kubeconfig secret ID).                                      |
| `terraform.tfvars`                 | Non-secret variable values for the prod environment.                                                                         |
| `scripts/bootstrap.sh`        | One-time: creates state bucket + passphrase + hcloud token SM secrets.                                                       |
| `scripts/setup.sh`            | Per-machine: generates `.envrc` from scw CLI config + SM secrets.                                                            |
| `scripts/bootstrap-k8s-admin.sh` | Creates k8s SA + pushes token/CA/kubeconfig to SM.                                                                        |
| `scripts/bootstrap-object-storage.sh` | Operator-bootstrapped Hetzner S3 credentials + bucket creation.                                                       |
| `scripts/rotate_scw_key.py`   | Rotates Scaleway IAM API keys (click CLI, full state machine).                                                               |

## Architecture

### IaC Scope

OpenTofu manages three provider domains:

1. **Hetzner Cloud** (via `hetznercloud/hcloud`): SSH keys, floating IP, firewall, volume, server, assignments
2. **Scaleway** (via `scaleway/scaleway`): container registry, IAM (apps, policies, API keys), Secrets Manager containers and versions
3. **Hetzner Object Storage** (via `aminueza/minio`): S3 buckets, bucket policies, versioning, lifecycle rules

**Kubernetes resources are NOT managed by OpenTofu.** Helm charts (cert-manager, ESO, webhook-bunny) are managed by `k8s/scripts/bootstrap-helm.sh`. Application deployments are managed by Kustomize overlays applied by GitHub Actions.

### k3s Cluster (hetzner_compute.tf)

The **Floating IP** is the public entry point: DNS A records for all apexes point here. It is a top-level resource passed into the server via cloud-init.

The server provisions k3s via cloud-init, which:
- Mounts the 10GB volume at `/data` (k3s state, campaigns, preview data)
- Installs k3s with `--tls-san <floating-ip>` and `--data-dir /data/k3s`
- Auto-applies a `pulumi-admin` SA (legacy name, see [Legacy Naming](#legacy-naming)) + cluster-admin binding + token Secret manifest

**`lifecycle { ignore_changes = [user_data] }`** on the server prevents cloud-init changes from replacing the server. Cloud-init only runs at first boot.

### Object Storage (hetzner_s3.tf)

Two Hetzner Object Storage buckets in `hel1`:
- `familiar-systems-prod`: versioning enabled, 7-day noncurrent retention
- `familiar-systems-preview`: 7-day object expiration

#### Credential model (five pairs, all operator-bootstrapped)

Hetzner has **no public API** for creating S3 credentials. Five pairs are bootstrapped via `scripts/bootstrap-object-storage.sh`:

| Credential | Used by | Prod access | Preview access |
|---|---|---|---|
| `familiar-systems-prod-key` | campaign-server (prod) | read+write | denied |
| `familiar-systems-preview-key` | campaign-server (preview) | denied | read+write |
| `familiar-systems-preview-seed-key` | CI seed step | read-only | write-only |
| `familiar-systems-pulumi-key` (legacy name) | OpenTofu minio provider | full | full |
| `familiar-systems-operator-key` | Human ad-hoc access | full | full |

#### Lockout protection

The `familiar-systems-pulumi-key` (legacy name) access-key ID **must remain in every bucket policy's allow list**. If removed, OpenTofu loses the ability to update those policies.

### Provider Authentication

| Provider | Auth source |
|---|---|
| `hcloud` | Ephemeral `scaleway_secret_version` (reads `hcloud-api-token` from SM at runtime) |
| `scaleway` | `~/.config/scw/config.yaml` (scw CLI config, no env vars needed) |
| `minio` | Ephemeral `scaleway_secret_version` (reads `familiar-systems-pulumi-key` (legacy name) from SM at runtime) |
| S3 backend | `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` (Scaleway creds mapped, via `setup.sh`) |

### State Backend

Scaleway Object Storage bucket `familiar-systems-tofu-state` in `fr-par`, accessed via the S3-compatible endpoint. State encrypted with PBKDF2 + AES-GCM (passphrase in SM: `tofu-config-passphrase`). No state locking (Scaleway Object Storage does not support it).

## Credentials Architecture

- **Scaleway Secrets Manager** is the single source of truth for all secrets.
- **Scaleway** is the control plane: stores OpenTofu state, secrets, and container registry.
- **Hetzner** is the data plane: where infrastructure is provisioned.
- `.envrc` (generated by `setup.sh`, gitignored) exports env vars for the S3 backend and state encryption. The hcloud and minio providers self-serve credentials from SM via ephemeral resources (see `providers.tf`).
- GitHub Actions secrets: `SCW_ACCESS_KEY`, `SCW_SECRET_KEY`, `SCW_DEFAULT_ORGANIZATION_ID`, `SCW_DEFAULT_PROJECT_ID` (provider credentials only).

### How secrets flow

1. **OpenTofu** reads SM at runtime via ephemeral `scaleway_secret_version` resources (for hcloud and minio provider auth, never persisted to state) and `data "scaleway_secret_version"` data sources (for S3 credential access-key IDs used in bucket policies).
2. **External Secrets Operator (ESO)** runs in-cluster and syncs SM secrets into k8s Secrets. All application secrets flow through ESO.

### Required Scaleway SM Secrets

| Secret Name | Purpose | Managed by |
|---|---|---|
| `loreweaver-deploy-ssh-key` | Break-glass SSH private key | OpenTofu (container) + operator (value) |
| `bunny-api-key` | bunny.net API key for DNS-01 ACME | OpenTofu (container) + operator (value) |
| `k3s-kubeconfig` | Token-based kubeconfig for GHA + kubectl | OpenTofu (container) + bootstrap-k8s-admin.sh (value) |
| `k3s-pulumi-admin-token` (legacy name) | SA bearer token (cluster-admin) | bootstrap-k8s-admin.sh |
| `k3s-cluster-ca` | Cluster CA cert (base64 PEM) | bootstrap-k8s-admin.sh |
| `hcloud-api-token` | Hetzner Cloud API token | OpenTofu (container) + operator (value) |
| `internal-bearer-prod` | Shared bearer for prod platform/campaign | OpenTofu (container) + operator (value) |
| `internal-bearer-preview` | Shared bearer for preview platform/campaign | OpenTofu (container) + operator (value) |
| `eso-scaleway-credential` | ESO's Scaleway API key (JSON) | OpenTofu (fully managed) |
| `registry-pull-credential` | Registry pull credential (JSON) | OpenTofu (fully managed) |
| `tofu-config-passphrase` | State encryption passphrase | bootstrap.sh |
| `familiar-systems-*-key` (5) | Hetzner S3 credentials | bootstrap-object-storage.sh |

## Commands

```bash
# Setup
./scripts/bootstrap.sh            # One-time: state bucket + SM secrets
./scripts/setup.sh                # Per-machine: generate .envrc
direnv allow
tofu init

# Day-to-day
tofu plan   # Dry-run (or: mise run plan:infra)
tofu apply  # Apply

# Outputs
tofu output k3s_floating_ip       # Get k3s IP for DNS
```

## Bootstrap Flows

### Fresh environment
```bash
scw init                                   # Scaleway credentials
./scripts/bootstrap.sh                     # State bucket + passphrase + hcloud token
# Fill hcloud-api-token: scw secret version create hcloud-api-token data=<token> region=fr-par
direnv allow
tofu init
./scripts/bootstrap-object-storage.sh      # S3 credentials + buckets
tofu apply           # Create all resources
./scripts/bootstrap-k8s-admin.sh           # k8s SA + kubeconfig to SM
```

### Existing environment (new machine)
```bash
scw init
./scripts/setup.sh
direnv allow
tofu init
```

## Preview Environments

Per-PR preview deployments are **not** OpenTofu-managed. They are created and destroyed by GitHub Actions workflows using Kustomize overlays. See `k8s/overlays/preview/` and `.github/workflows/ci_cd_preview.yml`.

## Rules

- Never commit `.envrc` or any file containing raw credentials.
- All application secrets live in Scaleway Secrets Manager.
- **Lint/format/check**: `mise run lint:infra && mise run format:infra`.
- Never run `tofu apply` without reviewing the plan first.

## Legacy Naming

Two operational names retain their original "pulumi" prefix from the Pulumi-to-OpenTofu migration ([#115](https://github.com/familiar-systems/familiar/issues/115), [#116](https://github.com/familiar-systems/familiar/issues/116)). Renaming requires cluster access, SM secret recreation, and (for the S3 key) manual Hetzner console work with no API. They will be renamed opportunistically at the next server replacement.

| Legacy name | Used in | Future name |
|---|---|---|
| `pulumi-admin` (k8s SA) | cloud-init manifest, `bootstrap-k8s-admin.sh`, SM secret `k3s-pulumi-admin-token` | `tofu-admin` or `iac-admin` |
| `familiar-systems-pulumi-key` | Hetzner S3 credential, `providers.tf`, `hetzner_s3.tf`, `bootstrap-object-storage.sh` | `familiar-systems-iac-key` |
