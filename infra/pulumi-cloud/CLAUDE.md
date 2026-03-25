# CLAUDE.md -- infra/pulumi-cloud

## What This Is

Pulumi Python project for Loreweaver's cloud infrastructure on Hetzner Cloud + Scaleway Container Registry + Scaleway Secrets Manager. State is stored in Scaleway Object Storage, secrets are encrypted with a passphrase from Scaleway Secrets Manager.

Single deployment target: **k3s cluster** serving `loreweaver.no` (production) and `preview.loreweaver.no` (PR previews).

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

The **Floating IP** (`cloud.py`) is the public entry point: DNS A record for `loreweaver.no` points here. It is a top-level resource, not owned by any cluster. The IP is passed into `K3sCluster` as an input.

`K3sCluster` ComponentResource encapsulates:

- Floating IP assignment (binds the external IP to the server)
- Volume (10GB, `/data/k3s`, `/data/campaigns`, `/data/preview`)
- Server with k3s cloud-init
- Automated kubeconfig extraction via `pulumi-command` (SSH, waits for cloud-init)

`k8s.py` declares all Kubernetes resources using the extracted kubeconfig:

- cert-manager (Jetstack Helm chart, v1.17.2)
- cert-manager-webhook-bunny (DNS-01 for bunny.net)
- ClusterIssuer + Certificate for `loreweaver.no` and `*.preview.loreweaver.no`
- Site Deployment + Service + Ingress (serves both production and preview domains)

### Provider cascade hazard

Pulumi manages both the Hetzner server and the k8s workloads running on it. The k8s provider is derived from the server's kubeconfig, creating a fragile dependency chain:

```
Server (Hetzner) --> kubeconfig (SSH command) --> k8s Provider --> all k8s resources
```

If the server is **replaced**, the entire chain cascades: Pulumi creates a new provider and tries to delete all old k8s resources via the old provider, but the old provider's gRPC connection is already dead (old server is gone). Every delete fails with `grpc: the client connection is closing`.

**Mitigations in place:**

- `ignoreChanges: ["user_data"]` on the server resource (`k3s_cluster.py`). Cloud-init only runs at first boot, so changes to it are meaningless on an existing server. This prevents cloud-init edits from triggering a server replacement and the resulting cascade.
- `scripts/nuke-k8s.sh` for recovery when the cascade happens anyway (e.g. intentional server resize). It removes k8s resources from Pulumi state so `pulumi up` can recreate them from scratch.

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
- pulumi-command: https://www.pulumi.com/registry/packages/command/api-docs/

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

| Secret Name                 | Purpose                                                         |
| --------------------------- | --------------------------------------------------------------- |
| `loreweaver-deploy-ssh-key` | SSH private key for `pulumi-command` to extract k3s kubeconfig  |
| `bunny-api-key`             | bunny.net API key for DNS-01 ACME (deployed as k8s Secret)      |
| `k3s-kubeconfig`            | k3s kubeconfig for GHA deploys (written by Pulumi, read by GHA) |

Registry auth uses `nologin` + `SCW_SECRET_KEY` directly (no SM secret needed).

## Commands

```bash
# Always source .envrc first (direnv does this automatically)
source .envrc

pulumi preview        # Dry-run
pulumi up             # Apply
pulumi stack output k3s_floating_ip    # Get k3s IP for DNS
pulumi stack output k3s_kubeconfig --show-secrets > /tmp/k3s.yaml  # Get kubeconfig

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
