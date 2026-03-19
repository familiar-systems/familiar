"""K3sCluster ComponentResource.

Provisions a single-node k3s cluster on Hetzner Cloud with automated
kubeconfig extraction. Encapsulates server, volume, and assignments
behind a typed interface. The floating IP is created externally
(cloud.py) and passed in.
"""

from __future__ import annotations

import pulumi
import pulumi_command as command
import pulumi_hcloud as hcloud


class K3sCluster(pulumi.ComponentResource):
    """Provisions a k3s server with automated kubeconfig extraction."""

    kubeconfig: pulumi.Output[str]
    server_ip: pulumi.Output[str]

    def __init__(
        self,
        name: str,
        *,
        floating_ip: hcloud.FloatingIp,
        firewall: hcloud.Firewall,
        ssh_keys: list[hcloud.SshKey],
        location: str,
        server_type: str,
        image: str,
        deploy_private_key: pulumi.Input[str],
        labels: dict[str, str],
        opts: pulumi.ResourceOptions | None = None,
    ) -> None:
        super().__init__("loreweaver:infra:K3sCluster", name, None, opts)

        child_opts = pulumi.ResourceOptions(parent=self)

        # -- Volume -----------------------------------------------------------
        volume = hcloud.Volume(
            f"{name}-data-volume",
            name=f"{name}-data",
            size=10,
            location=location,
            format="ext4",
            delete_protection=True,
            labels=labels,
            opts=child_opts,
        )

        # -- Cloud-init -------------------------------------------------------
        fip = floating_ip.ip_address
        tls_san_arg = fip.apply(lambda ip: f"--tls-san {ip}")

        cloud_init_script: pulumi.Output[str] = pulumi.Output.all(
            fip=fip,
            device=volume.linux_device,
            tls_sans=tls_san_arg,
        ).apply(
            lambda args: _render_cloud_init(
                fip=str(args["fip"]),  # pyright: ignore[reportAny]
                device=str(args["device"]),  # pyright: ignore[reportAny]
                tls_sans=str(args["tls_sans"]),  # pyright: ignore[reportAny]
            )
        )

        # -- Server -----------------------------------------------------------
        server = hcloud.Server(
            f"{name}-server",
            name=f"loreweaver-{name}",
            server_type=server_type,
            image=image,
            location=location,
            ssh_keys=[k.name for k in ssh_keys],
            firewall_ids=[firewall.id.apply(int)],
            user_data=cloud_init_script,
            labels=labels,
            # cloud-init only runs at first boot. Changing it should never
            # replace the server -- that cascades to the k8s provider and
            # every k8s resource, which Pulumi can't handle atomically.
            opts=pulumi.ResourceOptions(parent=self, ignore_changes=["user_data"]),
        )

        self.server_ip = server.ipv4_address

        # -- Floating IP Assignment -------------------------------------------
        _ = hcloud.FloatingIpAssignment(
            f"{name}-fip-assignment",
            floating_ip_id=floating_ip.id.apply(int),
            server_id=server.id.apply(int),
            opts=child_opts,
        )

        # -- Volume Attachment ------------------------------------------------
        _ = hcloud.VolumeAttachment(
            f"{name}-volume-attachment",
            volume_id=volume.id.apply(int),
            server_id=server.id.apply(int),
            automount=False,
            opts=child_opts,
        )

        # -- Kubeconfig extraction via SSH ------------------------------------
        # Waits for cloud-init to finish, then reads the k3s kubeconfig and
        # replaces 127.0.0.1 with the floating IP so it works remotely.
        kubeconfig_cmd = command.remote.Command(
            f"{name}-kubeconfig",
            connection=command.remote.ConnectionArgs(
                host=server.ipv4_address,
                user="root",
                private_key=deploy_private_key,
                dial_error_limit=30,
                per_dial_timeout=30,
            ),
            # cloud-init can take 3-5 minutes; wait for it, then extract
            create=fip.apply(
                lambda fip: (
                    "cloud-init status --wait > /dev/null 2>&1 && "
                    f"sed 's/127\\.0\\.0\\.1/{fip}/g' /etc/rancher/k3s/k3s.yaml"
                )
            ),
            # Re-extract on update (e.g. server replacement)
            update=fip.apply(
                lambda fip: f"sed 's/127\\.0\\.0\\.1/{fip}/g' /etc/rancher/k3s/k3s.yaml"
            ),
            triggers=[server.id],
            opts=pulumi.ResourceOptions(parent=self, additional_secret_outputs=["stdout"]),
        )

        self.kubeconfig = kubeconfig_cmd.stdout

        self.register_outputs(
            {
                "kubeconfig": self.kubeconfig,
                "serverIp": self.server_ip,
            }
        )


def _render_cloud_init(*, fip: str, device: str, tls_sans: str) -> str:
    """Render the cloud-init YAML for a k3s node."""
    return f"""\
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
        address {fip}
        netmask 255.255.255.255
  - path: /etc/rancher/k3s/config.yaml
    content: |
      kubelet-arg:
        - "container-log-max-files=10"
        - "container-log-max-size=50Mi"
  - path: /etc/fstab
    append: true
    content: "{device} /data ext4 defaults,nofail 0 2"
runcmd:
  - ip addr add {fip}/32 dev lo
  - mkdir -p /data
  - mount /data || true
  - mkdir -p /data/k3s /data/campaigns /data/preview
  - >-
    curl -sfL https://get.k3s.io |
    INSTALL_K3S_EXEC="{tls_sans}
    --data-dir /data/k3s
    --node-external-ip {fip}" sh -
"""
