# Infrastructure: k3s + Pulumi Python

**Status:** Active
**Date:** 2026-03-30
**Supersedes:** [k3s + Pulumi Infrastructure (deployment strategy)](../archive/plans/2026-03-12-deployment-strategy.md) — jointly with [Deployment Architecture](./2026-03-30-deployment-architecture.md). The superseded document covered both infrastructure primitives and deployment concerns as one plan with a Coolify-to-k3s migration path. That migration is complete. This document describes the infrastructure as it exists today.
**Related decisions:** [Deployment Architecture](./2026-03-30-deployment-architecture.md), [Campaign Collaboration Architecture](./2026-03-25-campaign-collaboration-architecture.md), [libSQL decision](../discovery/2026-03-09-sqlite-over-postgres-decision.md)

---

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│  Hetzner CX23 (hel1) — single-node k3s cluster           │
│                                                           │
│  Floating IP ──→ Traefik Ingress (built into k3s)         │
│                                                           │
│  Pods:                                                    │
│    platform    ← Auth, campaign CRUD, routing table,      │
│                  discover endpoint (Rust binary)           │
│    campaign    ← Actor hierarchy, WebSocket collab,        │
│                  AI conversations (Rust binary)            │
│    site        ← Astro static site (nginx)                │
│    web         ← Vite SPA (nginx)                         │
│                                                           │
│  Volume /data:                                            │
│    /data/k3s          ← k3s server state (etcd/SQLite)    │
│    /data/platform.db  ← Platform database (users,         │
│                         campaigns, routing table)          │
│    /data/campaigns    ← libSQL campaign databases          │
│                         (local cache; source of truth      │
│                         is Object Storage)                 │
│                                                           │
│  Pulumi manages: server, volume, floating IP,             │
│                  AND all k8s resources on the cluster      │
└──────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────┐
│  GitHub Actions                                           │
│    Build images → push to Scaleway CR                     │
│    pulumi up --stack prod (deploys to k3s)                │
│    PR open → create preview namespace + deploy            │
│    PR close → delete preview namespace                    │
└──────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────┐
│  Local dev (your laptop)                                  │
│    docker compose up (platform + campaign + deps)         │
│    k3d for testing infra changes against local k3s        │
│    pulumi up --stack dev (deploys to local k3d)           │
└──────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────┐
│  Nebius (Finnish GPU infrastructure)                      │
│    Python ML workers: faster-whisper, pyannote            │
│    Dispatched by campaign server via HTTP                 │
│    Stateless — receives audio, returns transcripts        │
└──────────────────────────────────────────────────────────┘
```

The service topology (platform + campaign server split, interface boundaries, graceful restart protocol, preview environments) is defined in the [Deployment Architecture](./2026-03-30-deployment-architecture.md). This document covers the infrastructure those services run on.

### Why single-node k3s

k3s on a single node is a fully functional Kubernetes cluster including the datastore, control plane, and container runtime ([k3s Quick Start](https://docs.k3s.io/quick-start)). The k3s maintainers explicitly state it's production-ready for single-node deployments ([k3s Discussion #2988](https://github.com/k3s-io/k3s/discussions/2988)). The cluster state (etcd/SQLite) lives on a Hetzner Volume, making server replacement automatic. If the single node is outgrown, k3s supports adding agent nodes with a single command using a join token.

### Why Pulumi Python

The existing Pulumi project is Python. The ML/ASR pipeline work is Python. basedpyright with block-on-fail provides compile-time-like guarantees that catch type errors before runtime. Pulumi's Kubernetes SDK has full Python support with the same auto-generated types from the Kubernetes OpenAPI spec ([Pulumi Kubernetes SDK](https://github.com/pulumi/pulumi-kubernetes)). The provider types are always current because they're generated from the upstream spec ([pulumi-kubernetes README](https://github.com/pulumi/pulumi-kubernetes)). Keeping infrastructure in Python avoids context-switching between infra and ML work.

---

## The Local Test Loop

Every infrastructure change is testable locally before it touches production.

**k3d** runs k3s inside Docker containers on your laptop ([k3d.io](https://k3d.io)). It creates and destroys clusters in seconds:

```bash
# Create a local cluster
k3d cluster create familiar-dev --port "8080:80@loadbalancer" --port "8443:443@loadbalancer"

