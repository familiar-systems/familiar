# Infrastructure: k3s + OpenTofu

**Status:** Active
**Date:** 2026-05-23
**Supersedes:** [k3s + Pulumi Python](../archive/plans/2026-03-30-infrastructure.md) (self-hosted Pulumi for IaC management)
**Related decisions:** [Deployment Architecture](./2026-03-30-deployment-architecture.md), [Campaign Collaboration Architecture](./2026-03-25-campaign-collaboration-architecture.md), [libSQL decision](../discovery/2026-03-09-sqlite-over-postgres-decision.md)

---

## Architecture

```
+----------------------------------------------------------+
|  Hetzner CX23 (hel1) -- single-node k3s cluster          |
|                                                           |
|  Floating IP --> Traefik Ingress (built into k3s)         |
|                                                           |
|  Pods:                                                    |
|    platform    -- Auth, campaign CRUD, routing table,     |
|                   discover endpoint (Rust binary)         |
|    campaign    -- Actor hierarchy, WebSocket collab,      |
|                   AI conversations (Rust binary)          |
|    site        -- Astro static site (nginx)               |
|    web         -- Vite SPA (nginx)                        |
|                                                           |
|  Volume /data:                                            |
|    /data/k3s          -- k3s server state (etcd/SQLite)   |
|    /data/platform.db  -- Platform database (users,        |
|                          campaigns, routing table)        |
|    /data/campaigns    -- libSQL campaign databases         |
|                          (local cache; source of truth    |
|                          is Object Storage)               |
+----------------------------------------------------------+

+----------------------------------------------------------+
|  GitHub Actions                                           |
|    Build images --> push to Scaleway CR                   |
|    kustomize build | kubectl apply (deploys to k3s)       |
|    PR open --> create preview namespace + deploy          |
|    PR close --> delete preview namespace                  |
+----------------------------------------------------------+

+----------------------------------------------------------+
|  OpenTofu (infra/tofu/)                                   |
|    Manages: server, volume, floating IP, firewall,        |
|             SSH keys, Scaleway IAM, SM secrets,           |
|             Hetzner Object Storage buckets                |
|    Does NOT manage: k8s resources on the cluster          |
+----------------------------------------------------------+

+----------------------------------------------------------+
|  Nebius (Finnish GPU infrastructure)                      |
|    Python ML workers: faster-whisper, pyannote            |
|    Dispatched by campaign server via HTTP                 |
|    Stateless -- receives audio, returns transcripts       |
+----------------------------------------------------------+
```

The service topology (platform + campaign server split, interface boundaries, graceful restart protocol, preview environments) is defined in the [Deployment Architecture](./2026-03-30-deployment-architecture.md). This document covers the infrastructure those services run on.

### Why single-node k3s

k3s on a single node is a fully functional Kubernetes cluster including the datastore, control plane, and container runtime. The k3s maintainers explicitly state it's production-ready for single-node deployments. The cluster state (etcd/SQLite) lives on a Hetzner Volume, making server replacement automatic. If the single node is outgrown, k3s supports adding agent nodes with a single command using a join token.

---

## IaC Scope

Three tools manage infrastructure at different layers:

| Layer | Tool | What it manages |
|---|---|---|
| Cloud infrastructure | **OpenTofu** (`infra/tofu/`) | Hetzner: server, volume, floating IP, firewall, SSH keys. Scaleway: container registry, IAM, Secrets Manager. Hetzner Object Storage: S3 buckets, policies, lifecycle. |
| Cluster controllers | **Helm** (`infra/k8s/helm/`, `scripts/bootstrap-helm.sh`) | cert-manager, External Secrets Operator, webhook-bunny. One-time bootstrap, not CI-managed. |
| Application manifests | **Kustomize** (`infra/k8s/base/`, `overlays/`) | Deployments, Services, IngressRoutes, PV/PVC, ExternalSecrets, Certificates. Applied by GHA on every merge to main (prod) and PR push (preview). |

OpenTofu does not manage any Kubernetes resources. Kubernetes resources are either Helm-bootstrapped (controllers) or Kustomize-applied (application layer).

### OpenTofu project structure

```
infra/tofu/
  *.tf                    HCL resource definitions (see infra/CLAUDE.md Key Files)
  terraform.tfvars        Non-secret variable values for prod
  scripts/
    bootstrap.sh          One-time: state bucket + SM secrets
    setup.sh              Per-machine: generate .envrc
    bootstrap-k8s-admin.sh   k8s SA + kubeconfig to SM
    bootstrap-object-storage.sh   Hetzner S3 credentials + buckets
    rotate_scw_key.py     Scaleway IAM key rotation (click CLI)
```

