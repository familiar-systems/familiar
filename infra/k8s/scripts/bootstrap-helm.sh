#!/usr/bin/env bash
set -euo pipefail

# Installs (or upgrades) cert-manager and the bunny.net DNS-01 webhook
# solver on the k3s cluster. Run once during cluster bootstrap, or when
# upgrading chart versions.
#
# Prerequisites:
#   - kubectl configured with cluster access
#   - helm v3 installed
#   - bunny-api-key k8s Secret created in cert-manager namespace
#     (run sync-prod-secrets.sh first)
#
# The cert-manager ServiceAccount name is needed by webhook-bunny for
# RBAC. Helm suffixes release names, so we discover it dynamically
# after the cert-manager install.

CERT_MANAGER_VERSION="v1.17.2"
WEBHOOK_BUNNY_VERSION="1.0.3"
CERT_MANAGER_NS="cert-manager"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HELM_DIR="${SCRIPT_DIR}/../helm"

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

echo "==> Done. cert-manager and webhook-bunny installed."
