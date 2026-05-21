"""Hetzner Cloud + Scaleway resources for familiar.systems.

Shared infrastructure: SSH keys, firewall, Scaleway Container Registry,
and Scaleway Secrets Manager entries. The k3s cluster (k3s_cluster.py)
builds on these. Kubernetes resources are managed by Kustomize (infra/k8s/).
"""

import json

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
# Floating IP (public entry point -- DNS A records for familiar.systems,
# app.familiar.systems, preview.familiar.systems, app.preview.familiar.systems,
# and the legacy loreweaver.no apexes all point here)
# ---------------------------------------------------------------------------
floating_ip = hcloud.FloatingIp(
    "floating-ip",
    type="ipv4",
    home_location=LOCATION,
    description="Public IP for familiar.systems + loreweaver.no (DNS, TLS, ingress)",
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
    description="familiar.systems container images",
    is_public=False,
    region="fr-par",
)

# ---------------------------------------------------------------------------
# Scaleway IAM: dedicated pull-scoped principal for cluster image pulls
# ---------------------------------------------------------------------------
# The cluster's imagePullSecret needs a Scaleway API key. Pulumi owns a
# dedicated application + scoped policy + api key, limiting blast radius to
# read-only on the loreweaver container registry project. The credential is
# written to SM as `registry-pull-credential`; ESO reads it and constructs
# the kubernetes.io/dockerconfigjson Secret in-cluster.
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
# Scaleway IAM: dedicated SM-read principal for External Secrets Operator
# ---------------------------------------------------------------------------
# ESO runs in-cluster and syncs secrets from Scaleway SM into k8s Secrets.
# Same pattern as the registry pull app above: dedicated application with
# least-privilege policy, API key written to SM so bootstrap-helm.sh can
# seed the initial k8s Secret that ESO authenticates with.
eso_app = scaleway.iam.Application(
    "eso-app",
    name="k3s-external-secrets",
    description="ESO principal (read-only Secrets Manager access)",
)

eso_policy = scaleway.iam.Policy(
    "eso-policy",
    name="k3s-external-secrets",
    description="Read-only access to Scaleway Secrets Manager",
    application_id=eso_app.id,
    rules=[
        scaleway.iam.PolicyRuleArgs(
            permission_set_names=["SecretManagerReadOnly"],
            project_ids=[registry.project_id],
        ),
    ],
)

eso_api_key = scaleway.iam.ApiKey(
    "eso-api-key",
    application_id=eso_app.id,
    description="ESO cluster credential (managed by Pulumi)",
    opts=pulumi.ResourceOptions(depends_on=[eso_policy]),
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

# Internal-bearer SM containers, Pulumi-owned, operator-filled.
# Rotation is operator-driven:
#
#   openssl rand -base64 32 | scw secret version create internal-bearer-prod \
#       data=- region=fr-par
#
# ESO picks up the new SM version on its refreshInterval (1h) or
# on-demand via: kubectl annotate externalsecret internal-bearer \
#   force-sync=$(date +%s) --overwrite
# Then restart consuming pods: kubectl rollout restart deployment/platform
#   deployment/campaign
internal_bearer_prod_secret = scaleway.secrets.Secret(
    "internal-bearer-prod-secret",
    name="internal-bearer-prod",
    description="Shared bearer for prod platform <-> campaign /internal/*",
    region="fr-par",
    protected=True,
)

internal_bearer_preview_secret = scaleway.secrets.Secret(
    "internal-bearer-preview-secret",
    name="internal-bearer-preview",
    description="Shared bearer for preview platform <-> campaign /internal/* (shared across PRs)",
    region="fr-par",
)

# ---------------------------------------------------------------------------
# Scaleway Secrets: Pulumi-written credentials for ESO
# ---------------------------------------------------------------------------
# ESO's own authentication credential, written to SM so bootstrap-helm.sh
# can read it and create the initial k8s Secret.
eso_credential_secret = scaleway.secrets.Secret(
    "eso-credential-secret",
    name="eso-scaleway-credential",
    description="ESO's Scaleway API key for SM access (access_key + secret_key JSON)",
    region="fr-par",
)

eso_credential_version = scaleway.secrets.Version(
    "eso-credential-version",
    secret_id=eso_credential_secret.id,
    data=pulumi.Output.all(
        access_key=eso_api_key.access_key,
        secret_key=eso_api_key.secret_key,
    ).apply(
        lambda args: json.dumps(
            {"access_key": args["access_key"], "secret_key": args["secret_key"]}
        )
    ),
    region="fr-par",
)

# Registry pull credential written to SM so ESO can construct the
# kubernetes.io/dockerconfigjson Secret in-cluster. Uses the existing
# registry_pull_api_key (ContainerRegistryReadOnly scope) rather than
# the admin SCW_SECRET_KEY.
registry_pull_credential_secret = scaleway.secrets.Secret(
    "registry-pull-credential-secret",
    name="registry-pull-credential",
    description="Scaleway registry pull credential (access_key + secret_key JSON)",
    region="fr-par",
)

registry_pull_credential_version = scaleway.secrets.Version(
    "registry-pull-credential-version",
    secret_id=registry_pull_credential_secret.id,
    data=pulumi.Output.all(
        access_key=registry_pull_api_key.access_key,
        secret_key=registry_pull_api_key.secret_key,
    ).apply(
        lambda args: json.dumps(
            {"access_key": args["access_key"], "secret_key": args["secret_key"]}
        )
    ),
    region="fr-par",
)