See [`infra/CLAUDE.md`](../../infra/CLAUDE.md) for the full key files table, credential model, and bootstrap flows.

### Kustomize layout

```
infra/k8s/
  base/                   Shared manifests (Deployments, Services, NetworkPolicies)
    platform/, campaign/, site/, web/
  eso/                    ExternalSecret + ClusterSecretStore
  helm/                   Helm value files for bootstrapped controllers
  overlays/
    prod/                 Prod: cert, ingresses, patches, storage, secrets
    preview/              Preview: per-PR namespace, ingresses, patches, storage
  scripts/
    bootstrap-helm.sh     Install cert-manager, ESO, webhook-bunny
    apply-preview.sh      Apply preview overlay with envsubst
```

---

## SSL: cert-manager with DNS-01 + bunny.net

cert-manager is Helm-bootstrapped into the cluster (not OpenTofu-managed). It uses a ClusterIssuer with DNS-01 validation via the bunny.net webhook solver.

The Certificate resource in `infra/k8s/overlays/prod/cert/certificate.yaml` covers all four apexes:
- `familiar.systems`
- `app.familiar.systems`
- `preview.familiar.systems`
- `app.preview.familiar.systems`

The TLS secret name is `preview-wildcard-tls` (a historical name; the cert is neither wildcard nor preview-only). Certificates survive server replacement because they're stored as Kubernetes Secrets in k3s's datastore on the Volume at `/data/k3s`.

---

## Secrets Management

**Scaleway Secrets Manager** is the single source of truth for all secrets. Two paths deliver secrets to where they're needed:

1. **OpenTofu** reads SM at runtime via ephemeral `scaleway_secret_version` resources (for provider auth, never persisted to state) and `data` sources (for S3 credential access-key IDs used in bucket policies).
2. **External Secrets Operator (ESO)** runs in-cluster and syncs SM secrets into k8s Secrets via `ExternalSecret` CRDs. All application secrets flow through ESO.

See [`infra/CLAUDE.md`](../../infra/CLAUDE.md) for the full list of required SM secrets and who manages each.

---

## Data Persistence

Campaign databases and the platform database use Kubernetes PersistentVolumes backed by the Hetzner Volume. PV/PVC definitions live in Kustomize overlays (`infra/k8s/overlays/{prod,preview}/storage/`).

The [Campaign Collaboration Architecture](./2026-03-25-campaign-collaboration-architecture.md) establishes the persistence model: Object Storage is the source of truth for campaign databases, with the Volume serving as a local working cache. Writeback to Object Storage happens every ~30 seconds during active use.

---

## CI/CD Pipeline

After CI builds and pushes images to Scaleway CR, the GHA workflow applies the Kustomize overlay:

- **Prod:** `ci_cd_main.yml` runs `kustomize build infra/k8s/overlays/prod | kubectl apply` with SHA-tagged images.
- **Preview:** `ci_cd_preview.yml` runs `infra/k8s/scripts/apply-preview.sh` with per-PR variables (namespace, images, Hanko URL) via envsubst.

GHA authenticates to the k3s API server using a kubeconfig stored in Scaleway SM (secret `k3s-kubeconfig`), fetched at workflow runtime.

---

## Server Replacement Procedure

Because k3s state lives on the Hetzner Volume at `/data/k3s`:

1. OpenTofu provisions new server (Volume + Floating IP survive)
2. cloud-init mounts Volume at `/data`, installs k3s with `--data-dir /data/k3s`
3. k3s starts, reads existing state from Volume
4. All Deployments, Services, Ingresses, certificates resume automatically
5. Traefik (built into k3s) picks up cached certs from k3s state
6. DNS already points at Floating IP -- zero DNS changes

**No runbook steps.** The Volume IS the cluster. For the campaign server specifically, server replacement triggers the same reconnection flow as a graceful restart -- see [Deployment Architecture: Graceful Restart Protocol](./2026-03-30-deployment-architecture.md#graceful-restart-protocol).

---

## Legacy Naming

Two operational names retain "pulumi" from the previous IaC tool. See [`infra/CLAUDE.md` Legacy Naming](../../infra/CLAUDE.md#legacy-naming) for details and the rename plan.

---

## Key References

- **k3s Quick Start:** https://docs.k3s.io/quick-start
- **k3s Advanced Options (--data-dir):** https://docs.k3s.io/advanced
- **OpenTofu:** https://opentofu.org/docs/
- **cert-manager:** https://cert-manager.io
- **cert-manager-webhook-bunny:** https://github.com/nicholasgasior/cert-manager-webhook-bunny
- **External Secrets Operator:** https://external-secrets.io
- **Kustomize:** https://kustomize.io
