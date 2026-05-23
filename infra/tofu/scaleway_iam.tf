# -----------------------------------------------------------------------------
# Container Registry
# -----------------------------------------------------------------------------

resource "scaleway_registry_namespace" "main" {
  name        = "loreweaver"
  description = "familiar.systems container images"
  is_public   = false
  region      = "fr-par"
}

# -----------------------------------------------------------------------------
# IAM: registry pull principal (read-only, for cluster imagePullSecret)
# -----------------------------------------------------------------------------

resource "scaleway_iam_application" "registry_pull" {
  name        = "k3s-registry-puller"
  description = "Cluster imagePullSecret principal (read-only registry access)"
}

resource "scaleway_iam_policy" "registry_pull" {
  name           = "k3s-registry-puller"
  description    = "Read-only access to the loreweaver container registry"
  application_id = scaleway_iam_application.registry_pull.id

  rule {
    permission_set_names = ["ContainerRegistryReadOnly"]
    project_ids          = [scaleway_registry_namespace.main.project_id]
  }
}

resource "scaleway_iam_api_key" "registry_pull" {
  application_id = scaleway_iam_application.registry_pull.id
  description    = "Cluster imagePullSecret credential (managed by OpenTofu)"
  depends_on     = [scaleway_iam_policy.registry_pull]
}

# -----------------------------------------------------------------------------
# IAM: ESO principal (read-only Secrets Manager access)
# -----------------------------------------------------------------------------

resource "scaleway_iam_application" "eso" {
  name        = "k3s-external-secrets"
  description = "ESO principal (read-only Secrets Manager access)"
}

resource "scaleway_iam_policy" "eso" {
  name           = "k3s-external-secrets"
  description    = "Read-only access to Scaleway Secrets Manager (list + read payloads)"
  application_id = scaleway_iam_application.eso.id

  rule {
    permission_set_names = ["SecretManagerReadOnly", "SecretManagerSecretAccess"]
    project_ids          = [scaleway_registry_namespace.main.project_id]
  }
}

resource "scaleway_iam_api_key" "eso" {
  application_id = scaleway_iam_application.eso.id
  description    = "ESO cluster credential (managed by OpenTofu)"
  depends_on     = [scaleway_iam_policy.eso]
}
