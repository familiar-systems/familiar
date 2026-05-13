#!/usr/bin/env bash
# bootstrap-object-storage.sh -- Operator-bootstrap Hetzner Object Storage
# state that Pulumi can't manage end-to-end:
#   1. Five S3 credential pairs stored in Scaleway Secrets Manager.
#   2. The two buckets themselves (familiar-systems-prod, -preview).
#
# Why this exists:
#   (1) Hetzner Object Storage has no public API for creating S3 credentials
#       -- they can only be generated through the Hetzner Console UI. So the
#       five credential pairs we need are operator-bootstrapped: created by
#       hand in the Console, then written into Scaleway SM as JSON blobs
#       that Pulumi reads at apply time. Same shape `bootstrap.sh` uses for
#       `pulumi-config-passphrase`.
#   (2) Bucket Create runs into an unfixed upstream bug. pulumi-minio 0.16.9
#       pins aminueza/terraform-provider-minio v1.20.1, whose Create flow
#       does an immediate Read-after-Create that races with Hetzner's
#       eventually-consistent bucket index -- the Read sees NoSuchBucket
#       and the provider returns (nil state, nil error), tripping a Pulumi
#       bridge panic. The fix is in aminueza v3.28.1 but the bridge has
#       never bumped past v1.20.1 (released 2023-11-08, immediately put
#       into maintenance mode). See pulumi-minio#754, aminueza#839.
#       So we create the buckets here, and Pulumi adopts them on first
#       apply via `pulumi.ResourceOptions(import_=...)` in object_storage.py.
#
# Per-credential JSON shape stored in SM:
#   {"access_key_id": "...", "secret_access_key": "..."}
#
# Credentials and their purpose:
#   - familiar-systems-prod-key         -- campaign-server prod (full project access)
#   - familiar-systems-preview-key      -- campaign-server preview (full project access)
#   - familiar-systems-preview-seed-key -- CI: read prod, write preview only
#   - familiar-systems-pulumi-key       -- Pulumi management (configures the MinIO provider)
#                                          AND the credential this script uses for CreateBucket.
#   - familiar-systems-operator-key     -- Human ad-hoc data access (Cyberduck, AWS CLI)
#
# Bucket policies (created by Pulumi after this script runs) restrict the
# seed key to read-only on prod and write-only on preview. The other three
# keys have full access to their respective buckets via the policies' allow
# lists.
#
# Prerequisites:
#   - scw CLI authenticated for the familiar-systems Scaleway project
#   - aws CLI installed (used for bucket creation against Hetzner's endpoint)
#   - jq installed
#   - Five S3 credential pairs already generated in the Hetzner Console
#     (Security -> S3 Credentials -> Generate credentials). Have access-key
#     ID and secret access key ready for each one.
#
# Usage:
#   ./scripts/bootstrap-object-storage.sh
#
# Idempotent: existing SM secrets are reused (only new versions are appended);
# existing buckets are skipped. Safe to re-run after rotating one or more
# credentials.

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
for tool in scw aws jq; do
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
    # to stdout. Secret key is read silently (no echo). A single Ctrl-D
    # (EOF) at the first prompt short-circuits the second prompt and
    # returns non-zero so the caller can mark this credential "Skipped."
    local label="$1"
    local access_key_id secret_key

    echo "" >&2
    echo "  ${label}" >&2
    if ! read -r -p "    access_key_id: " access_key_id || [[ -z "${access_key_id}" ]]; then
        echo "" >&2
        return 1
    fi
    if ! read -r -s -p "    secret_access_key (hidden): " secret_key || [[ -z "${secret_key}" ]]; then
        echo "" >&2
        echo "ERROR: secret_access_key cannot be empty" >&2
        return 1
    fi
    echo "" >&2

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

# ---------------------------------------------------------------------------
# Bucket creation
# ---------------------------------------------------------------------------
# Buckets are created here, not by Pulumi, because pulumi-minio 0.16.9 cannot
# survive Hetzner's read-after-create race (see header). Pulumi adopts the
# pre-created buckets on first apply via `import_=` in object_storage.py.

HETZNER_S3_ENDPOINT="https://hel1.your-objectstorage.com"
HETZNER_REGION="hel1"
BUCKETS=(
    "familiar-systems-prod"
    "familiar-systems-preview"
)
PULUMI_SECRET_NAME="familiar-systems-pulumi-key"

echo ""
echo "==> Creating buckets on Hetzner Object Storage"

pulumi_secret_id=$(scw secret secret list name="${PULUMI_SECRET_NAME}" region="${REGION}" -o json \
    | jq -r '.[0].id // empty')
if [[ -z "${pulumi_secret_id}" ]]; then
    echo "ERROR: ${PULUMI_SECRET_NAME} not found in SM. Re-run and provide that credential pair." >&2
    exit 1
fi

pulumi_creds_b64=$(scw secret version access "${pulumi_secret_id}" \
    revision=latest region="${REGION}" -o json 2>/dev/null \
    | jq -r '.data // empty')
if [[ -z "${pulumi_creds_b64}" ]]; then
    echo "ERROR: ${PULUMI_SECRET_NAME} has no version in SM. Re-run and provide that credential pair." >&2
    exit 1
fi

pulumi_creds_json=$(printf '%s' "${pulumi_creds_b64}" | base64 -d)
AWS_ACCESS_KEY_ID=$(printf '%s' "${pulumi_creds_json}" | jq -r '.access_key_id')
AWS_SECRET_ACCESS_KEY=$(printf '%s' "${pulumi_creds_json}" | jq -r '.secret_access_key')
export AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY
unset AWS_SESSION_TOKEN AWS_PROFILE

for bucket in "${BUCKETS[@]}"; do
    echo ""
    echo "==> ${bucket}"
    if aws s3api head-bucket \
            --bucket "${bucket}" \
            --endpoint-url "${HETZNER_S3_ENDPOINT}" \
            --region "${HETZNER_REGION}" >/dev/null 2>&1; then
        echo "    Bucket exists."
        continue
    fi
    aws s3 mb "s3://${bucket}" \
        --endpoint-url "${HETZNER_S3_ENDPOINT}" \
        --region "${HETZNER_REGION}" >/dev/null
    echo "    Created."
done

cat <<EOF

==> Bootstrap complete.

Next steps:
  1. Make sure Pulumi config has the Hetzner Cloud project ID set:
       pulumi config set hetzner-project-id <numeric-project-id>
     (Find it in Hetzner Console -> top-right project menu -> the number
      after the project name. NOT a secret.)
  2. pulumi up  -- adopts the two buckets into Pulumi state (via import_=
                   on the S3Bucket resources) and creates the bucket policies,
                   lifecycle rules, and versioning. The pulumi-key SM secret
                   is what the MinIO provider authenticates with.
EOF
