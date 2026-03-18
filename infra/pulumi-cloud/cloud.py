"""Hetzner Cloud + Scaleway resources for Loreweaver.

Shared infrastructure: SSH keys, firewall, Scaleway Container Registry,
and Scaleway Secrets Manager entries. The k3s cluster (k3s_cluster.py)
and Kubernetes resources (k8s.py) build on these.
"""

import pulumi_hcloud as hcloud
import pulumiverse_scaleway as scaleway

from config import LABELS, config

# ---------------------------------------------------------------------------
# SSH Keys (both registered -- personal for desktop, deploy for break-glass)
# ---------------------------------------------------------------------------
personal_key = hcloud.SshKey(
    "personal-ssh-key",
    name="loreweaver-personal",
    public_key=config.require("personal-ssh-public-key"),
    labels=LABELS,
)

deploy_key = hcloud.SshKey(
    "deploy-ssh-key",
    name="loreweaver-deploy",
    public_key=config.require("deploy-ssh-public-key"),
    labels=LABELS,
)

# ---------------------------------------------------------------------------
# Firewall
# ---------------------------------------------------------------------------
firewall = hcloud.Firewall(
    "firewall",
    name="loreweaver-fw",
    rules=[
        hcloud.FirewallRuleArgs(
            direction="in",
            protocol="icmp",
            source_ips=["0.0.0.0/0", "::/0"],
            description="Allow ping",
        ),
        hcloud.FirewallRuleArgs(
            direction="in",
            protocol="tcp",
            port="22",
            source_ips=["0.0.0.0/0", "::/0"],
            description="Allow SSH",
        ),
        hcloud.FirewallRuleArgs(
            direction="in",
            protocol="tcp",
            port="80",
            source_ips=["0.0.0.0/0", "::/0"],
            description="Allow HTTP",
        ),
        hcloud.FirewallRuleArgs(
            direction="in",
            protocol="tcp",
            port="443",
            source_ips=["0.0.0.0/0", "::/0"],
            description="Allow HTTPS",
        ),
        hcloud.FirewallRuleArgs(
            direction="in",
            protocol="tcp",
            port="6443",
            source_ips=["0.0.0.0/0", "::/0"],
            description="Allow k3s API server",
        ),
    ],
    labels=LABELS,
)

# ---------------------------------------------------------------------------
# Scaleway Container Registry
# ---------------------------------------------------------------------------
registry = scaleway.registry.Namespace(
    "container-registry",
    name="loreweaver",
    description="Loreweaver container images",
    is_public=False,
    region="fr-par",
)

# ---------------------------------------------------------------------------
# Scaleway Secrets (empty containers -- filled manually or by GHA)
# ---------------------------------------------------------------------------
deploy_ssh_secret = scaleway.secrets.Secret(
    "deploy-ssh-secret",
    name="loreweaver-deploy-ssh-key",
    description="Break-glass SSH private key for server access",
    region="fr-par",
    protected=True,
)

bunny_api_key_secret = scaleway.secrets.Secret(
    "bunny-api-key-secret",
    name="bunny-api-key",
    description="bunny.net API key for Traefik DNS-01 ACME challenges",
    region="fr-par",
)

k3s_kubeconfig_secret = scaleway.secrets.Secret(
    "k3s-kubeconfig-secret",
    name="k3s-kubeconfig",
    description="k3s cluster kubeconfig for GHA deploys",
    region="fr-par",
)
