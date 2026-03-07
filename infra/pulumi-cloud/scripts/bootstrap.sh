#!/usr/bin/env bash
# One-time bootstrap: creates the Scaleway Object Storage bucket and
# Secrets Manager secret for the Pulumi passphrase.
#
# Prerequisites:
#   - scw CLI installed and configured (scw init)
#   - jq installed
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

BUCKET_NAME="loreweaver-pulumi-state"
REGION="fr-par"
SECRET_NAME="pulumi-config-passphrase"

echo "==> Creating Object Storage bucket: ${BUCKET_NAME} (${REGION})"
if scw object bucket list region="${REGION}" -o json | jq -e ".[] | select(.name == \"${BUCKET_NAME}\")" > /dev/null 2>&1; then
    echo "    Bucket already exists, skipping."
else
    scw object bucket create name="${BUCKET_NAME}" region="${REGION}"
    echo "    Bucket created."
fi

echo "==> Generating Pulumi passphrase"
PASSPHRASE=$(openssl rand -base64 32)

echo "==> Creating secret: ${SECRET_NAME}"
if scw secret secret list name="${SECRET_NAME}" region="${REGION}" -o json | jq -e '.[0]' > /dev/null 2>&1; then
    echo "    Secret already exists. Creating new version."
    SECRET_ID=$(scw secret secret list name="${SECRET_NAME}" region="${REGION}" -o json | jq -r '.[0].id')
else
    SECRET_ID=$(scw secret secret create name="${SECRET_NAME}" region="${REGION}" -o json | jq -r '.id')
    echo "    Secret created: ${SECRET_ID}"
fi

# Store the passphrase as a secret version
echo -n "${PASSPHRASE}" | scw secret version create secret-id="${SECRET_ID}" region="${REGION}" data=-
echo "    Passphrase stored as secret version."

echo ""
echo "==> Bootstrap complete!"
echo "    Bucket:     s3://${BUCKET_NAME} (${REGION})"
echo "    Secret:     ${SECRET_NAME} (${SECRET_ID})"
echo ""
echo "    IMPORTANT: Save this passphrase in your password manager as backup:"
echo "    ${PASSPHRASE}"
echo ""
echo "    Next steps:"
echo "      1. Run 'direnv allow' in infra/pulumi-cloud/"
echo "      2. Run 'pulumi stack init prod'"
