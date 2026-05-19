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

collect_k8s_urns() {
    local state
    state=$(pulumi stack export) || {
        echo "ERROR: Failed to export Pulumi state." >&2
        exit 1
    }
    echo "${state}" | jq -r '
        .deployment.resources[]
        | select(
            (.type | startswith("kubernetes:"))
            or (.type == "pulumi:providers:kubernetes")
            or (.type == "command:remote:Command" and (.urn | contains("kubeconfig")))
            or (.urn | contains("kubeconfig-version"))
        )
        | .urn
    '
}

echo "==> Scanning Pulumi state for k8s resources..."
INITIAL_URNS=$(collect_k8s_urns)

if [[ -z "${INITIAL_URNS}" ]]; then
    echo "No k8s resources found in Pulumi state. Nothing to do."
    exit 0
fi

echo "==> Found resources to remove:"
while IFS= read -r urn; do
    echo "    ${urn##*::}"
done <<< "${INITIAL_URNS}"

if [[ -t 0 ]]; then
    echo ""
    read -rp "Proceed with state cleanup? [y/N] " confirm
    if [[ "${confirm}" != [yY] ]]; then
        echo "Aborted."
        exit 1
    fi
else
    echo ""
    echo "Non-interactive mode, proceeding..."
fi

# Loop until no k8s resources remain. Each pass re-exports fresh state
# because deleting a resource can invalidate URNs from the prior snapshot
# (parent-child relationships, Helm release children, provider dependencies).
TOTAL_REMOVED=0
PASS=0
while true; do
    URNS=$(collect_k8s_urns)
    if [[ -z "${URNS}" ]]; then
        break
    fi

    ((PASS++)) || true
    if [[ ${PASS} -gt 5 ]]; then
        echo "ERROR: Still have k8s resources after 5 passes. Remaining:" >&2
        echo "${URNS}" | while IFS= read -r urn; do echo "    ${urn##*::}"; done >&2
        exit 1
    fi

    [[ ${PASS} -gt 1 ]] && echo "==> Pass ${PASS} (re-scanning after state changes)..."

    while IFS= read -r urn; do
        name="${urn##*::}"
        if pulumi state delete --force --target-dependents -y "${urn}"; then
            echo "  Removed: ${name}"
            ((TOTAL_REMOVED++)) || true
        fi
    done <<< "${URNS}"
done

echo ""
echo "==> Done. Removed ${TOTAL_REMOVED} resources in ${PASS} pass(es)."
