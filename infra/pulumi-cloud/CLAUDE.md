# CLAUDE.md -- infra/pulumi-cloud

## What This Is

Pulumi Python project for familiar.systems cloud infrastructure on Hetzner Cloud + Scaleway Container Registry + Scaleway Secrets Manager. State is stored in Scaleway Object Storage, secrets are encrypted with a passphrase from Scaleway Secrets Manager.

Single deployment target: **k3s cluster** serving `familiar.systems` + `app.familiar.systems` (production) and `preview.familiar.systems` + `app.preview.familiar.systems` (PR previews), plus the legacy `loreweaver.no` apex set until it retires. The two-apex layout (marketing vs app) is documented in [Deployment Architecture §URL routing](../../docs/plans/2026-03-30-deployment-architecture.md#url-routing).

## Key Files

| File                   | Purpose                                                                                                                   |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `__main__.py`          | Pulumi entrypoint. Wires together modules, declares exports.                                                              |
| `config.py`            | Shared constants: `LOCATION`, `SERVER_TYPE`, `IMAGE`, `LABELS`, `config` object.                                          |
| `cloud.py`             | Shared Hetzner resources (floating IP, SSH keys, firewall) + Scaleway resources (registry, secrets).                      |
| `k3s_cluster.py`       | `K3sCluster` ComponentResource: provisions k3s server with automated kubeconfig extraction via `pulumi-command`.          |
| `k8s.py`               | Kubernetes resources on the k3s cluster: cert-manager, webhook-bunny, ClusterIssuer, TLS cert, site deployment + ingress. |
| `Pulumi.yaml`          | Project config. Runtime is Python via uv toolchain.                                                                       |
| `Pulumi.prod.yaml`     | Stack config for `prod`. Contains encrypted secrets + SSH public keys.                                                    |
| `pyproject.toml`       | Python deps: pulumi, pulumi-hcloud, pulumi-command, pulumi-kubernetes, pulumiverse-scaleway.                              |
| `scripts/bootstrap.sh` | One-time setup: creates Scaleway bucket + passphrase secret.                                                              |
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
- ClusterIssuer (prod + staging), Certificate covering the aggregated `PRODUCTION_DOMAINS` + `PREVIEW_DOMAINS` from `config.py` (marketing + app apexes for each environment; the cert is not a wildcard — SAN list is the exact apex set)
- Site Deployment + Service + Ingress bound to marketing apexes only (`MARKETING_*_DOMAINS` in `config.py`); the SPA + platform + campaign bind to the app apexes separately

### Provider cascade hazard

Pulumi manages both the Hetzner server and the k8s workloads running on it. The k8s Provider's `kubeconfig` field is `replaceOnChanges`, so any change to it cascades through every k8s resource parented to the provider (delete-and-recreate, which momentarily breaks cert-manager, the TLS cert, and the site ingress).

**Two fronts of mitigation:**

- **`ignoreChanges: ["user_data"]` on the server resource** (`k3s_cluster.py`) prevents cloud-init edits from replacing the server. Cloud-init only runs at first boot anyway, so changes to it are meaningless on an existing server. **Note:** if you DO need to apply cloud-init changes (e.g., updating the auto-applied SA manifest), you'll have to replace the server, which is a planned-downtime event with cascade implications.
- **The k8s Provider's `kubeconfig` is built from byte-stable SM inputs**, not from an SSH-extracted Output that can change. See "Auth model" below. This means routine `pulumi up` calls (image bumps, helm upgrades, new manifests) cannot trigger the cascade as a side effect — only deliberate SA token rotation or cluster CA rotation can, and both are planned events.
- **`scripts/nuke-k8s.sh` for recovery** when the cascade happens anyway (e.g. intentional server resize). It removes k8s resources from Pulumi state so `pulumi up` can recreate them from scratch.

**If you need to intentionally replace the server** (resize, OS upgrade, etc.):

1. Run `./scripts/nuke-k8s.sh --wipe-k3s` (wipes k3s datastore on the Volume, cleans Pulumi state)
2. Temporarily remove `ignore_changes=["user_data"]` if cloud-init changes are needed
3. `pulumi up` (creates new server, reinstalls k3s, recreates all k8s resources)
4. Restore `ignore_changes=["user_data"]`

k3s state on the Volume (`/data/k3s`) persists across server replacements. A new server will inherit corrupted k3s state unless `/data/k3s` is wiped first. Campaign data (`/data/campaigns`, `/data/preview`) is unaffected.

## Reference Documentation

**MANDATORY: Read the relevant docs before writing or modifying any resource. Do not guess at API shapes, field names, or encoding requirements.**

### Pulumi Providers

- pulumiverse-scaleway registry: https://www.pulumi.com/registry/packages/scaleway/api-docs/
    - Secret: https://www.pulumi.com/registry/packages/scaleway/api-docs/secret/
    - SecretVersion: https://www.pulumi.com/registry/packages/scaleway/api-docs/secretversion/
    - RegistryNamespace: https://www.pulumi.com/registry/packages/scaleway/api-docs/registrynamespace/
- pulumi-kubernetes: https://www.pulumi.com/registry/packages/kubernetes/api-docs/
- pulumi-hcloud: https://www.pulumi.com/registry/packages/hcloud/api-docs/

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

Pulumi reads secrets from Scaleway SM at deploy time via `config.read_secret(name)`, which wraps `pulumiverse_scaleway.secrets.get_version_output()`. GHA reads the same secrets via `.github/actions/fetch-scw-secret/`.

### Required Scaleway SM Secrets

| Secret Name                 | Purpose                                                                       |
| --------------------------- | ----------------------------------------------------------------------------- |
| `loreweaver-deploy-ssh-key` | Break-glass SSH private key for direct server access (rare, manual ops)       |
| `bunny-api-key`             | bunny.net API key for DNS-01 ACME (deployed as k8s Secret)                    |
| `k3s-kubeconfig`            | Token-based kubeconfig for GHA deploys + local kubectl (operator-managed)     |
| `k3s-pulumi-admin-token`    | `pulumi-admin` ServiceAccount bearer token (cluster-admin, operator-managed)  |
| `k3s-cluster-ca`            | k3s cluster CA cert, base64 PEM (operator-managed, rarely rotates)            |

The `k3s-*` secrets are populated by `scripts/bootstrap-pulumi-admin.sh`, not by Pulumi. Pulumi reads them at deploy time via `config.read_secret(name)`. Re-running the bootstrap script is safe (idempotent) and writes new SM versions for all three.

### Registry pull credential (Pulumi-owned, not in SM)

The cluster's `registry-pull-secret` (`kubernetes.io/dockerconfigjson`) is built from a dedicated, least-privilege Scaleway IAM credential that Pulumi owns end-to-end: `iam.Application` (`k3s-registry-puller`) + `iam.Policy` (scoped to `ContainerRegistryReadOnly` on the registry project) + `iam.ApiKey` (see `cloud.py`). The `api_key.secret_key` Output flows directly into `docker_config` in `k8s.py`. There is no SM secret for this credential, and no operator bootstrap step -- `pulumi up` creates everything.

**Rotation behavior (important):** Pulumi's Kubernetes provider treats changes to `Secret.data` as **replace-triggering**, not in-place update. We verified this empirically on this resource with both `string_data=` and `data=` (base64) declarations -- both produce `+- replace`. The root cause lives in the provider's `forceNewProperties` logic and is not bypassable from user code without writing a custom resource. Rotating the credential therefore deletes-and-recreates this one Secret, producing a ~1-second window where `scaleway-registry` doesn't exist. Already-running pods are unaffected (their images are long since pulled); only pods being scheduled during that exact window hit `ImagePullBackOff` and retry. Acceptable blast radius for a credential that rotates on operator initiative, not on a schedule.

**The replace is confined to this one resource.** The k8s Provider is not being replaced, so nothing parented to it cascades -- no cert-manager churn, no site Deployment/Service/Ingress churn, no wildcard cert re-issue. The "never wire IaC to rotatable credentials" rule from `feedback_iac_credential_decoupling.md` applies specifically to credentials feeding `replaceOnChanges` fields on cascading parents (like `kubernetes.Provider.kubeconfig`). For non-cascading consumers, Pulumi-owning the credential lifecycle is the preferred shape because it eliminates operator toil; the one-second rotation gap on a single Secret resource is the explicit tradeoff we're accepting in exchange.

## Auth model (k8s Provider)

The Pulumi `kubernetes.Provider` authenticates with a **static long-lived ServiceAccount bearer token** in a byte-stable kubeconfig. The kubeconfig is constructed in `__main__.py` from three SM-sourced inputs (floating IP, cluster CA, SA token), all of which are stable across `pulumi up` runs unless deliberately rotated. Pulumi's diff sees the same kubeconfig string every time, so the Provider is never replaced and the k8s resource graph never cascades.

**Why this matters:** The `kubernetes.Provider.kubeconfig` field is `replaceOnChanges`. Wiring it to any Pulumi-tracked Output that can change (e.g., the SSH-extracted `command.remote.Command.stdout` we used before) means any rotation event causes the provider to be replaced, which deletes-and-recreates every k8s resource parented to it (10+ resources including cert-manager, the wildcard Cert, the site Deployment/Service/Ingress, etc.). The static-SA-token pattern eliminates this entirely. **Do not regress to "convenience" patterns that wire credentials directly to the Provider.** See `~/.claude/projects/.../memory/feedback_iac_credential_decoupling.md` for the full pathology.

**Bootstrap flows:**

- **Fresh cluster:** Cloud-init drops a manifest into `/var/lib/rancher/k3s/server/manifests/pulumi-admin.yaml` (k3s's auto-apply directory), creating the ServiceAccount + ClusterRoleBinding + token-Secret at first boot. The operator then runs `scripts/bootstrap-pulumi-admin.sh` once to capture the populated token + CA from the cluster and write them to SM. After that, `pulumi up` works normally.
- **Existing cluster:** Run `scripts/bootstrap-pulumi-admin.sh` directly. It applies the manifest via the current kubeconfig, captures the token + CA, and writes them to SM. Same end state as the fresh-cluster path.

**Rotation:** The SA token has no expiry and is valid as long as the SA exists. To rotate, re-run the bootstrap script — it pushes a new SM version, and the next `pulumi up` picks it up. **This DOES trigger the cascade** (kubeconfig string changes), so plan accordingly with `letsencrypt-staging-dns` as the active issuer (see `k8s.py` constants). Cluster CA rotation is a separate planned-downtime event; the CA is valid for ~10 years by default.

`make rotate-certs` was deleted because it was the kind of "convenience" routine that triggered the very cascade this auth model exists to prevent. If you ever genuinely need to rotate the CA, do it as a planned event with explicit operator action.

## Domains

Production and preview domains are listed in `config.py` as `PRODUCTION_DOMAINS` and `PREVIEW_DOMAINS`. Both lists are imported by `k8s.py` to construct the wildcard Certificate's `dnsNames` and the site Ingress's `tls`/`rules` blocks. To add a domain:

1. Append the apex (e.g., `example.com`) to `PRODUCTION_DOMAINS`.
2. Append the preview prefix (e.g., `preview.example.com`) to `PREVIEW_DOMAINS`.
3. Confirm the bunny.net account managing `bunny-api-key` controls the DNS zone for the new domain (DNS-01 ACME requires this).
4. `pulumi up` — the cert re-issues with the new SANs.

The wildcard Certificate covers, for each preview domain `D`: `D` itself and `*.D`. So `pr-42.preview.example.com` is covered automatically.

When migrating away from a domain, remove it from both lists and `pulumi up`. The cert re-issues without the old SANs; old DNS records can be deleted afterward.

## Preview environments (PR previews)

Per-PR preview deployments live in namespaces named `preview-pr-${PR_NUMBER}`, one per open PR. They are **not** Pulumi-managed — they're created and destroyed by two GitHub Actions workflows:

- `.github/workflows/deploy-preview.yml` — builds the PR's image, fetches the `k3s-kubeconfig` SM secret, and applies manifests from `infra/k8s/preview/*.yaml` via `envsubst` for templating. Runs on PR `opened`/`synchronize`.
- `.github/workflows/cleanup-preview.yml` — deletes the namespace (which cascade-removes all resources inside) and cleans up the registry image tag. Runs on PR `closed`.

The preview manifests live in `infra/k8s/preview/` as plain Kubernetes YAML with `${VAR}` placeholders. The templating variables are:

| Variable | Source | Example |
|---|---|---|
| `NAMESPACE` | workflow: `preview-pr-${PR_NUMBER}` | `preview-pr-42` |
| `PR_NUMBER` | workflow: the PR number; consumed by Ingress manifests to build `/pr-${N}` path prefixes and by the web build's `VITE_BASE_PATH` | `42` |
| `SITE_IMAGE` / `WEB_IMAGE` / `PLATFORM_IMAGE` | workflow: built image tags | `rg.fr-par.scw.cloud/loreweaver/site:pr-42-abc1234` (registry namespace retains the legacy project name) |
| `DOCKERCONFIG_B64` | workflow: base64-encoded dockerconfigjson for the SCW registry | (computed) |

**To edit preview behavior**, edit the YAML files directly — the workflow only handles substitution and apply. **To validate changes**, run `mise run lint:k8s` (kubeconform).

**Why this split, rather than Pulumi-managing the preview resources:** PR previews are ephemeral (seconds of creation, minutes of lifetime) and per-PR (one namespace per open PR, potentially dozens at once). Pulumi's state model is designed for long-lived, named resources; spinning up a Pulumi stack per PR would be wildly over-engineered. GitHub Actions can apply raw YAML in ~2s per resource, which is the right tool. The separation is: **Pulumi for permanent cluster state** (cert-manager, the TLS cert, the prod site + platform deployments, RBAC, the provider itself), **YAML-applied-by-CI for ephemeral per-PR resources**.

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
- **Lint/format/check command**: `uv run ruff check --fix . && uv run ruff format . && uv run basedpyright` (fix + format + typecheck in one pass).
