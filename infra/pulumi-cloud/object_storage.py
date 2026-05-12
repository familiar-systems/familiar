"""Hetzner Object Storage resources for familiar.systems.

Provisions two S3-compatible buckets (`familiar-systems-prod` and
`familiar-systems-preview`) in `hel1`, with per-bucket policies, lifecycle
rules, and (prod only) versioning. The campaign-server, the platform-server
DB backup workflow, and any future workloads share these buckets via
prefix discipline (`campaigns/<id>/`, `platform-backups/`, etc.).
Each campaign owns a prefix (`campaigns/<id>/`) with the libSQL file at
`campaigns/<id>/campaign.db`; any future per-campaign sidecars colocate
under the same prefix so GDPR deletion remains one recursive delete.

Credentials
-----------
Hetzner has no public API for creating S3 credentials -- they must be
generated in the Hetzner Console. Five credential pairs are operator-
bootstrapped via `scripts/bootstrap-object-storage.sh`, which writes each
pair to Scaleway SM as JSON `{"access_key_id", "secret_access_key"}`:

  - familiar-systems-prod-key         -- campaign-server prod
  - familiar-systems-preview-key      -- campaign-server preview
  - familiar-systems-preview-seed-key -- CI: read prod, write preview only
  - familiar-systems-pulumi-key       -- Pulumi management (configures provider)
  - familiar-systems-operator-key     -- Human ad-hoc data access (Cyberduck,
                                         AWS CLI). Same full-access shape as
                                         the prod/preview keys but not bound
                                         to any pod, so it rotates without
                                         restarts.

Pulumi reads each via `config.read_secret(...)` and constructs the bucket
policy's principal ARNs at apply time. The pulumi-key pair is what the
MinIO provider authenticates with; its access-key ID must remain in every
bucket policy's allow list, or `pulumi up` will lose the ability to update
those policies (lockout).

Bucket policies
---------------
Each bucket carries a two-statement policy:
  1. Deny anyone whose access-key isn't in the per-bucket allow list.
  2. Restrict the seed key further: read-only on prod, PutObject-only on
     preview. A leaked seed key cannot corrupt prod or exfiltrate preview.

Lifecycle gap
-------------
The pulumi-minio IlmPolicy resource does not expose
AbortIncompleteMultipartUpload. Orphaned multipart parts on Hetzner cost
storage GB but no per-request fees; for our access pattern the leak is
negligible. Revisit by adding a periodic `mc ilm rule add` cron if it
ever matters.

Provider gap
------------
pulumi-hcloud does not (as of late 2025/early 2026) expose Object Storage
resources -- only Hetzner Cloud's older StorageBox. The pulumi-minio
provider configured against Hetzner's S3-compatible endpoint is the
recommended path per Hetzner's own docs.
"""

import json

import pulumi
import pulumi_minio as minio
from pydantic import BaseModel, Field

from config import LOCATION, config, read_secret


class _S3Credentials(BaseModel):
    """JSON shape stored in Scaleway SM for each operator-bootstrapped pair."""

    access_key_id: str = Field(min_length=1)
    secret_access_key: str = Field(min_length=1)


# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------
PROD_BUCKET_NAME = "familiar-systems-prod"
PREVIEW_BUCKET_NAME = "familiar-systems-preview"

OBJECT_STORAGE_HOST = f"{LOCATION}.your-objectstorage.com"
OBJECT_STORAGE_ENDPOINT = f"https://{OBJECT_STORAGE_HOST}"

# SM secret names (operator-bootstrapped, not Pulumi-managed).
# These are SM identifiers, not credential values; the suppressions
# silence ruff's S105 heuristic on the `-key` suffix.
SECRET_PROD = "familiar-systems-prod-key"  # noqa: S105
SECRET_PREVIEW = "familiar-systems-preview-key"  # noqa: S105
SECRET_SEED = "familiar-systems-preview-seed-key"  # noqa: S105
SECRET_PULUMI = "familiar-systems-pulumi-key"  # noqa: S105
SECRET_OPERATOR = "familiar-systems-operator-key"  # noqa: S105

# Numeric Hetzner Cloud project ID. Non-secret; used to build principal
# ARNs in bucket policies (arn:aws:iam:::user/p<project_id>:<access_key>).
HETZNER_PROJECT_ID = config.require("hetzner-project-id")


