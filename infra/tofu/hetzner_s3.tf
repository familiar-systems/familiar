locals {
  object_storage_host     = "${local.location}.your-objectstorage.com"
  object_storage_endpoint = "https://${local.object_storage_host}"

  prod_bucket_name    = "familiar-systems-prod"
  preview_bucket_name = "familiar-systems-preview"
}

# -----------------------------------------------------------------------------
# Credential reads (data sources for bucket policy construction)
# -----------------------------------------------------------------------------
# The five operator-bootstrapped S3 credential pairs are stored in Scaleway SM.
# We read the access_key_id from each to build bucket policy principal ARNs.
# The provider itself authenticates via MINIO_USER/MINIO_PASSWORD env vars
# (the pulumi-key pair), not via these data sources.

data "scaleway_secret_version" "s3_prod_key" {
  secret_name = "familiar-systems-prod-key"
  revision    = "latest"
  region      = "fr-par"
}

data "scaleway_secret_version" "s3_preview_key" {
  secret_name = "familiar-systems-preview-key"
  revision    = "latest"
  region      = "fr-par"
}

data "scaleway_secret_version" "s3_seed_key" {
  secret_name = "familiar-systems-preview-seed-key"
  revision    = "latest"
  region      = "fr-par"
}

data "scaleway_secret_version" "s3_pulumi_key" {
  secret_name = "familiar-systems-pulumi-key"
  revision    = "latest"
  region      = "fr-par"
}

data "scaleway_secret_version" "s3_operator_key" {
  secret_name = "familiar-systems-operator-key"
  revision    = "latest"
  region      = "fr-par"
}

locals {
  # Access key IDs are the public half of S3 credentials (analogous to AWS
  # access key IDs). Explicitly nonsensitive so they can appear in bucket
  # policy JSON and stack outputs without redaction.
  prod_access_key_id     = nonsensitive(jsondecode(base64decode(data.scaleway_secret_version.s3_prod_key.data)).access_key_id)
  preview_access_key_id  = nonsensitive(jsondecode(base64decode(data.scaleway_secret_version.s3_preview_key.data)).access_key_id)
  seed_access_key_id     = nonsensitive(jsondecode(base64decode(data.scaleway_secret_version.s3_seed_key.data)).access_key_id)
  pulumi_access_key_id   = nonsensitive(jsondecode(base64decode(data.scaleway_secret_version.s3_pulumi_key.data)).access_key_id)
  operator_access_key_id = nonsensitive(jsondecode(base64decode(data.scaleway_secret_version.s3_operator_key.data)).access_key_id)
}

# -----------------------------------------------------------------------------
# Buckets
# -----------------------------------------------------------------------------

resource "minio_s3_bucket" "prod" {
  bucket         = local.prod_bucket_name
  acl            = "private"
  object_locking = false
}

resource "minio_s3_bucket" "preview" {
  bucket         = local.preview_bucket_name
  acl            = "private"
  object_locking = false
}

# -----------------------------------------------------------------------------
# Bucket policies
# -----------------------------------------------------------------------------
# Each bucket carries a two-statement policy:
#   1. Deny anyone whose access-key isn't in the per-bucket allow list
#   2. Restrict the seed key further (read-only on prod, PutObject-only on preview)
#
# Principal ARN format: arn:aws:iam:::user/p<project_id>:<access_key_id>
# (Hetzner's cosmetic AWS-SDK wrapper, not a real IAM principal)

resource "minio_s3_bucket_policy" "prod" {
  bucket = minio_s3_bucket.prod.bucket
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid    = "DenyAllOthers"
        Effect = "Deny"
        Action = "s3:*"
        NotPrincipal = {
          AWS = [
            "arn:aws:iam:::user/p${var.hetzner_project_id}:${local.prod_access_key_id}",
            "arn:aws:iam:::user/p${var.hetzner_project_id}:${local.seed_access_key_id}",
            "arn:aws:iam:::user/p${var.hetzner_project_id}:${local.pulumi_access_key_id}",
            "arn:aws:iam:::user/p${var.hetzner_project_id}:${local.operator_access_key_id}",
          ]
        }
        Resource = [
          "arn:aws:s3:::${local.prod_bucket_name}",
          "arn:aws:s3:::${local.prod_bucket_name}/*",
        ]
      },
      {
        Sid       = "RestrictSeedToReadOnly"
        Effect    = "Deny"
        NotAction = ["s3:GetObject", "s3:ListBucket"]
        Principal = {
          AWS = "arn:aws:iam:::user/p${var.hetzner_project_id}:${local.seed_access_key_id}"
        }
        Resource = [
          "arn:aws:s3:::${local.prod_bucket_name}",
          "arn:aws:s3:::${local.prod_bucket_name}/*",
        ]
      },
    ]
  })
}

resource "minio_s3_bucket_policy" "preview" {
  bucket = minio_s3_bucket.preview.bucket
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid    = "DenyAllOthers"
        Effect = "Deny"
        Action = "s3:*"
        NotPrincipal = {
          AWS = [
            "arn:aws:iam:::user/p${var.hetzner_project_id}:${local.preview_access_key_id}",
            "arn:aws:iam:::user/p${var.hetzner_project_id}:${local.seed_access_key_id}",
            "arn:aws:iam:::user/p${var.hetzner_project_id}:${local.pulumi_access_key_id}",
            "arn:aws:iam:::user/p${var.hetzner_project_id}:${local.operator_access_key_id}",
          ]
        }
        Resource = [
          "arn:aws:s3:::${local.preview_bucket_name}",
          "arn:aws:s3:::${local.preview_bucket_name}/*",
        ]
      },
      {
        Sid       = "RestrictSeedToPutOnly"
        Effect    = "Deny"
        NotAction = ["s3:PutObject"]
        Principal = {
          AWS = "arn:aws:iam:::user/p${var.hetzner_project_id}:${local.seed_access_key_id}"
        }
        Resource = [
          "arn:aws:s3:::${local.preview_bucket_name}",
          "arn:aws:s3:::${local.preview_bucket_name}/*",
        ]
      },
    ]
  })
}

# -----------------------------------------------------------------------------
# Versioning (prod only; preview data is disposable)
# -----------------------------------------------------------------------------

resource "minio_s3_bucket_versioning" "prod" {
  bucket = minio_s3_bucket.prod.bucket

  versioning_configuration {
    status = "Enabled"
  }
}

# -----------------------------------------------------------------------------
# Lifecycle (ILM) policies
# -----------------------------------------------------------------------------

resource "minio_ilm_policy" "prod" {
  bucket = minio_s3_bucket.prod.bucket

  rule {
    id = "expire-noncurrent-versions"

    noncurrent_expiration {
      days = "7d"
    }
  }

  depends_on = [minio_s3_bucket_versioning.prod]
}

resource "minio_ilm_policy" "preview" {
  bucket = minio_s3_bucket.preview.bucket

  rule {
    id         = "expire-after-7-days"
    expiration = "7d"
  }
}