# Your kubeconfig is automatically configured
kubectl get nodes  # shows your local k3s node

# Destroy when done
k3d cluster delete familiar-dev
```

**Pulumi stacks** let you target different clusters from the same code:

```bash
# Test locally
pulumi up --stack dev    # targets k3d cluster

# Ship to production
pulumi up --stack prod   # targets Hetzner k3s cluster
```

**The agent-friendly loop:**

1. Agent writes/modifies Pulumi Python code
2. `pulumi preview --stack dev` — shows diff, basedpyright catches type errors
3. `pulumi up --stack dev` — applies to local k3d cluster
4. If it fails: deterministic error message from Kubernetes API, fix and retry
5. If it works: `pulumi up --stack prod`

Pulumi also supports rendering manifests to YAML without applying, useful for dry-run inspection ([Pulumi renderYamlToDirectory](https://flicksfix.com/posts/using-pulumi-as-helm-alternative-for-templating/)):

```python
provider = k8s.Provider("dry-run",
    render_yaml_to_directory="output",
)
```

Note: k3d is for testing **infrastructure changes** (Pulumi manifests, ingress rules, cert config). Application development uses Docker Compose — see [Deployment Architecture](./2026-03-30-deployment-architecture.md).

---

## Pulumi Project Structure

```
infra/
├── pulumi-cloud/           # Hetzner + Scaleway resources
│   ├── __main__.py
│   ├── Pulumi.yaml
│   ├── Pulumi.prod.yaml
│   └── pyproject.toml
│
├── pulumi-k8s/             # Kubernetes resources on the k3s cluster
│   ├── __main__.py         # Entry point
│   ├── cert_manager.py     # ClusterIssuer, Certificate (DNS-01 + bunny)
│   ├── platform.py         # Platform Deployment + Service + Ingress routes
│   ├── campaign.py         # Campaign server Deployment + Service + Ingress routes
│   ├── site.py             # Astro static site Deployment + Service + Ingress
│   ├── web.py              # SPA Deployment + Service + Ingress
│   ├── preview.py          # PR preview namespace/deploy/ingress factory
│   ├── Pulumi.yaml
│   ├── Pulumi.dev.yaml     # k3d kubeconfig
│   ├── Pulumi.prod.yaml    # Hetzner k3s kubeconfig
│   └── pyproject.toml
```

### pulumi-cloud (Hetzner infrastructure)

Outputs the kubeconfig for the k3s cluster, which `pulumi-k8s` consumes via stack references.

**Resources:** SSH key, Floating IP, Volume, Firewall, Server (cloud-init installs k3s), FloatingIpAssignment, VolumeAttachment, Scaleway CR namespace.

**cloud-init:**

```yaml
runcmd:
    - curl -sfL https://get.k3s.io | INSTALL_K3S_EXEC="--data-dir /data/k3s --tls-san <floating-ip> --node-external-ip <floating-ip>" sh -
```

> **Open question (--tls-san / --node-external-ip):** `--tls-san` is needed so the API server cert includes the Floating IP as a SAN — otherwise the kubeconfig breaks on server replacement because the primary IP changes. `--node-external-ip` may be needed so Traefik binds to the Floating IP. Verify both flags against the k3s docs before implementing.

k3s supports custom data directories via `--data-dir` ([k3s Advanced Options](https://docs.k3s.io/advanced)). This puts all k3s state (embedded SQLite/etcd, certificates, manifests) on the Hetzner Volume.

### pulumi-k8s (Kubernetes resources)

Everything is typed Python. The `preview.py` module implements the PR preview namespace factory described in the [Deployment Architecture](./2026-03-30-deployment-architecture.md).

---

## SSL: cert-manager with DNS-01 + bunny.net

cert-manager handles automated certificate management, using the Lego ACME library with bunny.net as the DNS provider ([Lego DNS Providers](https://go-acme.github.io/lego/dns/)).

Install cert-manager via Pulumi's Helm v4 Chart resource ([Pulumi Helm v4 Chart](https://www.pulumi.com/blog/kubernetes-chart-v4/)):

```python
import pulumi_kubernetes as k8s

