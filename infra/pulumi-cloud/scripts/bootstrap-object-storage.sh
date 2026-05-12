#!/usr/bin/env bash
# bootstrap-object-storage.sh -- Create + populate Scaleway SM secrets for
# Hetzner Object Storage S3 credentials.
#
# Why this exists:
#   Hetzner Object Storage has no public API for creating S3 credentials --
#   they can only be generated through the Hetzner Console UI. So the four
#   credential pairs we need (prod, preview, preview-seed, pulumi) are
#   operator-bootstrapped: created by hand in the Console, then written
#   into Scaleway Secrets Manager as JSON blobs that Pulumi reads at apply
#   time. The same shape `bootstrap.sh` uses for `pulumi-config-passphrase`.
#
# Per-credential JSON shape stored in SM:
#   {"access_key_id": "...", "secret_access_key": "..."}
#
# Credentials and their purpose:
#   - familiar-systems-prod-key         -- campaign-server prod (full project access)
#   - familiar-systems-preview-key      -- campaign-server preview (full project access)
#   - familiar-systems-preview-seed-key -- CI: read prod, write preview only
#   - familiar-systems-pulumi-key       -- Pulumi management (configures the MinIO provider)
#   - familiar-systems-operator-key     -- Human ad-hoc data access (Cyberduck, AWS CLI)
#
# Bucket policies (created by Pulumi after this script runs) restrict the
# seed key to read-only on prod and write-only on preview. The other three
# keys have full access to their respective buckets via the policies' allow
# lists.
#
# Prerequisites:
#   - scw CLI authenticated for the familiar-systems Scaleway project
#   - jq installed
#   - Four S3 credential pairs already generated in the Hetzner Console
#     (Security -> S3 Credentials -> Generate credentials). Have access-key
#     ID and secret access key ready for each one.
#
# Usage:
#   ./scripts/bootstrap-object-storage.sh
#
# Idempotent: existing SM secrets are reused; only new versions are appended.
# Safe to re-run after rotating one or more credentials.

set -euo pipefail

REGION="fr-par"
SECRETS=(
    "familiar-systems-prod-key|campaign-server prod credentials (read+write, full project access)"
    "familiar-systems-preview-key|campaign-server preview credentials (read+write, full project access)"
    "familiar-systems-preview-seed-key|CI seed credentials (read prod, write preview only -- enforced by bucket policy)"
    "familiar-systems-pulumi-key|Pulumi management credentials (configures the MinIO provider; must remain in all bucket policies' allow lists)"
    "familiar-systems-operator-key|Operator ad-hoc data access (Cyberduck / AWS CLI). Full access to both buckets, not bound to any pod -- rotates without service impact."
)

# ---------------------------------------------------------------------------
# Pre-flight
# ---------------------------------------------------------------------------
for tool in scw jq; do
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
# Helpers
# ---------------------------------------------------------------------------
ensure_secret() {
    # $1 = secret name; $2 = description
    # Echoes the secret ID. Creates the secret if missing.
    local name="$1"
    local description="$2"
    local existing
    existing=$(scw secret secret list name="${name}" region="${REGION}" -o json 2>/dev/null \
        | jq -r '.[0].id // empty')

    if [[ -n "${existing}" ]]; then
        echo "${existing}"
        return 0
    fi

    scw secret secret create \
        name="${name}" \
        description="${description}" \
        region="${REGION}" \
        -o json 2>/dev/null \
        | jq -r '.id'
}

prompt_pair() {
    # $1 = secret display name (for prompts)
    # Reads access-key-id and secret-key from the operator and prints JSON
    # to stdout. Secret key is read silently (no echo).
    local label="$1"
    local access_key_id secret_key

    echo "" >&2
    echo "  ${label}" >&2
    read -r -p "    access_key_id: " access_key_id
    read -r -s -p "    secret_access_key (hidden): " secret_key
    echo "" >&2

    if [[ -z "${access_key_id}" || -z "${secret_key}" ]]; then
        echo "ERROR: both fields are required" >&2
        return 1
    fi

    jq -n \
        --arg ak "${access_key_id}" \
        --arg sk "${secret_key}" \
        '{access_key_id: $ak, secret_access_key: $sk}'
}

# ---------------------------------------------------------------------------
# Main loop
# ---------------------------------------------------------------------------
cat <<EOF
==> Hetzner Object Storage credential bootstrap

This will populate five Scaleway SM secrets with Hetzner S3 credential pairs.
For each one, you'll be prompted for the access-key ID and secret access key
from a credential you've already generated in the Hetzner Console.

  Hetzner Console -> Security -> S3 Credentials -> Generate credentials

If a credential pair has already been pasted into SM previously and you don't
want to overwrite it, press Ctrl-D at the access-key-id prompt to skip.
EOF

TMPDIR_LOCAL=$(mktemp -d -t object-storage-bootstrap.XXXXXX)
trap 'rm -rf "${TMPDIR_LOCAL}"' EXIT

for entry in "${SECRETS[@]}"; do
    name="${entry%%|*}"
    description="${entry#*|}"

    echo ""
    echo "==> ${name}"
    echo "    ${description}"

    secret_id=$(ensure_secret "${name}" "${description}")
    echo "    SM secret id: ${secret_id}"

    if ! json_blob=$(prompt_pair "${name}"); then
        echo "    Skipped."
        continue
    fi

    json_file="${TMPDIR_LOCAL}/${name}.json"
    printf '%s' "${json_blob}" > "${json_file}"
    chmod 600 "${json_file}"

    scw secret version create "${secret_id}" \
        data=@"${json_file}" \
        region="${REGION}" >/dev/null
    echo "    New version pushed."
done

cat <<EOF

==> Bootstrap complete.

Next steps:
  1. Make sure Pulumi config has the Hetzner Cloud project ID set:
       pulumi config set hetzner-project-id <numeric-project-id>
     (Find it in Hetzner Console -> top-right project menu -> the number
      after the project name. NOT a secret.)
  2. pulumi up  -- creates the two buckets, attaches policies, lifecycle,
                   and versioning. The pulumi-key SM secret you just set
                   is what the MinIO provider authenticates with.
EOF
