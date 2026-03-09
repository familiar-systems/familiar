"""Loreweaver cloud infrastructure.

Provisions Hetzner Cloud VPS with Coolify, Floating IP, Volume,
and Scaleway Container Registry for image storage.

Resources (11):
  1a. SshKey (personal) — Desktop SSH key for daily access
  1b. SshKey (deploy)   — Break-glass key, private key in Scaleway SM
  2. FloatingIp         — IPv4 in fsn1 (zero-downtime server replacement)
  3. Volume             — 10GB ext4 for persistent data (/data)
  4. Firewall           — Inbound TCP 22/80/443 + ICMP
  5. Server             — CAX11 (ARM) with cloud-init (fstab, loopback IP, Coolify)
  6. FloatingIpAssignment — Links Floating IP → Server
  7. VolumeAttachment   — Links Volume → Server (no automount)
  8. RegistryNamespace  — Scaleway Container Registry (private, fr-par)
  9. Secret             — Empty shell for deploy SSH private key (you fill it)

Dependency graph (no cycles):
  FloatingIp ──→ Server ←── Volume    (cloud-init reads ip_address / linux_device)
  SshKeys ───────→ Server ←── Firewall
  FloatingIp + Server ──→ FloatingIpAssignment
  Volume + Server ──→ VolumeAttachment
"""

import pulumi
import pulumi_hcloud as hcloud
import pulumiverse_scaleway as scaleway

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------
config = pulumi.Config()
personal_ssh_key = config.require("personal-ssh-public-key")
deploy_ssh_key = config.require("deploy-ssh-public-key")

LOCATION = "hel1"
SERVER_TYPE = "cax11"
IMAGE = "ubuntu-24.04"
LABELS = {"project": "loreweaver", "managed-by": "pulumi"}


# ---------------------------------------------------------------------------
# 1. SSH Keys (both registered — personal for desktop, deploy for break-glass)
# ---------------------------------------------------------------------------
personal_key = hcloud.SshKey(
    "personal-ssh-key",
    name="loreweaver-personal",
    public_key=personal_ssh_key,
    labels=LABELS,
)

deploy_key = hcloud.SshKey(
    "deploy-ssh-key",
    name="loreweaver-deploy",
    public_key=deploy_ssh_key,
    labels=LABELS,
)

# ---------------------------------------------------------------------------
# 2. Floating IP (location only — no server_id, avoids circular dep)
# ---------------------------------------------------------------------------
floating_ip = hcloud.FloatingIp(
    "floating-ip",
    type="ipv4",
    home_location=LOCATION,
    description="Loreweaver production IP",
    labels=LABELS,
)

# ---------------------------------------------------------------------------
# 3. Volume (location only — no server_id, avoids circular dep)
# ---------------------------------------------------------------------------
volume = hcloud.Volume(
    "data-volume",
    name="loreweaver-data",
    size=10,
    location=LOCATION,
    format="ext4",
    delete_protection=True,
    labels=LABELS,
)

# ---------------------------------------------------------------------------
# 4. Firewall
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
    ],
    labels=LABELS,
)


# ---------------------------------------------------------------------------
# 5. Server (cloud-init interpolates Floating IP + Volume device)
# ---------------------------------------------------------------------------
cloud_init = pulumi.Output.format(
    """\
#cloud-config
package_update: true
package_upgrade: true
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
runcmd:
  - ip addr add {0}/32 dev lo
  - mkdir -p /data
  - mount /data || true
  - mkdir -p /data/campaigns /data/previews
  - curl -fsSL https://cdn.coollabs.io/coolify/install.sh | bash
""",
    floating_ip.ip_address,
    volume.linux_device,
)

server = hcloud.Server(
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
# 6. Floating IP Assignment (links floating_ip → server)
# ---------------------------------------------------------------------------
_ = hcloud.FloatingIpAssignment(
    "floating-ip-assignment",
    floating_ip_id=floating_ip.id.apply(int),
    server_id=server.id.apply(int),
)

# ---------------------------------------------------------------------------
# 7. Volume Attachment (links volume → server, no automount)
# ---------------------------------------------------------------------------
_ = hcloud.VolumeAttachment(
    "volume-attachment",
    volume_id=volume.id.apply(int),
    server_id=server.id.apply(int),
    automount=False,
)

# ---------------------------------------------------------------------------
# 8. Scaleway Container Registry
# ---------------------------------------------------------------------------
registry = scaleway.registry.Namespace(
    "container-registry",
    name="loreweaver",
    description="Loreweaver container images",
    is_public=False,
    region="fr-par",
)

# ---------------------------------------------------------------------------
# 9. Scaleway Secret (empty container for deploy SSH private key)
#    Pulumi owns the resource; you fill it manually. See CLAUDE.md.
# ---------------------------------------------------------------------------
deploy_ssh_secret = scaleway.secrets.Secret(
    "deploy-ssh-secret",
    name="loreweaver-deploy-ssh-key",
    description="Break-glass SSH private key for server access",
    region="fr-par",
    protected=True,
)

# ---------------------------------------------------------------------------
# Exports
# ---------------------------------------------------------------------------
pulumi.export("floating_ip", floating_ip.ip_address)
pulumi.export("server_ip", server.ipv4_address)
pulumi.export("server_id", server.id)
pulumi.export("volume_id", volume.id)
pulumi.export("volume_linux_device", volume.linux_device)
pulumi.export("registry_endpoint", registry.endpoint)
pulumi.export("deploy_ssh_secret_id", deploy_ssh_secret.id)
