#!/usr/bin/env bash
set -euo pipefail

# Syncs secrets from Scaleway Secrets Manager into Kubernetes Secrets
# for the production namespace. Run during cluster bootstrap and
# whenever a secret value changes (rotation).
#
# Prerequisites:
#   - kubectl configured with cluster access
#   - scw CLI authenticated (SCW_ACCESS_KEY, SCW_SECRET_KEY set)
#   - Scaleway SM secrets populated (see infra/pulumi-cloud/CLAUDE.md)
#
# Secrets synced:
#   1. bunny-api-key        (cert-manager namespace, for DNS-01 ACME)
#   2. internal-bearer       (default namespace, platform + campaign envFrom)
#   3. scaleway-registry     (default namespace, imagePullSecret)
#
# The registry pull secret requires SCW_SECRET_KEY (the Scaleway API
# secret key doubles as the registry password with username "nologin").

REGION="fr-par"
REGISTRY="rg.fr-par.scw.cloud/loreweaver"

fetch_secret() {
  local name="$1"
  local secret_id
  secret_id=$(scw secret secret list "name=${name}" "region=${REGION}" -o json | jq -r '.[0].id')
  if [ "$secret_id" = "null" ] || [ -z "$secret_id" ]; then
    echo "ERROR: Secret '${name}' not found in region ${REGION}" >&2
    exit 1
  fi
  scw secret version access "$secret_id" revision=latest "region=${REGION}" -o json | jq -r '.data' | base64 -d
}

echo "==> Syncing bunny-api-key to cert-manager namespace..."
BUNNY_KEY=$(fetch_secret "bunny-api-key")
kubectl create secret generic bunny-api-key \
  --namespace cert-manager \
  --from-literal="api-key=${BUNNY_KEY}" \
  --dry-run=client -o yaml | kubectl apply -f -

echo "==> Syncing internal-bearer to default namespace..."
BEARER=$(fetch_secret "internal-bearer-prod")
kubectl create secret generic internal-bearer \
  --namespace default \
  --from-literal="INTERNAL_BEARER_PRIMARY=${BEARER}" \
  --dry-run=client -o yaml | kubectl apply -f -

echo "==> Syncing scaleway-registry to default namespace..."
if [ -z "${SCW_SECRET_KEY:-}" ]; then
  echo "ERROR: SCW_SECRET_KEY not set. Required for registry pull secret." >&2
  exit 1
fi

kubectl create secret docker-registry scaleway-registry \
  --namespace default \
  --docker-server="$REGISTRY" \
  --docker-username=nologin \
  --docker-password="$SCW_SECRET_KEY" \
  --dry-run=client -o yaml | kubectl apply -f -

echo "==> All secrets synced."
