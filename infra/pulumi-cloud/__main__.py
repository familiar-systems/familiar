"""Loreweaver cloud infrastructure.

Provisions:
  - Coolify server on Hetzner (Phase 1, production)
  - k3s cluster on Hetzner (Phase 2, preview)
  - Scaleway Container Registry + Secrets Manager

See cloud.py for Coolify-era Hetzner + Scaleway resources.
See k3s_cluster.py for the K3sCluster ComponentResource.
See config.py for shared constants.
"""

import pulumi
import pulumiverse_scaleway as scaleway

import cloud as loreweaver_cloud
import config as loreweaver_config
from k3s_cluster import K3sCluster
from k8s import create_k8s_resources

# ---------------------------------------------------------------------------
# k3s cluster (Phase 2: preview subdomain)
# ---------------------------------------------------------------------------
k3s = K3sCluster(
    "k3s",
    location=loreweaver_config.LOCATION,
    server_type=loreweaver_config.SERVER_TYPE,
    image=loreweaver_config.IMAGE,
    ssh_keys=[loreweaver_cloud.personal_key.name, loreweaver_cloud.deploy_key.name],
    firewall_id=loreweaver_cloud.firewall.id.apply(int),
    deploy_private_key=loreweaver_config.read_secret("loreweaver-deploy-ssh-key"),
    labels=loreweaver_config.LABELS,
    extra_tls_sans=[loreweaver_cloud.coolify_floating_ip.ip_address],
)

# ---------------------------------------------------------------------------
# Kubernetes resources on the k3s cluster
# ---------------------------------------------------------------------------
create_k8s_resources(
    kubeconfig=k3s.kubeconfig,
    registry_endpoint=loreweaver_cloud.registry.endpoint,
    bunny_api_key=loreweaver_config.read_secret("bunny-api-key"),
    acme_email=loreweaver_config.config.require("acme-email"),
)

# ---------------------------------------------------------------------------
# Populate k3s kubeconfig into Scaleway Secrets Manager (for GHA deploys)
# ---------------------------------------------------------------------------
_k3s_kubeconfig_version = scaleway.secrets.Version(
    "k3s-kubeconfig-version",
    secret_id=loreweaver_cloud.k3s_kubeconfig_secret.id,
    data=k3s.kubeconfig,
    region="fr-par",
)

# ---------------------------------------------------------------------------
# Exports
# ---------------------------------------------------------------------------
# Coolify (Phase 1)
pulumi.export("floating_ip", loreweaver_cloud.coolify_floating_ip.ip_address)
pulumi.export("server_ip", loreweaver_cloud.coolify_server.ipv4_address)
pulumi.export("server_id", loreweaver_cloud.coolify_server.id)
pulumi.export("volume_id", loreweaver_cloud.coolify_volume.id)
pulumi.export("volume_linux_device", loreweaver_cloud.coolify_volume.linux_device)

# Scaleway
pulumi.export("registry_endpoint", loreweaver_cloud.registry.endpoint)
pulumi.export("deploy_ssh_secret_id", loreweaver_cloud.deploy_ssh_secret.id)
pulumi.export("coolify_api_token_secret_id", loreweaver_cloud.coolify_api_token_secret.id)
pulumi.export("coolify_site_webhook_secret_id", loreweaver_cloud.coolify_site_webhook_secret.id)
pulumi.export("bunny_api_key_secret_id", loreweaver_cloud.bunny_api_key_secret.id)
pulumi.export("k3s_kubeconfig_secret_id", loreweaver_cloud.k3s_kubeconfig_secret.id)

# k3s (Phase 2)
pulumi.export("k3s_floating_ip", k3s.floating_ip_address)
pulumi.export("k3s_server_ip", k3s.server_ip)
pulumi.export("k3s_kubeconfig", k3s.kubeconfig)