cert_manager = k8s.helm.v4.Chart("cert-manager",
    namespace="cert-manager",
    chart="oci://registry-1.docker.io/bitnamicharts/cert-manager",
    version="1.3.1",
    values={"installCRDs": True},
)
```

> **Open question (chart source):** Bitnami's cert-manager chart vs the official chart at `https://charts.jetstack.io` (`jetstack/cert-manager`). Bitnami charts sometimes lag behind upstream and use their own image builds. For a security-critical component, the official chart may be the better choice. Evaluate both before implementing.

ClusterIssuer for DNS-01 with bunny.net:

```python
config = pulumi.Config()

bunny_api_key_secret = k8s.core.v1.Secret("bunny-api-key",
    metadata=k8s.meta.v1.ObjectMetaArgs(namespace="cert-manager"),
    string_data={"api-key": config.require_secret("bunnyApiKey")},
)

cluster_issuer = k8s.apiextensions.CustomResource("letsencrypt-dns",
    api_version="cert-manager.io/v1",
    kind="ClusterIssuer",
    metadata=k8s.meta.v1.ObjectMetaArgs(name="letsencrypt-dns"),
    other_fields={
        "spec": {
            "acme": {
                "server": "https://acme-v02.api.letsencrypt.org/directory",
                "email": "mike@familiar.systems",
                "privateKeySecretRef": {"name": "letsencrypt-dns-key"},
                "solvers": [{
                    "dns01": {
                        "webhook": {
                            # bunny.net solver config — cert-manager-webhook-bunny
                            # https://github.com/nicholasgasior/cert-manager-webhook-bunny
                        },
                    },
                }],
            },
        },
    },
    opts=pulumi.ResourceOptions(depends_on=[cert_manager]),
)
```

