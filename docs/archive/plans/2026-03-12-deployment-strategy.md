# Plan: Loreweaver Infrastructure — k3s + Pulumi Python

## Why This Plan Exists

The Coolify-based plan surfaced structural problems: Coolify's state lives in a Postgres database inside a Docker named volume that's awkward to persist across server replacements ([Coolify Backup & Restore docs](https://coolify.io/docs/knowledge-base/how-to/backup-restore-coolify)). Traefik proxy configuration is per-server and managed through a dashboard UI with no API endpoint for proxy config ([DeepWiki: Proxy Management](https://deepwiki.com/coollabsio/coolify/3.4-proxy-management)). Application migration between servers requires manual re-creation ([Coolify: Migrate Applications](https://coolify.io/docs/knowledge-base/how-to/migrate-apps-different-host)). The CLI exists but doesn't cover proxy or server setup ([coolify-cli on GitHub](https://github.com/coollabsio/coolify-cli)).

k3s replaces all of this with declarative manifests in git. Everything is code, everything is testable locally, and server replacement is "point k3s at the same state."

## Architecture (Target State — after Phase 3)

```
┌─────────────────────────────────────────────────────┐
│  Hetzner CX23 (hel1) — single-node k3s cluster      │
│                                                      │
│  Floating IP ──→ Traefik Ingress (built into k3s)    │
│                                                      │
│  Volume /data:                                       │
│    /data/k3s          ← k3s server state (etcd/SQLite)│
│    /data/campaigns    ← libsql campaign databases (local cache; source of truth is Object Storage, see Hocuspocus ADR) │
│    /data/preview      ← PR preview db copies          │
│                                                      │
│  Pulumi manages: server, volume, floating IP,        │
│                  AND all k8s resources on the cluster │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│  GitHub Actions                                      │
│    Build image → push to Scaleway CR                 │
│    pulumi up --stack prod (deploys to k3s)            │
│    PR open → create preview namespace + deploy       │
│    PR close → delete preview namespace               │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│  Local dev (your laptop)                             │
│    k3d cluster create loreweaver-dev                 │
│    pulumi up --stack dev (deploys to local k3d)      │
│    Iterate until it works                            │
│    k3d cluster delete loreweaver-dev                 │
└─────────────────────────────────────────────────────┘
```

During Phases 1-2, Coolify serves `loreweaver.no` on a separate server while k3s is built out on `preview.loreweaver.no`. Phase 3 collapses to the single-server target state above.

### Why single-node, not main/worker?

k3s on a single node is a fully functional Kubernetes cluster including the datastore, control plane, and container runtime ([k3s Quick Start](https://docs.k3s.io/quick-start)). The k3s maintainers explicitly state it's production-ready for single-node deployments ([k3s Discussion #2988](https://github.com/k3s-io/k3s/discussions/2988)). Unlike Coolify, the cluster state (etcd/SQLite) is a well-understood, file-based store that can live on a Hetzner Volume. If you outgrow the single node, k3s supports adding agent nodes with a single command using a join token.

### Why Pulumi Python?

The existing Pulumi project is Python. The ML/ASR pipeline work is Python. basedpyright with block-on-fail provides compile-time-like guarantees that catch type errors before runtime. Pulumi's Kubernetes SDK has full Python support with the same auto-generated types from the Kubernetes OpenAPI spec ([Pulumi Kubernetes SDK](https://github.com/pulumi/pulumi-kubernetes)). The provider types are always current because they're generated from the upstream spec ([pulumi-kubernetes README](https://github.com/pulumi/pulumi-kubernetes)). Keeping infrastructure in Python avoids context-switching between infra and ML work, and the application code (TypeScript) is a different cognitive context anyway.

---

## The Local Test Loop

This is the key advantage over Coolify. Every infrastructure change is testable locally before it touches production.

**k3d** runs k3s inside Docker containers on your laptop ([k3d.io](https://k3d.io)). It creates and destroys clusters in seconds:

```bash
# Create a local cluster
k3d cluster create loreweaver-dev --port "8080:80@loadbalancer" --port "8443:443@loadbalancer"

# Your kubeconfig is automatically configured
kubectl get nodes  # shows your local k3s node

# Destroy when done
k3d cluster delete loreweaver-dev
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

---

## Pulumi Project Structure

```
infra/
├── pulumi-cloud/           # Hetzner + Scaleway resources (existing Python project)
│   ├── __main__.py
│   ├── Pulumi.yaml
│   ├── Pulumi.prod.yaml
│   └── pyproject.toml
│
├── pulumi-k8s/             # Kubernetes resources on the k3s cluster
│   ├── __main__.py         # Entry point
│   ├── cert_manager.py     # ClusterIssuer, Certificate (DNS-01 + bunny)
│   ├── site.py             # Deployment + Service + Ingress for Astro site
│   ├── preview.py          # PR preview namespace/deploy/ingress factory
│   ├── Pulumi.yaml
│   ├── Pulumi.dev.yaml     # k3d kubeconfig
│   ├── Pulumi.prod.yaml    # Hetzner k3s kubeconfig
│   └── pyproject.toml
```

### pulumi-cloud (Hetzner infrastructure)

Existing Python project, extended. Outputs the kubeconfig for the k3s cluster, which `pulumi-k8s` consumes via stack references.

**Resources:** SSH key, Floating IP, Volume, Firewall, Server (cloud-init installs k3s instead of Coolify), FloatingIpAssignment, VolumeAttachment, Scaleway CR namespace.

**cloud-init change from original plan:** Instead of installing Coolify, install k3s with the data dir on the Volume:

```yaml
runcmd:
    - curl -sfL https://get.k3s.io | INSTALL_K3S_EXEC="--data-dir /data/k3s --tls-san <floating-ip> --node-external-ip <floating-ip>" sh -
```

> **Open question (--tls-san / --node-external-ip):** We think `--tls-san` is needed so the API server cert includes the Floating IP as a SAN — otherwise the kubeconfig breaks on server replacement because the primary IP changes. `--node-external-ip` may be needed so Traefik binds to the Floating IP. Verify both flags against the k3s docs before implementing.

k3s supports custom data directories via `--data-dir` ([k3s Advanced Options](https://docs.k3s.io/advanced)). This puts all k3s state (embedded SQLite/etcd, certificates, manifests) on the Hetzner Volume. Server replacement: new server, attach Volume, run the same k3s install command, cluster comes back with all state intact.

### pulumi-k8s (Kubernetes resources)

This is where the declarative magic lives. Everything that was dashboard clicks in Coolify becomes typed Python.

---

## SSL: cert-manager with DNS-01 + bunny.net

cert-manager is the standard Kubernetes tool for automated certificate management. It uses the same Lego ACME library that Traefik uses under the hood, with bunny.net as a supported DNS provider ([Lego DNS Providers](https://go-acme.github.io/lego/dns/)).

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

Then declare a ClusterIssuer for DNS-01 with bunny.net:

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
                "email": "mike@loreweaver.no",
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

The wildcard certificate for preview environments:

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
                "loreweaver.no",
                "*.preview.loreweaver.no",
            ],
        },
    },
)
```

**This certificate survives server replacement** because it's stored as a Kubernetes Secret in k3s's datastore, which lives on the Volume at `/data/k3s`. No reconfiguration needed.

---

## Production Site Deployment

Secrets (registry credentials, bunny API key, etc.) are Kubernetes Secrets created via Pulumi — they live in the cluster's datastore on the Volume. No external secret manager dance, no manual filling. Pulumi encrypts them in state, creates them as k8s resources, and they survive server replacement because the k3s datastore is on `/data/k3s`. This is one of the biggest operational wins over Coolify.

```python
# Registry auth — lets k8s pull images from Scaleway CR
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

site_deployment = k8s.apps.v1.Deployment("site",
    metadata=k8s.meta.v1.ObjectMetaArgs(labels={"app": "site"}),
    spec=k8s.apps.v1.DeploymentSpecArgs(
        replicas=1,
        selector=k8s.meta.v1.LabelSelectorArgs(match_labels={"app": "site"}),
        template=k8s.core.v1.PodTemplateSpecArgs(
            metadata=k8s.meta.v1.ObjectMetaArgs(labels={"app": "site"}),
            spec=k8s.core.v1.PodSpecArgs(
                containers=[k8s.core.v1.ContainerArgs(
                    name="site",
                    image=registry_endpoint.apply(lambda ep: f"{ep}/site:latest"),
                    ports=[k8s.core.v1.ContainerPortArgs(container_port=80)],
                )],
                image_pull_secrets=[k8s.core.v1.LocalObjectReferenceArgs(name="scaleway-cr")],
            ),
        ),
    ),
)

site_service = k8s.core.v1.Service("site",
    metadata=k8s.meta.v1.ObjectMetaArgs(labels={"app": "site"}),
    spec=k8s.core.v1.ServiceSpecArgs(
        selector={"app": "site"},
        ports=[k8s.core.v1.ServicePortArgs(port=80, target_port=80)],
    ),
)

site_ingress = k8s.networking.v1.Ingress("site",
    metadata=k8s.meta.v1.ObjectMetaArgs(
        annotations={"cert-manager.io/cluster-issuer": "letsencrypt-dns"},
    ),
    spec=k8s.networking.v1.IngressSpecArgs(
        tls=[k8s.networking.v1.IngressTLSArgs(
            hosts=["loreweaver.no"],
            secret_name="site-tls",
        )],
        rules=[k8s.networking.v1.IngressRuleArgs(
            host="loreweaver.no",
            http=k8s.networking.v1.HTTPIngressRuleValueArgs(
                paths=[k8s.networking.v1.HTTPIngressPathArgs(
                    path="/",
                    path_type="Prefix",
                    backend=k8s.networking.v1.IngressBackendArgs(
                        service=k8s.networking.v1.IngressServiceBackendArgs(
                            name="site",
                            port=k8s.networking.v1.ServiceBackendPortArgs(number=80),
                        ),
                    ),
                )],
            ),
        )],
    ),
)
```

**GHA deploy workflow:** After CI builds and pushes the image, run `pulumi up --stack prod` in the `infra/pulumi-k8s` directory. Pulumi detects the image tag change, rolls the Deployment. Same artifact CI tested is what deploys — identical to the original plan's intent.

> **Open question (GHA → k3s auth):** Both `pulumi up --stack prod` and `kubectl apply` for previews need a kubeconfig that authenticates to the k3s API server. How does GHA get it? Options: (a) store the kubeconfig in Scaleway SM, (b) create a Kubernetes ServiceAccount with a long-lived token and store that in SM, (c) use Pulumi stack output from pulumi-cloud to pass the kubeconfig. This is on the critical path for the CD workflow and needs to be resolved before Phase 2 deploys to production.

---

## PR Preview Environments

Each PR gets its own Kubernetes Namespace with a Deployment, Service, and Ingress. The wildcard cert (`*.preview.loreweaver.no`) covers all preview subdomains automatically.

**GHA workflow on PR open/push:**

```bash
# Create namespace
kubectl create namespace preview-pr-${PR_NUMBER} --dry-run=client -o yaml | kubectl apply -f -

# Deploy preview
kubectl -n preview-pr-${PR_NUMBER} apply -f - <<EOF
apiVersion: apps/v1
kind: Deployment
metadata:
  name: site-preview
spec:
  replicas: 1
  selector:
    matchLabels:
      app: site-preview
  template:
    metadata:
      labels:
        app: site-preview
    spec:
      containers:
      - name: site
        image: ${REGISTRY_ENDPOINT}/site:pr-${PR_NUMBER}
        ports:
        - containerPort: 80
      imagePullSecrets:
      - name: scaleway-cr
---
apiVersion: v1
kind: Service
metadata:
  name: site-preview
spec:
  selector:
    app: site-preview
  ports:
  - port: 80
---
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: site-preview
spec:
  tls:
  - hosts:
    - pr-${PR_NUMBER}.preview.loreweaver.no
    secretName: preview-wildcard-tls
  rules:
  - host: pr-${PR_NUMBER}.preview.loreweaver.no
    http:
      paths:
      - path: /
        pathType: Prefix
        backend:
          service:
            name: site-preview
            port:
              number: 80
EOF
```

**GHA workflow on PR close:**

```bash
kubectl delete namespace preview-pr-${PR_NUMBER}
```

That's it. Namespace deletion cascades and cleans up everything. No Coolify API, no webhook, no prune strategy.

For database previews (when the API exists): `cp /data/campaigns/campaign-1.db /data/preview/pr-${PR_NUMBER}/` and mount the preview directory into the API container's PVC. Same `cp` strategy from the original plan, now via a Kubernetes Job or init container.

---

## Data Persistence

Campaign databases use Kubernetes PersistentVolumes backed by the Hetzner Volume:

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

**Note:** The [Hocuspocus Architecture ADR](../archive/plans/2026-03-14-hocuspocus-architecture.md) (now superseded by the [Campaign Collaboration Architecture](./2026-03-25-campaign-collaboration-architecture.md)) established this persistence model: Object Storage is the source of truth for campaign databases, with the Volume serving as a local working cache. Writeback to Object Storage happens every ~30 seconds during active use, not nightly. This model carries forward unchanged. The PV/PVC definitions above remain relevant (the Volume is the local cache), but the nightly CronJob backup is replaced by continuous writeback. The collaboration server changes from Node.js/Hocuspocus to a Rust binary (Axum + kameo actors).

---

## Server Replacement Procedure

Because k3s state lives on the Hetzner Volume at `/data/k3s`:

1. Pulumi provisions new server (Volume + Floating IP survive)
2. cloud-init mounts Volume at `/data`, installs k3s with `--data-dir /data/k3s`
3. k3s starts, reads existing state from Volume
4. All Deployments, Services, Ingresses, certificates resume automatically
5. Traefik (built into k3s) picks up cached certs from k3s state
6. DNS already points at Floating IP — zero DNS changes

**No runbook steps.** No credentials to re-enter, no proxy config to paste, no applications to re-create. The Volume IS the cluster.

---

## What Coolify Gave You (and What Replaces It)

| Coolify feature             | k3s replacement                                                        |
| --------------------------- | ---------------------------------------------------------------------- |
| Dashboard UI                | `kubectl` / Pulumi / k9s (terminal dashboard)                          |
| Application resource config | Kubernetes Deployment + Service + Ingress manifests in Python          |
| SSL cert management         | cert-manager (declarative, in git)                                     |
| Deploy webhook              | `pulumi up` or `kubectl set image` in GHA                              |
| PR previews                 | Namespace-per-PR with wildcard cert                                    |
| Traefik proxy config        | Traefik ships with k3s, configured via Ingress resources (declarative) |
| Backup scheduling           | Kubernetes CronJob                                                     |
| Container registry auth     | Kubernetes Secret (imagePullSecrets)                                   |
| Server monitoring           | Prometheus + Grafana via Helm charts (when needed)                     |

---

## Migration Path — Three Phases

The migration is designed so that each phase is independently valuable and independently revertible. At no point are you committed to completing the next phase.

### Phase 1: Coolify Static Site — Get Something Live (days)

**Goal:** `loreweaver.no` is live and serving the Astro site. No containers you manage, no registry, no webhook. Just Coolify doing the simplest thing it can do.

**Steps:**

1. Provision Hetzner CX23 (hel1) + Floating IP via Pulumi (existing Python project)
2. cloud-init installs Coolify ([Coolify install docs](https://coolify.io/docs/get-started/installation))
3. Access Coolify via SSH tunnel, create admin account
4. Point Coolify at the Astro site repo, let Coolify build and serve it
5. DNS: `loreweaver.no` → Floating IP (manual, bunny.net)
6. SSL: Coolify/Traefik handles via HTTP-01 automatically

**Verification:** `curl -I https://loreweaver.no` — 200 OK, valid SSL.

**What this buys you:** A live marketing site while you learn k3s. No urgency on the k3s timeline. If k3s takes a month, the site is up the whole time.

### Phase 2: k3s on Preview Subdomain — Learn and Build (weeks)

**Goal:** k3s cluster running on a separate server (or alongside Coolify if resources allow), serving at `preview.loreweaver.no` and `*.preview.loreweaver.no`. Full PR preview pipeline working.

**Steps:**

1. **Local first.** Install k3d locally ([k3d.io](https://k3d.io)). Create `infra/pulumi-k8s/` as a new Pulumi Python project. Write all k8s resources (cert-manager, ClusterIssuer, Deployment, Service, Ingress) and iterate against the local k3d cluster until everything converges cleanly.

2. **Provision k3s server.** Either:
    - Second CX23 with its own Floating IP and Volume (clean separation, ~€4/mo extra)
    - Or reuse the Coolify server if it has headroom (saves money, muddier separation)

    cloud-init installs k3s with `--data-dir /data/k3s` ([k3s Advanced Options](https://docs.k3s.io/advanced)).

3. **DNS:** `preview.loreweaver.no` and `*.preview.loreweaver.no` → k3s server's Floating IP (bunny.net).

4. **Deploy to k3s:** `pulumi up --stack prod` in `infra/pulumi-k8s/`. cert-manager issues wildcard cert via DNS-01 + bunny.net. Astro site serves at `preview.loreweaver.no`.

5. **PR preview pipeline.** GHA workflow: on PR open, `kubectl apply` creates namespace + deployment + ingress at `pr-{N}.preview.loreweaver.no`. On PR close, `kubectl delete namespace`. Test with real PRs.

**Verification:**

- `curl -I https://preview.loreweaver.no` — 200 OK, valid wildcard SSL
- Open a PR → preview appears at `pr-{N}.preview.loreweaver.no`
- Close the PR → preview is torn down

**What this buys you:** Full k3s operational experience on a subdomain where failures are invisible to the public. The live site is still on Coolify, untouched.

### Phase 3: Cutover — Decommission Coolify (hours)

**Goal:** k3s serves `loreweaver.no`. Coolify server decommissioned.

**Steps:**

1. Update the Ingress in `infra/pulumi-k8s/` to serve `loreweaver.no` in addition to (or instead of) `preview.loreweaver.no`. `pulumi up --stack prod`.
2. Update DNS: `loreweaver.no` → k3s server's Floating IP.
3. Verify: `curl -I https://loreweaver.no` — 200 OK, cert-manager-issued cert.
4. If anything is wrong: revert DNS to Coolify's Floating IP. Instant rollback.
5. Once confirmed working: decommission Coolify server via Pulumi (`pulumi destroy` on the Coolify stack, or just delete the server in Hetzner console). Release its Floating IP.

**Verification:** Site serves from k3s. PR previews still work. One server, one Volume, one set of manifests in git.

**If Phase 1 Coolify and Phase 2 k3s are on separate servers:** you end up with one server after cutover. The Coolify server was always disposable — it had no persistent state you need to keep.

**If they shared a server:** the cutover is "stop Coolify containers, k3s is already running, update DNS." Even simpler.

---

## Risks and Tradeoffs

| Risk                                | Impact                                            | Mitigation                                                                                                                     |
| ----------------------------------- | ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Kubernetes learning curve           | Slower initial velocity                           | Local k3d loop means fast iteration; basedpyright catches type errors before runtime; Phase 2 is on a non-production subdomain |
| k3s single-node failure             | Site down until server replaced                   | Floating IP + Volume survive; k3s restarts from Volume state; recovery is automated via cloud-init                             |
| cert-manager-webhook-bunny maturity | May have bugs or lag behind cert-manager versions | Fallback: use Lego's built-in bunny provider with cert-manager's generic ACME solver                                           |
| Phase 1 Coolify becomes permanent   | Inertia keeps you on Coolify                      | Phase 2 is on a separate subdomain with no dependency on Phase 1; you can work on it whenever                                  |
| Overengineering for a static site   | Unnecessary complexity for current needs          | Same argument applied to Coolify; the point is learning the machinery on a simple workload                                     |
| k3s version upgrades                | May require careful sequencing                    | k3s supports in-place upgrades via the install script; state on Volume is versioned                                            |

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
- **Coolify installation (Phase 1):** https://coolify.io/docs/get-started/installation
- **Coolify Backup & Restore (why we're leaving):** https://coolify.io/docs/knowledge-base/how-to/backup-restore-coolify
- **Coolify application migration (why we're leaving):** https://coolify.io/docs/knowledge-base/how-to/migrate-apps-different-host
- **Coolify proxy management internals:** https://deepwiki.com/coollabsio/coolify/3.4-proxy-management
- **Coolify CLI (partial coverage):** https://github.com/coollabsio/coolify-cli
- **FluxCD preview environments on k8s (alternative approach):** https://developer-friendly.blog/blog/2025/03/10/how-to-setup-preview-environments-with-fluxcd-in-kubernetes/
- **Docker data-root configuration:** https://docs.docker.com/engine/daemon/
- **Coolify wildcard SSL with bunny.net (proven path):** https://eventuallymaking.io/p/2025-10-coolify-wildcard-ssl
- **Coolify remote server architecture:** https://coolify.io/docs/knowledge-base/server/introduction
