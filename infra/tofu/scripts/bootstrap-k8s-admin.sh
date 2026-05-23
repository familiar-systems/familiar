#!/usr/bin/env bash
# bootstrap-k8s-admin.sh -- Create cluster-side RBAC for the IaC k8s provider.
#
# Creates a `pulumi-admin` ServiceAccount + ClusterRoleBinding (cluster-admin) +
# long-lived token Secret on the k3s cluster, captures the populated token and
# the cluster CA, and writes both to Scaleway Secrets Manager. Then constructs
# a new token-based kubeconfig and pushes it as a new version of the existing
# `k3s-kubeconfig` SM secret.
#
# When to run:
#   - Once, during initial cluster bootstrap.
#   - Again, only if you deliberately rotate the SA token (rare; planned event).
#   - Again, after wiping k3s (the new SA needs to be registered
#     against the rebuilt cluster).
#   - Idempotent: re-running on a healthy cluster writes a new SA token version
#     into SM, replacing nothing else. Safe.
#
# Prerequisites:
#   - `scw` CLI authenticated (~/.config/scw/config.yaml)
#   - `kubectl`, `jq`, and `tofu` available
#   - direnv loaded (.envrc sourced) so `tofu output` works for the IP.
#
# Usage:
#   ./scripts/bootstrap-k8s-admin.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${PROJECT_DIR}"

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------
REGION="fr-par"
SA_NAME="pulumi-admin"
SA_NAMESPACE="kube-system"
TOKEN_SECRET_NAME="pulumi-admin-token"
CRB_NAME="pulumi-admin-cluster-admin"
KUBECONFIG_SECRET_NAME="k3s-kubeconfig"
ADMIN_TOKEN_SECRET_NAME="k3s-pulumi-admin-token"
CLUSTER_CA_SECRET_NAME="k3s-cluster-ca"

TMPDIR_LOCAL=$(mktemp -d -t k8s-admin-bootstrap.XXXXXX)
trap 'rm -rf "${TMPDIR_LOCAL}"' EXIT

# ---------------------------------------------------------------------------
# Pre-flight checks
# ---------------------------------------------------------------------------
echo "==> Pre-flight checks..."
for tool in scw kubectl jq tofu; do
    if ! command -v "${tool}" >/dev/null 2>&1; then
        echo "ERROR: '${tool}' not found in PATH" >&2
        exit 1
    fi
done

if ! scw secret secret list region="${REGION}" -o json >/dev/null 2>&1; then
    echo "ERROR: scw CLI cannot list secrets in ${REGION}. Check 'scw init' / credentials." >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# Read the floating IP and the existing kubeconfig secret ID
# ---------------------------------------------------------------------------
echo "==> Reading OpenTofu outputs..."
FLOATING_IP=$(tofu output -raw k3s_floating_ip 2>/dev/null) || {
    echo "ERROR: Can't read k3s_floating_ip from OpenTofu. Is .envrc sourced and tofu init run?" >&2
    exit 1
}
echo "    Floating IP: ${FLOATING_IP}"

KUBECONFIG_SECRET_ID=$(tofu output -raw k3s_kubeconfig_secret_id 2>/dev/null | cut -d/ -f2) || {
    echo "ERROR: Can't read k3s_kubeconfig_secret_id from OpenTofu." >&2
    exit 1
}
echo "    Kubeconfig SM secret ID: ${KUBECONFIG_SECRET_ID}"

# ---------------------------------------------------------------------------
# Fetch current kubeconfig from SM
# ---------------------------------------------------------------------------
echo "==> Fetching current kubeconfig from Scaleway SM..."
CURRENT_KUBECONFIG="${TMPDIR_LOCAL}/current-kubeconfig.yaml"
scw secret version access "${KUBECONFIG_SECRET_ID}" revision=latest region="${REGION}" -o json \
    | jq -r '.data' \
    | base64 -d > "${CURRENT_KUBECONFIG}"
chmod 600 "${CURRENT_KUBECONFIG}"

