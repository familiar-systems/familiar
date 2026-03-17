# Local k8s Smoke Test

Manual smoke test for the Kubernetes resources in `../pulumi-cloud/k8s.py`. Uses k3d to run a local k3s cluster and Pulumi to deploy everything against it.

This is NOT CI. It's a runbook for validating changes to k8s resources before deploying to the real cluster.

## Prerequisites

- Docker
- k3d (`brew install k3d`)
- kubectl (`brew install kubectl`)
- helm (`brew install helm`)
- Pulumi CLI (`brew install pulumi`)

## Setup (one-time)

```bash
cd infra/test-k8s

# Install dependencies (pulls in k8s.py etc. from ../pulumi-cloud)
uv sync

# Create a local k3s cluster
k3d cluster create loreweaver-test --wait

# Build the site image and import it into k3d
# (from repo root)
docker build -f apps/site/Dockerfile -t rg.fr-par.scw.cloud/loreweaver/site:latest .
k3d image import rg.fr-par.scw.cloud/loreweaver/site:latest -c loreweaver-test

# Initialize Pulumi with local state (no cloud backend)
pulumi login --local
export PULUMI_CONFIG_PASSPHRASE=test
pulumi stack init test

# Configure the stack
k3d kubeconfig get loreweaver-test | pulumi config set --secret k3d-kubeconfig --
pulumi config set --secret bunny-api-key fake-test-key
pulumi config set acme-email test@example.com
export SCW_SECRET_KEY=noop  # dummy value for imagePullSecret
```

## Run the test

```bash
export PULUMI_CONFIG_PASSPHRASE=test
pulumi up --yes
```

All 12 resources should create successfully in ~30 seconds. Expected output:
- cert-manager + webhook-bunny: all pods Running
- ClusterIssuer: created but `READY=False` (no real DNS)
- Certificate: created but `READY=False` (no real DNS)
- Site Deployment: pod Running, serving nginx
- Ingress: configured for `preview.loreweaver.no`

## Verify the site is serving

```bash
kubectl port-forward svc/site 8081:80
# In another terminal:
curl -I http://localhost:8081
# Should return 200 OK with nginx
```

## Verify cert-manager resources

```bash
kubectl get clusterissuer,certificate,ingress -A
```

## Teardown

```bash
export PULUMI_CONFIG_PASSPHRASE=test
pulumi destroy --yes
pulumi stack rm test --yes
k3d cluster delete loreweaver-test
```

## What this catches

- Helm chart version compatibility (cert-manager, webhook-bunny)
- CRD schema acceptance (ClusterIssuer, Certificate)
- Deployment spec correctness (image pull, resource limits, selectors)
- Service/Ingress wiring (ports, paths, TLS config)
- Pulumi resource ordering (depends_on chains)

## What this does NOT catch

- Actual TLS cert issuance (needs real bunny.net DNS)
- Registry authentication (uses dummy credentials)
- Cloud-init / k3s server provisioning (Hetzner-specific)
- DNS resolution for preview.loreweaver.no
