# Scaleway: reads credentials and region from ~/.config/scw/config.yaml (scw CLI config).
provider "scaleway" {}

# Ephemeral secrets: read at runtime via Scaleway SM, never persisted to
# state or plan. https://opentofu.org/docs/v1.12/language/ephemerality/

ephemeral "scaleway_secret_version" "hcloud_token" {
  secret_name = "hcloud-api-token"
  revision    = "latest"
  region      = "fr-par"
}

# TODO: rename to familiar-systems-iac-key (requires new Hetzner console credential + bucket policy update)
ephemeral "scaleway_secret_version" "minio_key" {
  secret_name = "familiar-systems-pulumi-key"
  revision    = "latest"
  region      = "fr-par"
}

# Hetzner Cloud
provider "hcloud" {
  token = base64decode(ephemeral.scaleway_secret_version.hcloud_token.data)
}

# Hetzner Object Storage (S3-compatible via aminueza/minio)
provider "minio" {
  minio_server   = "hel1.your-objectstorage.com"
  minio_region   = "hel1"
  minio_ssl      = true
  minio_user     = jsondecode(base64decode(ephemeral.scaleway_secret_version.minio_key.data)).access_key_id
  minio_password = jsondecode(base64decode(ephemeral.scaleway_secret_version.minio_key.data)).secret_access_key
}