if KUBECONFIG="${CURRENT_KUBECONFIG}" kubectl get nodes >/dev/null 2>&1; then
    echo "    Current kubeconfig works."
else
    echo "    SM kubeconfig is stale (expected after k3s wipe)."
    echo "    Falling back to SSH + k3s local kubeconfig..."
    ssh -o StrictHostKeyChecking=accept-new "root@${FLOATING_IP}" cat /etc/rancher/k3s/k3s.yaml \
        | sed "s|127.0.0.1|${FLOATING_IP}|g" > "${CURRENT_KUBECONFIG}"
    chmod 600 "${CURRENT_KUBECONFIG}"
    if ! KUBECONFIG="${CURRENT_KUBECONFIG}" kubectl get nodes >/dev/null 2>&1; then
        echo "ERROR: SSH fallback kubeconfig also failed. Cluster unreachable." >&2
        exit 1
    fi
    echo "    SSH fallback kubeconfig works."
fi

# ---------------------------------------------------------------------------
# Apply the SA + ClusterRoleBinding + token Secret manifest
# ---------------------------------------------------------------------------
echo "==> Applying ${SA_NAME} ServiceAccount, ClusterRoleBinding, and token Secret..."
KUBECONFIG="${CURRENT_KUBECONFIG}" kubectl apply -f - <<EOF
apiVersion: v1
kind: ServiceAccount
metadata:
  name: ${SA_NAME}
  namespace: ${SA_NAMESPACE}
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: ${CRB_NAME}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: cluster-admin
subjects:
  - kind: ServiceAccount
    name: ${SA_NAME}
    namespace: ${SA_NAMESPACE}
---
apiVersion: v1
kind: Secret
metadata:
  name: ${TOKEN_SECRET_NAME}
  namespace: ${SA_NAMESPACE}
  annotations:
    kubernetes.io/service-account.name: ${SA_NAME}
type: kubernetes.io/service-account-token
EOF

# ---------------------------------------------------------------------------
# Wait for the controller to populate the token Secret
# ---------------------------------------------------------------------------
echo "==> Waiting for service-account-token controller to populate the Secret..."
for _ in $(seq 1 30); do
    TOKEN=$(KUBECONFIG="${CURRENT_KUBECONFIG}" kubectl get secret "${TOKEN_SECRET_NAME}" \
        -n "${SA_NAMESPACE}" -o jsonpath='{.data.token}' 2>/dev/null || true)
    if [[ -n "${TOKEN}" ]]; then
        break
    fi
    sleep 1
done

if [[ -z "${TOKEN:-}" ]]; then
    echo "ERROR: Token Secret was not populated after 30s." >&2
    exit 1
fi
echo "    Token Secret populated."

# ---------------------------------------------------------------------------
# Read the populated token and CA cert
# ---------------------------------------------------------------------------
SA_TOKEN=$(KUBECONFIG="${CURRENT_KUBECONFIG}" kubectl get secret "${TOKEN_SECRET_NAME}" \
    -n "${SA_NAMESPACE}" -o jsonpath='{.data.token}' | base64 -d)

CLUSTER_CA_B64=$(KUBECONFIG="${CURRENT_KUBECONFIG}" kubectl get secret "${TOKEN_SECRET_NAME}" \
    -n "${SA_NAMESPACE}" -o jsonpath='{.data.ca\.crt}')

if [[ -z "${SA_TOKEN}" || -z "${CLUSTER_CA_B64}" ]]; then
    echo "ERROR: Failed to read token or CA from the Secret." >&2
    exit 1
fi
echo "    Captured SA token (length: ${#SA_TOKEN}) and CA (base64 length: ${#CLUSTER_CA_B64})."

# ---------------------------------------------------------------------------
# Build new token-based kubeconfig
# ---------------------------------------------------------------------------
echo "==> Building new token-based kubeconfig..."
NEW_KUBECONFIG="${TMPDIR_LOCAL}/new-kubeconfig.yaml"
cat > "${NEW_KUBECONFIG}" <<EOF
apiVersion: v1
kind: Config
clusters:
  - name: k3s-loreweaver
    cluster:
      server: https://${FLOATING_IP}:6443
      certificate-authority-data: ${CLUSTER_CA_B64}
