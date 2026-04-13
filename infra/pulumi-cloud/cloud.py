"""Hetzner Cloud + Scaleway resources for Loreweaver.

Shared infrastructure: SSH keys, firewall, Scaleway Container Registry,
and Scaleway Secrets Manager entries. The k3s cluster (k3s_cluster.py)
and Kubernetes resources (k8s.py) build on these.
"""

import pulumi
import pulumi_hcloud as hcloud
import pulumiverse_scaleway as scaleway

from config import LABELS, LOCATION, config

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
# Floating IP (public entry point -- DNS A record for loreweaver.no)
# ---------------------------------------------------------------------------
floating_ip = hcloud.FloatingIp(
    "floating-ip",
    type="ipv4",
    home_location=LOCATION,
    description="Public IP for loreweaver.no (DNS, TLS, ingress)",
    labels=LABELS,
    # Alias: this resource was previously named "k3s-floating-ip" as a child
    # of K3sCluster. Safe to remove after one successful `pulumi up`.
    opts=pulumi.ResourceOptions(
        aliases=[
            pulumi.Alias(
                name="k3s-floating-ip",
                parent="urn:pulumi:prod::loreweaver-cloud::loreweaver:infra:K3sCluster::k3s",
            ),
        ],
    ),
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
# Scaleway IAM: dedicated pull-scoped principal for cluster image pulls
# ---------------------------------------------------------------------------
# The cluster's imagePullSecret needs a Scaleway API key. Baking the admin
# SCW_SECRET_KEY into the k8s Secret would couple the state graph to a
# rotatable, per-operator credential (every operator writes their own key,
# every `pulumi up` shows drift). Having Pulumi own a dedicated application
# + scoped policy + api key instead makes rotation a `pulumi up` away, zero
# console toil, and limits blast radius to read-only on the loreweaver
# container registry project.
#
# Safe here (and NOT for the kubeconfig) because the consumed field is
# k8s.core.v1.Secret.string_data -- an in-place PATCH, not replaceOnChanges.
# See feedback_iac_credential_decoupling.md for the full rule.
registry_pull_app = scaleway.iam.Application(
    "registry-pull-app",
    name="k3s-registry-puller",
    description="Cluster imagePullSecret principal (read-only registry access)",
)

registry_pull_policy = scaleway.iam.Policy(
    "registry-pull-policy",
    name="k3s-registry-puller",
    description="Read-only access to the loreweaver container registry",
    application_id=registry_pull_app.id,
    rules=[
        scaleway.iam.PolicyRuleArgs(
            permission_set_names=["ContainerRegistryReadOnly"],
            project_ids=[registry.project_id],
        ),
    ],
)

# depends_on forces Policy -> ApiKey ordering. Without it, Pulumi could
# create the ApiKey before the Policy (they share no data flow), and the
# key would briefly exist with zero permissions -- fine in steady state,
# but a race on first `pulumi up`.
registry_pull_api_key = scaleway.iam.ApiKey(
    "registry-pull-api-key",
    application_id=registry_pull_app.id,
    description="Cluster imagePullSecret credential (managed by Pulumi)",
    opts=pulumi.ResourceOptions(depends_on=[registry_pull_policy]),
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
