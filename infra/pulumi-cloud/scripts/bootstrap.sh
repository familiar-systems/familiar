#!/usr/bin/env bash
# One-time bootstrap: creates the Scaleway Object Storage bucket, stores the
# Pulumi passphrase in Scaleway Secrets Manager, and generates a .envrc that
# pulls everything together at cd-time via direnv.
#
# Prerequisites:
#   - scw CLI installed and configured (scw init)
#   - jq installed
#   - direnv installed
#
# Usage: ./scripts/bootstrap.sh

set -euo pipefail

if ! command -v jq > /dev/null 2>&1; then
    echo "Error: jq is not installed. Install via 'brew install jq' or your package manager." >&2
    exit 1
fi

if ! command -v scw > /dev/null 2>&1; then
    echo "Error: scw is not installed. Install via 'brew install scw' or see https://github.com/scaleway/scaleway-cli" >&2
    exit 1
fi

if ! command -v direnv > /dev/null 2>&1; then
    echo "Error: direnv is not installed. Install via 'brew install direnv' or see https://direnv.net" >&2
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

BUCKET_NAME="loreweaver-pulumi-state"
REGION="fr-par"
SECRET_NAME="pulumi-config-passphrase"
S3_ENDPOINT="s3.${REGION}.scw.cloud"

# --- Bucket ---
echo "==> Creating Object Storage bucket: ${BUCKET_NAME} (${REGION})"
if scw object bucket list region="${REGION}" -o json | jq -e ".[] | select(.Name == \"${BUCKET_NAME}\")" > /dev/null 2>&1; then
    echo "    Bucket already exists, skipping."
else
    scw object bucket create name="${BUCKET_NAME}" region="${REGION}"
    echo "    Bucket created."
fi

# --- Passphrase secret ---
echo "==> Setting up passphrase secret: ${SECRET_NAME}"
if scw secret secret list name="${SECRET_NAME}" region="${REGION}" -o json | jq -e '.[0]' > /dev/null 2>&1; then
    SECRET_ID=$(scw secret secret list name="${SECRET_NAME}" region="${REGION}" -o json | jq -r '.[0].id')
    echo "    Secret already exists: ${SECRET_ID}"
else
    PASSPHRASE=$(openssl rand -base64 32)
    SECRET_ID=$(scw secret secret create name="${SECRET_NAME}" region="${REGION}" -o json | jq -r '.id')
    scw secret version create secret-id="${SECRET_ID}" region="${REGION}" data="${PASSPHRASE}"
    echo "    Secret created: ${SECRET_ID}"
    echo ""
    echo "    IMPORTANT: Save this passphrase in your password manager as backup:"
    echo "    ${PASSPHRASE}"
    echo ""
fi

echo ""
echo "==> Bootstrap complete!"
echo "    Bucket:  s3://${BUCKET_NAME} (${REGION})"
echo "    Secret:  ${SECRET_NAME} (${SECRET_ID})"

# Generate .envrc via setup script
echo ""
exec "${SCRIPT_DIR}/setup.sh"
