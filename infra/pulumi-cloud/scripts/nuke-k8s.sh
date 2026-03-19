#!/usr/bin/env bash
# nuke-k8s.sh -- Emergency recovery when Pulumi can't reach the k8s API.
#
# Removes all Kubernetes resources from Pulumi state so `pulumi up` can
# recreate them from scratch. Optionally wipes the k3s datastore on the
# server's Volume first (for corrupted k3s state).
#
# When to use:
#   `pulumi up` fails with "grpc: the client connection is closing" or
#   similar provider errors after a server replacement or k3s crash.
#
# Usage:
#   ./scripts/nuke-k8s.sh               # Show help
#   ./scripts/nuke-k8s.sh --state-only  # Pulumi state cleanup only
#   ./scripts/nuke-k8s.sh --wipe-k3s    # Wipe k3s + state cleanup
#
# After running:
#   pulumi up    # Recreates all k8s resources from scratch
#
# Why not just destroy the server?
#   k3s state lives on the Hetzner Volume at /data/k3s. Destroying the
#   server doesn't clear it -- the new server mounts the same Volume and
#   inherits the corruption. The Volume also holds /data/campaigns and
#   /data/preview which must NOT be lost. Wiping /data/k3s is surgical.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${PROJECT_DIR}"


show_help() {
    echo "nuke-k8s.sh -- Emergency k8s recovery"
    echo ""
    echo "Usage: ./scripts/nuke-k8s.sh <--state-only | --wipe-k3s>"
    echo ""
    echo "  --state-only   Remove k8s resources from Pulumi state"
    echo "                 Use when: server and k3s are fine, but pulumi up"
    echo "                 fails with gRPC errors after a provider cascade."
    echo ""
    echo "  --wipe-k3s     Wipe /data/k3s, reinstall k3s, then clean state"
    echo "                 Use when: k3s itself is corrupted (stuck namespaces,"
    echo "                 orphaned CRDs, finalizer deadlocks)."
    echo ""
    echo "After running either mode:"
    echo "  pulumi up      # Recreates all k8s resources from scratch"
}

WIPE_K3S=false
case "${1:-}" in
    --state-only)   WIPE_K3S=false ;;
    --wipe-k3s)     WIPE_K3S=true ;;
    *)
        show_help
        exit 0
        ;;
esac

# ---------------------------------------------------------------------------
# Phase 1 (optional): Wipe k3s on the server
# ---------------------------------------------------------------------------
if [[ "${WIPE_K3S}" == true ]]; then
    echo ""
    echo "==> Reading server IP from Pulumi outputs..."
    FIP=$(pulumi stack output k3s_floating_ip 2>/dev/null) || {
        echo "ERROR: Can't read k3s_floating_ip. If the server is gone, use --state-only." >&2
        exit 1
    }

    echo "==> Wiping k3s on ${FIP}..."
    echo "    This will:"
    echo "      - Uninstall k3s"
    echo "      - Delete /data/k3s (Volume mount)"
    echo "      - Reinstall k3s fresh"
    echo "    Campaign data (/data/campaigns, /data/preview) is NOT touched."
    echo ""
    read -rp "    Proceed with k3s wipe? [y/N] " confirm
    if [[ "${confirm}" != [yY] ]]; then
        echo "Aborted."
        exit 1
    fi

    # shellcheck disable=SC2087
    ssh -o StrictHostKeyChecking=accept-new "root@${FIP}" bash -s "${FIP}" <<'REMOTE'
        set -euo pipefail
        FIP="$1"

        echo "  Stopping k3s..."
        systemctl stop k3s 2>/dev/null || true

        echo "  Uninstalling k3s..."
        /usr/local/bin/k3s-uninstall.sh 2>/dev/null || true

        echo "  Wiping /data/k3s..."
        rm -rf /data/k3s
        mkdir -p /data/k3s

        echo "  Reinstalling k3s..."
        curl -sfL https://get.k3s.io | \
            INSTALL_K3S_EXEC="--tls-san ${FIP} --data-dir /data/k3s --node-external-ip ${FIP}" sh -

        echo "  Waiting for k3s API..."
        timeout 120 bash -c 'until kubectl get nodes >/dev/null 2>&1; do sleep 2; done'
        echo "  k3s is ready."
REMOTE

    echo "==> k3s wipe complete."
    echo ""
fi

# ---------------------------------------------------------------------------
# Phase 2: Remove k8s resources from Pulumi state
# ---------------------------------------------------------------------------
echo "==> Exporting Pulumi state..."
STATE=$(pulumi stack export) || {
    echo "ERROR: Failed to export Pulumi state." >&2
    exit 1
}

# Collect URNs in deletion order:
#   1. k8s resources (workloads, secrets, namespaces, CRDs, Helm releases)
#   2. k8s provider
#   3. kubeconfig command + Scaleway secret version (so both get recreated)
K8S_URNS=$(echo "${STATE}" | jq -r '
    .deployment.resources[]
    | select(.type | startswith("kubernetes:"))
    | .urn
')

PROVIDER_URNS=$(echo "${STATE}" | jq -r '
    .deployment.resources[]
    | select(.type == "pulumi:providers:kubernetes")
    | .urn
')

KUBECONFIG_URNS=$(echo "${STATE}" | jq -r '
    .deployment.resources[]
    | select(
        (.type == "command:remote:Command" and (.urn | contains("kubeconfig")))
        or (.urn | contains("kubeconfig-version"))
    )
    | .urn
')

# Merge into ordered array
ALL_URNS=()
while IFS= read -r urn; do [[ -n "${urn}" ]] && ALL_URNS+=("${urn}"); done <<< "${K8S_URNS}"
while IFS= read -r urn; do [[ -n "${urn}" ]] && ALL_URNS+=("${urn}"); done <<< "${PROVIDER_URNS}"
while IFS= read -r urn; do [[ -n "${urn}" ]] && ALL_URNS+=("${urn}"); done <<< "${KUBECONFIG_URNS}"

if [[ ${#ALL_URNS[@]} -eq 0 ]]; then
    echo "No k8s resources found in Pulumi state. Nothing to do."
    exit 0
fi

echo "==> Found ${#ALL_URNS[@]} resources to remove from state:"
for urn in "${ALL_URNS[@]}"; do
    # Print just the resource name (last segment of URN)
    echo "    ${urn##*::}"
done

echo ""
read -rp "Proceed with state cleanup? [y/N] " confirm
if [[ "${confirm}" != [yY] ]]; then
    echo "Aborted."
    exit 1
fi

FAILED=0
for urn in "${ALL_URNS[@]}"; do
    name="${urn##*::}"
    if pulumi state delete --force -y "${urn}" 2>/dev/null; then
        echo "  Removed: ${name}"
    else
        echo "  Skipped: ${name} (not found or already removed)"
        ((FAILED++)) || true
    fi
done

echo ""
echo "==> Done. Removed $((${#ALL_URNS[@]} - FAILED))/${#ALL_URNS[@]} resources."
echo ""
echo "Next step:"
echo "  pulumi up    # Recreates k8s provider, kubeconfig, and all k8s resources"