# ---------------------------------------------------------------------------
# Credential parsing
# ---------------------------------------------------------------------------
def _split_creds(json_blob: str) -> tuple[str, str]:
    """Parse a JSON credential blob into (access_key_id, secret_access_key).

    Raises if the blob is malformed -- which surfaces as a Pulumi apply
    error pointing at the offending SM secret, exactly when the operator
    needs to know.
    """
    creds = _S3Credentials.model_validate_json(json_blob)
    return creds.access_key_id, creds.secret_access_key


def _read_split(secret_name: str) -> tuple[pulumi.Output[str], pulumi.Output[str]]:
    parts: pulumi.Output[tuple[str, str]] = read_secret(secret_name).apply(_split_creds)
    # The access-key ID is the public half of the credential pair (analogous
    # to an AWS access-key ID). Leaving it marked secret -- inherited from
    # the SM blob -- causes Pulumi to redact any artifact that includes it,
    # including the bucket policy JSON and any stack outputs that surface it.
    # Explicitly unsecret it. The secret-key half stays marked.
    access_key_id = pulumi.Output.unsecret(parts.apply(lambda t: t[0]))
    secret_key = parts.apply(lambda t: t[1])
    return access_key_id, secret_key


prod_access_key_id, _prod_secret_key = _read_split(SECRET_PROD)
preview_access_key_id, _preview_secret_key = _read_split(SECRET_PREVIEW)
seed_access_key_id, _seed_secret_key = _read_split(SECRET_SEED)
_pulumi_access_key_id, _pulumi_secret_key = _read_split(SECRET_PULUMI)
operator_access_key_id, _operator_secret_key = _read_split(SECRET_OPERATOR)


# ---------------------------------------------------------------------------
# MinIO provider (Hetzner endpoint, authenticated with pulumi-admin pair)
# ---------------------------------------------------------------------------
minio_provider = minio.Provider(
    "hetzner-object-storage",
    minio_server=OBJECT_STORAGE_HOST,
    minio_user=_pulumi_access_key_id,
    minio_password=_pulumi_secret_key,
    minio_region=LOCATION,
    minio_ssl=True,
)
_provider_opts = pulumi.ResourceOptions(provider=minio_provider)


# ---------------------------------------------------------------------------
# Bucket policy builders (pure functions over resolved access-key IDs)
# ---------------------------------------------------------------------------
def _principal_arn(access_key_id: str) -> str:
    return f"arn:aws:iam:::user/p{HETZNER_PROJECT_ID}:{access_key_id}"


def _bucket_resource_arns(bucket: str) -> list[str]:
    return [f"arn:aws:s3:::{bucket}", f"arn:aws:s3:::{bucket}/*"]


def _build_prod_policy(*, prod_id: str, seed_id: str, pulumi_id: str, operator_id: str) -> str:
    return json.dumps(
        {
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Sid": "DenyAllOthers",
                    "Effect": "Deny",
                    "Action": "s3:*",
                    "NotPrincipal": {
                        "AWS": [
                            _principal_arn(prod_id),
                            _principal_arn(seed_id),
                            _principal_arn(pulumi_id),
                            _principal_arn(operator_id),
                        ]
                    },
                    "Resource": _bucket_resource_arns(PROD_BUCKET_NAME),
                },
                {
                    "Sid": "RestrictSeedToReadOnly",
                    "Effect": "Deny",
                    "NotAction": ["s3:GetObject", "s3:ListBucket"],
                    "Principal": {"AWS": _principal_arn(seed_id)},
                    "Resource": _bucket_resource_arns(PROD_BUCKET_NAME),
                },
            ],
        }
    )


def _build_preview_policy(
    *, preview_id: str, seed_id: str, pulumi_id: str, operator_id: str
) -> str:
    return json.dumps(
        {
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Sid": "DenyAllOthers",
                    "Effect": "Deny",
                    "Action": "s3:*",
                    "NotPrincipal": {
                        "AWS": [
                            _principal_arn(preview_id),
                            _principal_arn(seed_id),
                            _principal_arn(pulumi_id),
                            _principal_arn(operator_id),
                        ]
                    },
                    "Resource": _bucket_resource_arns(PREVIEW_BUCKET_NAME),
                },
                {
                    "Sid": "RestrictSeedToPutOnly",
                    "Effect": "Deny",
                    "NotAction": ["s3:PutObject"],
                    "Principal": {"AWS": _principal_arn(seed_id)},
                    "Resource": _bucket_resource_arns(PREVIEW_BUCKET_NAME),
                },
            ],
        }
    )


