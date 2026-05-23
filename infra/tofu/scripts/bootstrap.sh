#!/usr/bin/env bash
# One-time bootstrap: creates the Scaleway Object Storage bucket for OpenTofu
# state, stores the encryption passphrase and hcloud token in Scaleway Secrets
# Manager, and generates a .envrc via setup.sh.
#
# Prerequisites:
#   - scw CLI installed and configured (scw init)
#   - jq installed
#   - direnv installed
#
# Usage: ./scripts/bootstrap.sh

set -euo pipefail

for tool in jq scw direnv; do
    if ! command -v "$tool" > /dev/null 2>&1; then
        echo "Error: ${tool} is not installed." >&2
        exit 1
    fi
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

BUCKET_NAME="familiar-systems-tofu-state"
REGION="fr-par"
PASSPHRASE_SECRET="tofu-config-passphrase"
HCLOUD_SECRET="hcloud-api-token"

# --- State bucket ---
echo "==> Creating Object Storage bucket: ${BUCKET_NAME} (${REGION})"
if scw object bucket list region="${REGION}" -o json | jq -e ".[] | select(.Name == \"${BUCKET_NAME}\")" > /dev/null 2>&1; then
    echo "    Bucket already exists, skipping."
else
    scw object bucket create name="${BUCKET_NAME}" region="${REGION}"
    echo "    Bucket created."
fi

# --- Encryption passphrase ---
echo "==> Setting up passphrase secret: ${PASSPHRASE_SECRET}"
if scw secret secret list name="${PASSPHRASE_SECRET}" region="${REGION}" -o json | jq -e '.[0]' > /dev/null 2>&1; then
    SECRET_ID=$(scw secret secret list name="${PASSPHRASE_SECRET}" region="${REGION}" -o json | jq -r '.[0].id')
    echo "    Secret already exists: ${SECRET_ID}"
else
    PASSPHRASE=$(openssl rand -base64 32)
    SECRET_ID=$(scw secret secret create name="${PASSPHRASE_SECRET}" region="${REGION}" -o json | jq -r '.id')
    scw secret version create secret-id="${SECRET_ID}" region="${REGION}" data="${PASSPHRASE}"
    echo "    Secret created: ${SECRET_ID}"
    echo ""
    echo "    IMPORTANT: Save this passphrase in your password manager as backup:"
    echo "    ${PASSPHRASE}"
    echo ""
fi

# --- hcloud API token ---
echo "==> Setting up hcloud token secret: ${HCLOUD_SECRET}"
if scw secret secret list name="${HCLOUD_SECRET}" region="${REGION}" -o json | jq -e '.[0]' > /dev/null 2>&1; then
    HCLOUD_SECRET_ID=$(scw secret secret list name="${HCLOUD_SECRET}" region="${REGION}" -o json | jq -r '.[0].id')
    echo "    Secret already exists: ${HCLOUD_SECRET_ID}"
else
    HCLOUD_SECRET_ID=$(scw secret secret create name="${HCLOUD_SECRET}" region="${REGION}" \
        description="Hetzner Cloud API token" protected=true -o json | jq -r '.id')
    echo "    Secret container created: ${HCLOUD_SECRET_ID}"
    echo ""
    echo "    You must now fill this secret with your Hetzner Cloud API token:"
    echo "    scw secret version create secret-id=${HCLOUD_SECRET_ID} region=${REGION} data=<your-hcloud-token>"
    echo ""
fi

echo ""
echo "==> Bootstrap complete!"
echo "    Bucket:     s3://${BUCKET_NAME} (${REGION})"
echo "    Passphrase: ${PASSPHRASE_SECRET}"
echo "    hcloud:     ${HCLOUD_SECRET}"

# Generate .envrc via setup script
echo ""
exec "${SCRIPT_DIR}/setup.sh"
