"""familiar.systems cloud infrastructure.

Provisions:
  - k3s cluster on Hetzner (production + preview)
  - Scaleway Container Registry + Secrets Manager
  - Hetzner Object Storage (S3-compatible)

Kubernetes resources are managed by Kustomize (infra/k8s/), not Pulumi.
See cloud.py for shared Hetzner + Scaleway resources.
See k3s_cluster.py for the K3sCluster ComponentResource.
See config.py for shared constants.
"""

import pulumi

import cloud as fs_cloud
import config as fs_config
import object_storage as fs_object_storage
from k3s_cluster import K3sCluster

# ---------------------------------------------------------------------------
# k3s cluster
# ---------------------------------------------------------------------------
k3s = K3sCluster(
    "k3s",
    floating_ip=fs_cloud.floating_ip,
    firewall=fs_cloud.firewall,
    ssh_keys=[fs_cloud.personal_key, fs_cloud.deploy_key],
    location=fs_config.LOCATION,
    server_type=fs_config.SERVER_TYPE,
    image=fs_config.IMAGE,
    labels=fs_config.LABELS,
)

# ---------------------------------------------------------------------------
# Exports
# ---------------------------------------------------------------------------
# Scaleway
pulumi.export("registry_endpoint", fs_cloud.registry.endpoint)
pulumi.export("deploy_ssh_secret_id", fs_cloud.deploy_ssh_secret.id)
pulumi.export("bunny_api_key_secret_id", fs_cloud.bunny_api_key_secret.id)
pulumi.export("k3s_kubeconfig_secret_id", fs_cloud.k3s_kubeconfig_secret.id)

# k3s
pulumi.export("k3s_floating_ip", fs_cloud.floating_ip.ip_address)
pulumi.export("k3s_server_ip", k3s.server_ip)

# Object Storage (Hetzner Object Storage in hel1, S3-compatible).
pulumi.export("object_storage_endpoint", fs_object_storage.OBJECT_STORAGE_ENDPOINT)
pulumi.export("object_storage_prod_bucket", fs_object_storage.PROD_BUCKET_NAME)
pulumi.export("object_storage_preview_bucket", fs_object_storage.PREVIEW_BUCKET_NAME)
pulumi.export("object_storage_prod_access_key_id", fs_object_storage.prod_access_key_id)
pulumi.export("object_storage_preview_access_key_id", fs_object_storage.preview_access_key_id)
pulumi.export("object_storage_preview_seed_access_key_id", fs_object_storage.seed_access_key_id)
pulumi.export("object_storage_operator_access_key_id", fs_object_storage.operator_access_key_id)
