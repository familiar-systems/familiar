"""Hetzner Cloud + Scaleway resources for Loreweaver.

Provisions the Coolify server (Phase 1) and all supporting infrastructure.
This module will be simplified in Phase 3 when Coolify resources are removed.
"""

import pulumi
import pulumi_hcloud as hcloud
import pulumiverse_scaleway as scaleway

from config import IMAGE, LABELS, LOCATION, SERVER_TYPE, config

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
# Floating IP (location only -- no server_id, avoids circular dep)
# ---------------------------------------------------------------------------
coolify_floating_ip = hcloud.FloatingIp(
    "floating-ip",
    type="ipv4",
    home_location=LOCATION,
    description="Loreweaver production IP",
    labels=LABELS,
)

# ---------------------------------------------------------------------------
# Volume (location only -- no server_id, avoids circular dep)
# ---------------------------------------------------------------------------
coolify_volume = hcloud.Volume(
    "data-volume",
    name="loreweaver-data",
    size=10,
    location=LOCATION,
    format="ext4",
    delete_protection=False,
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
# Server (cloud-init interpolates Floating IP + Volume device)
#
# TODO(k3s-migration): Replace Docker + Coolify install with k3s install: # noqa: FIX002, TD003
#   curl -sfL https://get.k3s.io | INSTALL_K3S_EXEC="..." sh -
#   (--data-dir /data/k3s --tls-san <fip> --node-external-ip <fip>)
# See docs/plans/2026-03-12-deployment-strategy.md
# ---------------------------------------------------------------------------
cloud_init = pulumi.Output.format(
    """\
#cloud-config
package_update: true
package_upgrade: true
apt:
  conf: |
    APT::Get::Assume-Yes "true";
    DPkg::Options:: "--force-confdef";
    DPkg::Options:: "--force-confold";
write_files:
  - path: /etc/network/interfaces.d/60-floating-ip.cfg
    content: |
      auto lo:1
      iface lo:1 inet static
        address {0}
        netmask 255.255.255.255
  - path: /etc/fstab
    append: true
    content: "{1} /data ext4 defaults,nofail 0 2"
  # Ubuntu 24.04 ships Docker 27.0.3 which has a broken IPv6 parser
  # that prevents Coolify's proxy from starting. Install from the
  # official repo first to get a working version.
  # https://github.com/coollabsio/coolify/issues/8649#issuecomment-3997077565
  - path: /opt/install-docker.sh
    permissions: "0755"
    content: |
      #!/bin/bash
      export DEBIAN_FRONTEND=noninteractive
      curl -fsSL https://get.docker.com | sh
  - path: /opt/install-coolify.sh
    permissions: "0755"
    content: |
      #!/bin/bash
      export DEBIAN_FRONTEND=noninteractive
      curl -fsSL https://cdn.coollabs.io/coolify/install.sh | bash
runcmd:
  - ip addr add {0}/32 dev lo
  - mkdir -p /data
  - mount /data || true
  - mkdir -p /data/campaigns /data/previews
  - /opt/install-docker.sh
  - /opt/install-coolify.sh
""",
    coolify_floating_ip.ip_address,
    coolify_volume.linux_device,
)

coolify_server = hcloud.Server(
    "server",
    name="loreweaver",
    server_type=SERVER_TYPE,
    image=IMAGE,
    location=LOCATION,
    ssh_keys=[personal_key.name, deploy_key.name],
    firewall_ids=[firewall.id.apply(int)],
    user_data=cloud_init,
    labels=LABELS,
)

# ---------------------------------------------------------------------------
# Floating IP Assignment (links floating_ip -> server)
# ---------------------------------------------------------------------------
_coolify_fip_assignment = hcloud.FloatingIpAssignment(
    "floating-ip-assignment",
    floating_ip_id=coolify_floating_ip.id.apply(int),
    server_id=coolify_server.id.apply(int),
)

# ---------------------------------------------------------------------------
# Volume Attachment (links volume -> server, no automount)
# ---------------------------------------------------------------------------
_coolify_volume_attachment = hcloud.VolumeAttachment(
    "volume-attachment",
    volume_id=coolify_volume.id.apply(int),
    server_id=coolify_server.id.apply(int),
    automount=False,
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

# TODO(k3s-migration): Remove coolify secrets after Phase 3 cutover # noqa: FIX002, TD003
coolify_api_token_secret = scaleway.secrets.Secret(
    "coolify-api-token-secret",
    name="coolify-api-token",
    description="Coolify API bearer token for deploy webhook auth",
    region="fr-par",
)

coolify_site_webhook_secret = scaleway.secrets.Secret(
    "coolify-site-webhook-secret",
    name="coolify-site-webhook",
    description="Coolify deploy webhook URL for the site resource",
    region="fr-par",
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