contexts:
  - name: default
    context:
      cluster: k3s-loreweaver
      user: ${SA_NAME}
current-context: default
users:
  - name: ${SA_NAME}
    user:
      token: ${SA_TOKEN}
EOF
chmod 600 "${NEW_KUBECONFIG}"

if ! KUBECONFIG="${NEW_KUBECONFIG}" kubectl get nodes >/dev/null 2>&1; then
    echo "ERROR: New token-based kubeconfig does not authenticate." >&2
    exit 1
fi
echo "    New kubeconfig authenticates successfully."

# ---------------------------------------------------------------------------
# Push to Scaleway SM
# ---------------------------------------------------------------------------
ensure_secret() {
    local name="$1"
    local description="${2:-}"
    local existing
    existing=$(scw secret secret list name="${name}" region="${REGION}" -o json 2>/dev/null \
        | jq -r '.[0].id // empty')

    if [[ -n "${existing}" ]]; then
        echo "${existing}"
        return 0
    fi

    if [[ -n "${description}" ]]; then
        scw secret secret create name="${name}" description="${description}" region="${REGION}" -o json 2>/dev/null \
            | jq -r '.id'
    else
        scw secret secret create name="${name}" region="${REGION}" -o json 2>/dev/null \
            | jq -r '.id'
    fi
}

echo "==> Pushing SA token to SM (${ADMIN_TOKEN_SECRET_NAME})..."
ADMIN_TOKEN_SECRET_ID=$(ensure_secret "${ADMIN_TOKEN_SECRET_NAME}" \
    "k3s pulumi-admin ServiceAccount bearer token (cluster-admin)")
TOKEN_FILE="${TMPDIR_LOCAL}/sa-token.txt"
printf '%s' "${SA_TOKEN}" > "${TOKEN_FILE}"
chmod 600 "${TOKEN_FILE}"
scw secret version create "${ADMIN_TOKEN_SECRET_ID}" data=@"${TOKEN_FILE}" region="${REGION}" >/dev/null
echo "    Pushed (secret ID: ${ADMIN_TOKEN_SECRET_ID})."

echo "==> Pushing cluster CA to SM (${CLUSTER_CA_SECRET_NAME})..."
CA_SECRET_ID=$(ensure_secret "${CLUSTER_CA_SECRET_NAME}" \
    "k3s cluster CA cert (base64 PEM)")
CA_FILE="${TMPDIR_LOCAL}/cluster-ca.b64"
printf '%s' "${CLUSTER_CA_B64}" > "${CA_FILE}"
chmod 600 "${CA_FILE}"
scw secret version create "${CA_SECRET_ID}" data=@"${CA_FILE}" region="${REGION}" >/dev/null
echo "    Pushed (secret ID: ${CA_SECRET_ID})."

echo "==> Pushing new kubeconfig to SM (${KUBECONFIG_SECRET_NAME})..."
scw secret version create "${KUBECONFIG_SECRET_ID}" data=@"${NEW_KUBECONFIG}" region="${REGION}" >/dev/null
echo "    Pushed."

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------
cat <<EOF

==> Bootstrap complete.

Cluster state:
  - ServiceAccount/${SA_NAME} in ${SA_NAMESPACE}
  - ClusterRoleBinding/${CRB_NAME} (cluster-admin)
  - Secret/${TOKEN_SECRET_NAME} (type: service-account-token)

Scaleway SM state:
  - ${ADMIN_TOKEN_SECRET_NAME} (id: ${ADMIN_TOKEN_SECRET_ID})
  - ${CLUSTER_CA_SECRET_NAME} (id: ${CA_SECRET_ID})
  - ${KUBECONFIG_SECRET_NAME} (id: ${KUBECONFIG_SECRET_ID})
EOF