Certificate covering both apexes in prod and preview. Path-based routing within each apex removed the need for wildcard SANs; see [app-server PRD §URL architecture](./2026-04-11-app-server-prd.md#url-architecture).

```python
wildcard_cert = k8s.apiextensions.CustomResource("preview-wildcard",
    api_version="cert-manager.io/v1",
    kind="Certificate",
    metadata=k8s.meta.v1.ObjectMetaArgs(
        name="preview-wildcard",
        namespace="default",
    ),
    other_fields={
        "spec": {
            "secretName": "preview-wildcard-tls",
            "issuerRef": {"name": "letsencrypt-dns", "kind": "ClusterIssuer"},
            "dnsNames": [
                "familiar.systems",
                "app.familiar.systems",
                "preview.familiar.systems",
                "app.preview.familiar.systems",
            ],
        },
    },
)
```

The secret name remains `preview-wildcard-tls` for continuity with existing Ingress references, despite the cert being neither a wildcard nor preview-only. See `infra/pulumi-cloud/k8s.py` for the authoritative resource declaration.

Certificates survive server replacement — stored as Kubernetes Secrets in k3s's datastore on the Volume at `/data/k3s`.

---

## Secrets Management

Secrets (registry credentials, bunny API key, etc.) are Kubernetes Secrets created via Pulumi. Pulumi encrypts them in state, creates them as k8s resources, and they survive server replacement because the k3s datastore is on `/data/k3s`.

```python
scaleway_cr_secret = k8s.core.v1.Secret("scaleway-cr",
    type="kubernetes.io/dockerconfigjson",
    metadata=k8s.meta.v1.ObjectMetaArgs(name="scaleway-cr"),
    string_data={
        ".dockerconfigjson": pulumi.Output.json_dumps({
            "auths": {
                "rg.fr-par.scw.cloud": {
                    "username": "nologin",
                    "password": config.require_secret("scwSecretKey"),
                },
            },
        }),
    },
)
```

---

## Data Persistence

Campaign databases and the platform database use Kubernetes PersistentVolumes backed by the Hetzner Volume:

```python
campaign_pv = k8s.core.v1.PersistentVolume("campaigns",
    spec=k8s.core.v1.PersistentVolumeSpecArgs(
        capacity={"storage": "10Gi"},
        access_modes=["ReadWriteOnce"],
        host_path=k8s.core.v1.HostPathVolumeSourceArgs(path="/data/campaigns"),
        persistent_volume_reclaim_policy="Retain",
    ),
)

campaign_pvc = k8s.core.v1.PersistentVolumeClaim("campaigns",
    spec=k8s.core.v1.PersistentVolumeClaimSpecArgs(
        access_modes=["ReadWriteOnce"],
        resources=k8s.core.v1.VolumeResourceRequirementsArgs(
            requests={"storage": "10Gi"},
        ),
        volume_name=campaign_pv.metadata.name,
    ),
)
```

The [Campaign Collaboration Architecture](./2026-03-25-campaign-collaboration-architecture.md) establishes the persistence model: Object Storage is the source of truth for campaign databases, with the Volume serving as a local working cache. Writeback to Object Storage happens every ~30 seconds during active use. The PV/PVC definitions above are for the local cache.

---

## CI/CD Pipeline

**GHA deploy workflow:** After CI builds and pushes images to Scaleway CR, `pulumi up --stack prod` in `infra/pulumi-k8s/` rolls the Deployments. Same artifact CI tested is what deploys.

> **Open question (GHA → k3s auth):** Both `pulumi up --stack prod` and `kubectl apply` for previews need a kubeconfig that authenticates to the k3s API server. How does GHA get it? Options: (a) store the kubeconfig in Scaleway SM, (b) create a Kubernetes ServiceAccount with a long-lived token and store that in SM, (c) use Pulumi stack output from pulumi-cloud to pass the kubeconfig. This is on the critical path for the CD workflow.

---

## Server Replacement Procedure

Because k3s state lives on the Hetzner Volume at `/data/k3s`:

1. Pulumi provisions new server (Volume + Floating IP survive)
2. cloud-init mounts Volume at `/data`, installs k3s with `--data-dir /data/k3s`
3. k3s starts, reads existing state from Volume
4. All Deployments, Services, Ingresses, certificates resume automatically
5. Traefik (built into k3s) picks up cached certs from k3s state
6. DNS already points at Floating IP — zero DNS changes

**No runbook steps.** The Volume IS the cluster. For the campaign server specifically, server replacement triggers the same reconnection flow as a graceful restart — see [Deployment Architecture: Graceful Restart Protocol](./2026-03-30-deployment-architecture.md#graceful-restart-protocol).

---

## Key References

- **k3s Quick Start:** https://docs.k3s.io/quick-start
- **k3s Advanced Options (--data-dir):** https://docs.k3s.io/advanced
- **k3s single-node production readiness:** https://github.com/k3s-io/k3s/discussions/2988
- **k3d (k3s in Docker for local dev):** https://k3d.io
- **Pulumi Kubernetes SDK:** https://github.com/pulumi/pulumi-kubernetes
- **Pulumi Helm v4 Chart:** https://www.pulumi.com/blog/kubernetes-chart-v4/
- **Pulumi Kubernetes guide:** https://www.pulumi.com/kubernetes/
- **Pulumi renderYamlToDirectory (dry-run):** https://flicksfix.com/posts/using-pulumi-as-helm-alternative-for-templating/
- **cert-manager:** https://cert-manager.io
- **Lego DNS providers (bunny.net):** https://go-acme.github.io/lego/dns/
- **cert-manager-webhook-bunny:** https://github.com/nicholasgasior/cert-manager-webhook-bunny