_prod_policy_doc: pulumi.Output[str] = pulumi.Output.all(
    prod_id=prod_access_key_id,
    seed_id=seed_access_key_id,
    pulumi_id=_pulumi_access_key_id,
    operator_id=operator_access_key_id,
).apply(
    lambda args: _build_prod_policy(
        prod_id=str(args["prod_id"]),  # pyright: ignore[reportAny]
        seed_id=str(args["seed_id"]),  # pyright: ignore[reportAny]
        pulumi_id=str(args["pulumi_id"]),  # pyright: ignore[reportAny]
        operator_id=str(args["operator_id"]),  # pyright: ignore[reportAny]
    )
)

_preview_policy_doc: pulumi.Output[str] = pulumi.Output.all(
    preview_id=preview_access_key_id,
    seed_id=seed_access_key_id,
    pulumi_id=_pulumi_access_key_id,
    operator_id=operator_access_key_id,
).apply(
    lambda args: _build_preview_policy(
        preview_id=str(args["preview_id"]),  # pyright: ignore[reportAny]
        seed_id=str(args["seed_id"]),  # pyright: ignore[reportAny]
        pulumi_id=str(args["pulumi_id"]),  # pyright: ignore[reportAny]
        operator_id=str(args["operator_id"]),  # pyright: ignore[reportAny]
    )
)


# ---------------------------------------------------------------------------
# Resources
# ---------------------------------------------------------------------------
prod_bucket = minio.S3Bucket(
    "bucket-prod",
    bucket=PROD_BUCKET_NAME,
    acl="private",
    object_locking=False,
    opts=_provider_opts,
)

preview_bucket = minio.S3Bucket(
    "bucket-preview",
    bucket=PREVIEW_BUCKET_NAME,
    acl="private",
    object_locking=False,
    opts=_provider_opts,
)

prod_bucket_policy = minio.S3BucketPolicy(
    "prod-bucket-policy",
    bucket=prod_bucket.bucket,
    policy=_prod_policy_doc,
    opts=_provider_opts,
)

preview_bucket_policy = minio.S3BucketPolicy(
    "preview-bucket-policy",
    bucket=preview_bucket.bucket,
    policy=_preview_policy_doc,
    opts=_provider_opts,
)

# Versioning enabled on prod for soft-delete safety. Preview stays unversioned
# (data is disposable by design and a bug-driven overwrite there is recoverable
# by re-running the preview-seed step from the bootstrap workflow).
prod_bucket_versioning = minio.S3BucketVersioning(
    "prod-bucket-versioning",
    bucket=prod_bucket.bucket,
    versioning_configuration=minio.S3BucketVersioningVersioningConfigurationArgs(
        status="Enabled",
    ),
    opts=_provider_opts,
)

# Prod: bound noncurrent-version retention at 7 days so versioning storage
# doesn't grow unbounded. Depends on versioning being enabled first.
prod_bucket_lifecycle = minio.IlmPolicy(
    "prod-bucket-lifecycle",
    bucket=prod_bucket.bucket,
    rules=[
        # `status` is computed by the provider (always "Enabled" once applied);
        # the Python type stubs expose it as a settable field but the underlying
        # Terraform schema rejects user-provided values.
        minio.IlmPolicyRuleArgs(
            id="expire-noncurrent-versions",
            noncurrent_version_expiration_days=7,
        ),
    ],
    opts=pulumi.ResourceOptions(
        provider=minio_provider,
        depends_on=[prod_bucket_versioning],
    ),
)

# Preview: blanket 7-day object expiration. S3 lifecycle uses last-modified
# semantics, so the writeback-every-30s pattern keeps an active PR's DB
# perpetually fresh; the clock only starts when writebacks stop (PR closed).
preview_bucket_lifecycle = minio.IlmPolicy(
    "preview-bucket-lifecycle",
    bucket=preview_bucket.bucket,
    rules=[
        minio.IlmPolicyRuleArgs(
            id="expire-after-7-days",
            expiration="7d",
        ),
    ],
    opts=_provider_opts,
)
