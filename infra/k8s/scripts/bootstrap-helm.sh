#!/usr/bin/env bash
set -euo pipefail

# Installs (or upgrades) cert-manager, the bunny.net DNS-01 webhook
# solver, and External Secrets Operator on the k3s cluster. Run once
# during cluster bootstrap, or when upgrading chart versions.
#
# Prerequisites:
#   - kubectl configured with cluster access
#   - helm v3 installed
#   - scw CLI authenticated (SCW_ACCESS_KEY, SCW_SECRET_KEY set)
#   - OpenTofu has run at least once (creates the ESO credential and
#     registry-pull-credential SM secrets)

CERT_MANAGER_VERSION="v1.17.2"
WEBHOOK_BUNNY_VERSION="1.0.3"
ESO_VERSION="0.17.0"
CERT_MANAGER_NS="cert-manager"
ESO_NS="external-secrets"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HELM_DIR="${SCRIPT_DIR}/../helm"
ESO_DIR="${SCRIPT_DIR}/../eso"
REGION="fr-par"

# ── cert-manager ────────────────────────────────────────

echo "==> Creating cert-manager namespace..."
kubectl create namespace "$CERT_MANAGER_NS" --dry-run=client -o yaml | kubectl apply -f -

echo "==> Installing cert-manager ${CERT_MANAGER_VERSION}..."
helm upgrade --install cert-manager cert-manager \
  --repo https://charts.jetstack.io \
  --version "$CERT_MANAGER_VERSION" \
  --namespace "$CERT_MANAGER_NS" \
  --values "${HELM_DIR}/cert-manager-values.yaml" \
  --wait --timeout 5m

# Discover the cert-manager ServiceAccount name (Helm may suffix it)
CM_SA=$(kubectl get serviceaccount -n "$CERT_MANAGER_NS" \
  -l app.kubernetes.io/name=cert-manager \
  -o jsonpath='{.items[0].metadata.name}')
echo "    cert-manager ServiceAccount: ${CM_SA}"

echo "==> Installing cert-manager-webhook-bunny ${WEBHOOK_BUNNY_VERSION}..."
helm upgrade --install cert-manager-webhook-bunny cert-manager-webhook-bunny \
  --repo https://davidhidvegi.github.io/cert-manager-webhook-bunny/charts/ \
  --version "$WEBHOOK_BUNNY_VERSION" \
  --namespace "$CERT_MANAGER_NS" \
  --values "${HELM_DIR}/webhook-bunny-values.yaml" \
  --set "certManager.serviceAccountName=${CM_SA}" \
  --wait --timeout 5m

# ── External Secrets Operator ───────────────────────────

echo "==> Installing External Secrets Operator ${ESO_VERSION}..."
helm upgrade --install external-secrets external-secrets \
  --repo https://charts.external-secrets.io \
  --version "$ESO_VERSION" \
  --namespace "$ESO_NS" --create-namespace \
  --values "${HELM_DIR}/external-secrets-values.yaml" \
  --wait --timeout 5m

echo "==> Fetching ESO credentials from Scaleway SM..."
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

ESO_CRED_JSON=$(fetch_secret "eso-scaleway-credential")
ESO_ACCESS_KEY=$(echo "$ESO_CRED_JSON" | jq -r '.access_key')
ESO_SECRET_KEY=$(echo "$ESO_CRED_JSON" | jq -r '.secret_key')

echo "==> Creating ESO bootstrap secret..."
kubectl create secret generic eso-scaleway-credentials \
  --namespace "$ESO_NS" \
  --from-literal="access-key=${ESO_ACCESS_KEY}" \
  --from-literal="secret-key=${ESO_SECRET_KEY}" \
  --dry-run=client -o yaml | kubectl apply -f -

echo "==> Applying ClusterSecretStore..."
SCW_PROJECT_ID=$(scw config get default-project-id)
export SCW_PROJECT_ID
envsubst < "${ESO_DIR}/cluster-secret-store.yaml" | kubectl apply -f -

echo "==> Applying bunny-api-key ExternalSecret..."
kubectl apply -f "${ESO_DIR}/bunny-api-key.yaml"

echo "==> Done. cert-manager, webhook-bunny, and ESO installed."
