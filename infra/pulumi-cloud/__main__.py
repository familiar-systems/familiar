"""Loreweaver cloud infrastructure.

Provisions:
  - k3s cluster on Hetzner (production + preview)
  - Scaleway Container Registry + Secrets Manager

See cloud.py for shared Hetzner + Scaleway resources.
See k3s_cluster.py for the K3sCluster ComponentResource.
See config.py for shared constants.

The k8s Provider authenticates with a static long-lived ServiceAccount
bearer token built from three byte-stable inputs (floating IP, cluster CA,
SA token), all sourced from Scaleway SM. The Provider's kubeconfig string
is therefore stable across `pulumi up` runs, which prevents the cascade
that would otherwise replace every k8s resource on credential change.
The SA, RoleBinding, and token-Secret are bootstrapped via cloud-init for
fresh clusters and via `scripts/bootstrap-pulumi-admin.sh` for existing
clusters.
"""

import pulumi

import cloud as loreweaver_cloud
import config as loreweaver_config
from k3s_cluster import K3sCluster
from k8s import create_k8s_resources

# ---------------------------------------------------------------------------
# k3s cluster
# ---------------------------------------------------------------------------
k3s = K3sCluster(
    "k3s",
    floating_ip=loreweaver_cloud.floating_ip,
    firewall=loreweaver_cloud.firewall,
    ssh_keys=[loreweaver_cloud.personal_key, loreweaver_cloud.deploy_key],
    location=loreweaver_config.LOCATION,
    server_type=loreweaver_config.SERVER_TYPE,
    image=loreweaver_config.IMAGE,
    labels=loreweaver_config.LABELS,
)

# ---------------------------------------------------------------------------
# k8s Provider kubeconfig (built from SM inputs, byte-stable forever)
# ---------------------------------------------------------------------------
# All three inputs are read at deploy time and never change unless an operator
# deliberately rotates them, so Pulumi's diff sees the same string on every
# run and the Provider is never replaced.
_pulumi_admin_token = loreweaver_config.read_secret("k3s-pulumi-admin-token")
_cluster_ca_b64 = loreweaver_config.read_secret("k3s-cluster-ca")

_static_kubeconfig: pulumi.Output[str] = pulumi.Output.all(
    floating_ip=loreweaver_cloud.floating_ip.ip_address,
    ca=_cluster_ca_b64,
    token=_pulumi_admin_token,
).apply(
    lambda args: _build_token_kubeconfig(
        floating_ip=str(args["floating_ip"]),  # pyright: ignore[reportAny]
        ca_b64=str(args["ca"]),  # pyright: ignore[reportAny]
        token=str(args["token"]),  # pyright: ignore[reportAny]
    )
)

# ---------------------------------------------------------------------------
# Kubernetes resources on the k3s cluster
# ---------------------------------------------------------------------------
create_k8s_resources(
    kubeconfig=_static_kubeconfig,
    registry=loreweaver_cloud.registry,
    bunny_api_key=loreweaver_config.read_secret("bunny-api-key"),
    registry_pull_key=loreweaver_cloud.registry_pull_api_key.secret_key,
    acme_email=loreweaver_config.config.require("acme-email"),
)

# ---------------------------------------------------------------------------
# Exports
# ---------------------------------------------------------------------------
# Scaleway
pulumi.export("registry_endpoint", loreweaver_cloud.registry.endpoint)
pulumi.export("deploy_ssh_secret_id", loreweaver_cloud.deploy_ssh_secret.id)
pulumi.export("bunny_api_key_secret_id", loreweaver_cloud.bunny_api_key_secret.id)
pulumi.export("k3s_kubeconfig_secret_id", loreweaver_cloud.k3s_kubeconfig_secret.id)

# k3s
pulumi.export("k3s_floating_ip", loreweaver_cloud.floating_ip.ip_address)
pulumi.export("k3s_server_ip", k3s.server_ip)


def _build_token_kubeconfig(*, floating_ip: str, ca_b64: str, token: str) -> str:
    """Build a token-based kubeconfig YAML string from byte-stable inputs."""
    return f"""\
apiVersion: v1
kind: Config
clusters:
  - name: k3s-loreweaver
    cluster:
      server: https://{floating_ip}:6443
      certificate-authority-data: {ca_b64}
contexts:
  - name: default
    context:
      cluster: k3s-loreweaver
      user: pulumi-admin
current-context: default
users:
  - name: pulumi-admin
    user:
      token: {token}
"""
