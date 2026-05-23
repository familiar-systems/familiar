# -----------------------------------------------------------------------------
# SM secret containers (operator-filled)
# -----------------------------------------------------------------------------
# These containers are managed by OpenTofu; their values are operator-managed
# via `scw secret version create <name> data=<value> region=fr-par`.

resource "scaleway_secret" "deploy_ssh" {
  name        = "loreweaver-deploy-ssh-key"
  description = "Break-glass SSH private key for server access"
  region      = "fr-par"
  protected   = true
}

resource "scaleway_secret" "bunny_api_key" {
  name        = "bunny-api-key"
  description = "bunny.net API key for DNS-01 ACME challenges"
  region      = "fr-par"
}

resource "scaleway_secret" "k3s_kubeconfig" {
  name        = "k3s-kubeconfig"
  description = "k3s cluster kubeconfig for GHA deploys"
  region      = "fr-par"
}

resource "scaleway_secret" "internal_bearer_prod" {
  name        = "internal-bearer-prod"
  description = "Shared bearer for prod platform <-> campaign /internal/*"
  region      = "fr-par"
  protected   = true
}

resource "scaleway_secret" "internal_bearer_preview" {
  name        = "internal-bearer-preview"
  description = "Shared bearer for preview platform <-> campaign /internal/* (shared across PRs)"
  region      = "fr-par"
}

resource "scaleway_secret" "hcloud_api_token" {
  name        = "hcloud-api-token"
  description = "Hetzner Cloud API token"
  region      = "fr-par"
  protected   = true
}

# -----------------------------------------------------------------------------
# SM secret containers + versions (OpenTofu-written)
# -----------------------------------------------------------------------------
# These secrets are fully managed: OpenTofu creates the IAM credentials
# (scaleway_iam.tf) and writes them here. No operator intervention needed.

resource "scaleway_secret" "eso_credential" {
  name        = "eso-scaleway-credential"
  description = "ESO's Scaleway API key for SM access (access_key + secret_key JSON)"
  region      = "fr-par"
}

resource "scaleway_secret_version" "eso_credential" {
  secret_id = scaleway_secret.eso_credential.id
  data = jsonencode({
    access_key = scaleway_iam_api_key.eso.access_key
    secret_key = scaleway_iam_api_key.eso.secret_key
  })
  region = "fr-par"

  # Imported IAM API keys have null secret_key (only available at creation).
  # The existing SM value is correct; don't replace it.
  lifecycle {
    ignore_changes = [data]
  }
}

resource "scaleway_secret" "registry_pull_credential" {
  name        = "registry-pull-credential"
  description = "Scaleway registry pull credential (access_key + secret_key JSON)"
  region      = "fr-par"
}

resource "scaleway_secret_version" "registry_pull_credential" {
  secret_id = scaleway_secret.registry_pull_credential.id
  data = jsonencode({
    access_key = scaleway_iam_api_key.registry_pull.access_key
    secret_key = scaleway_iam_api_key.registry_pull.secret_key
  })
  region = "fr-par"

  lifecycle {
    ignore_changes = [data]
  }
}
