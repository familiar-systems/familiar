# CLAUDE.md -- infra/pulumi-cloud

## What This Is

Pulumi Python project for Loreweaver's cloud infrastructure on Hetzner Cloud + Scaleway Container Registry + Scaleway Secrets Manager. State is stored in Scaleway Object Storage, secrets are encrypted with a passphrase from Scaleway Secrets Manager.

Single deployment target: **k3s cluster** serving `loreweaver.no` (production) and `preview.loreweaver.no` (PR previews).

## Key Files

| File                   | Purpose                                                                                                                   |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `__main__.py`          | Pulumi entrypoint. Wires together modules, declares exports.                                                              |
| `config.py`            | Shared constants: `LOCATION`, `SERVER_TYPE`, `IMAGE`, `LABELS`, `config` object.                                          |
| `cloud.py`             | Shared Hetzner resources (SSH keys, firewall) + Scaleway resources (registry, secrets).                                   |
| `k3s_cluster.py`       | `K3sCluster` ComponentResource: provisions k3s server with automated kubeconfig extraction via `pulumi-command`.          |
| `k8s.py`               | Kubernetes resources on the k3s cluster: cert-manager, webhook-bunny, ClusterIssuer, TLS cert, site deployment + ingress. |
| `Pulumi.yaml`          | Project config. Runtime is Python via uv toolchain.                                                                       |
| `Pulumi.prod.yaml`     | Stack config for `prod`. Contains encrypted secrets + SSH public keys.                                                    |
| `pyproject.toml`       | Python deps: pulumi, pulumi-hcloud, pulumi-command, pulumi-kubernetes, pulumiverse-scaleway.                              |
| `scripts/bootstrap.sh` | One-time setup: creates Scaleway bucket + passphrase secret.                                                              |
| `scripts/setup.sh`     | Per-machine setup: generates `.envrc` from existing Scaleway resources.                                                   |

## Architecture

### k3s (k3s_cluster.py + k8s.py)

`K3sCluster` ComponentResource encapsulates:

- Floating IP (k3s-owned, DNS points `loreweaver.no` here)
- Volume (10GB, `/data/k3s`, `/data/campaigns`, `/data/preview`)
- Server with k3s cloud-init
- Automated kubeconfig extraction via `pulumi-command` (SSH, waits for cloud-init)

`k8s.py` declares all Kubernetes resources using the extracted kubeconfig:

- cert-manager (Jetstack Helm chart, v1.17.2)
- cert-manager-webhook-bunny (DNS-01 for bunny.net)
- ClusterIssuer + Certificate for `loreweaver.no` and `*.preview.loreweaver.no`
- Site Deployment + Service + Ingress (serves both production and preview domains)

CRDs (ClusterIssuer, Certificate) use `pulumi_kubernetes.apiextensions.CustomResource` -- not `ConfigGroup`, which can't resolve CRD schemas before cert-manager is installed.

**Important:** CRD specs (ClusterIssuer, Certificate) and webhook solver configs are untyped dicts. Always read the upstream docs before writing or modifying them (see Reference Documentation section below).

## Reference Documentation

**MANDATORY: Read the relevant docs before writing or modifying any resource. Do not guess at API shapes, field names, or encoding requirements.**

### Pulumi Providers

- pulumiverse-scaleway registry: https://www.pulumi.com/registry/packages/scaleway/api-docs/
    - Secret: https://www.pulumi.com/registry/packages/scaleway/api-docs/secret/
    - SecretVersion: https://www.pulumi.com/registry/packages/scaleway/api-docs/secretversion/ (`data` field takes RAW payload, not base64)
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

### Three-Resource Dependency Pattern

Volume and Floating IP use a three-resource pattern to avoid circular dependencies:

```
FloatingIp (location only) --> ip_address --> Server (cloud-init configures loopback)
         |                                          |
         +-------- FloatingIpAssignment ------------+

Volume (location only) --> linux_device --> Server (cloud-init writes fstab)
         |                                        |
         +-------- VolumeAttachment --------------+
```

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

### Pulumi Config (non-secret)

| Key                       | Purpose                                                     |
| ------------------------- | ----------------------------------------------------------- |
| `hcloud:token`            | Hetzner provider credential (encrypted in Pulumi.prod.yaml) |
| `acme-email`              | Email for Let's Encrypt registration (not a secret)         |
| `personal-ssh-public-key` | SSH public key (not a secret)                               |
| `deploy-ssh-public-key`   | SSH public key (not a secret)                               |

## Pulumi Exports

| Export                     | Used By                                      |
| -------------------------- | -------------------------------------------- |
| `registry_endpoint`        | GHA deploy workflow (image push target)      |
| `deploy_ssh_secret_id`     | Reference                                    |
| `bunny_api_key_secret_id`  | Reference                                    |
| `k3s_kubeconfig_secret_id` | GHA deploy workflows                         |
| `k3s_floating_ip`          | DNS A record for `loreweaver.no` (bunny.net) |
| `k3s_server_ip`            | Direct SSH access to k3s server              |
| `k3s_kubeconfig`           | k8s Provider + GHA workflows                 |

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

## Local k8s Testing

See `../test-k8s/README.md` for a k3d-based smoke test of the Kubernetes resources.

## Rules

- Never commit `.env`, `.envrc`, or any file containing raw credentials.
- All application secrets live in Scaleway Secrets Manager. Only provider credentials (e.g. `hcloud:token`) belong in Pulumi config.
- The `encryptionsalt` in `Pulumi.prod.yaml` is safe to commit -- it's not a secret.
- Python: ruff for linting/formatting, basedpyright for type checking. Strict config in `pyproject.toml`.
- **Lint/format/check command**: `uv run ruff check --fix . && uv run ruff format . && uv run basedpyright` (fix + format + typecheck in one pass).
- The k3s volume has `delete_protection=True`. Must be disabled before Pulumi can destroy it (intentional friction).
