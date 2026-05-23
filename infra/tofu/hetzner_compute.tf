locals {
  location    = "hel1"
  server_type = "cx23"
  image       = "ubuntu-24.04"
  labels      = { project = "familiar-systems", managed-by = "opentofu" }
}

# -----------------------------------------------------------------------------
# SSH Keys (both registered; personal for desktop, deploy for break-glass)
# -----------------------------------------------------------------------------

resource "hcloud_ssh_key" "personal" {
  name       = "loreweaver-personal"
  public_key = var.personal_ssh_public_key
  labels     = local.labels
}

resource "hcloud_ssh_key" "deploy" {
  name       = "loreweaver-deploy"
  public_key = var.deploy_ssh_public_key
  labels     = local.labels
}

# -----------------------------------------------------------------------------
# Floating IP (public entry point; DNS A records for all apexes point here)
# -----------------------------------------------------------------------------

resource "hcloud_floating_ip" "main" {
  type          = "ipv4"
  home_location = local.location
  description   = "Public IP for familiar.systems (DNS, TLS, ingress)"
  labels        = local.labels
}

# -----------------------------------------------------------------------------
# Firewall
# -----------------------------------------------------------------------------

resource "hcloud_firewall" "main" {
  name   = "loreweaver-fw"
  labels = local.labels

  rule {
    direction   = "in"
    protocol    = "icmp"
    source_ips  = ["0.0.0.0/0", "::/0"]
    description = "Allow ping"
  }

  rule {
    direction   = "in"
    protocol    = "tcp"
    port        = "22"
    source_ips  = ["0.0.0.0/0", "::/0"]
    description = "Allow SSH"
  }

  rule {
    direction   = "in"
    protocol    = "tcp"
    port        = "80"
    source_ips  = ["0.0.0.0/0", "::/0"]
    description = "Allow HTTP"
  }

  rule {
    direction   = "in"
    protocol    = "tcp"
    port        = "443"
    source_ips  = ["0.0.0.0/0", "::/0"]
    description = "Allow HTTPS"
  }

  rule {
    direction   = "in"
    protocol    = "tcp"
    port        = "6443"
    source_ips  = ["0.0.0.0/0", "::/0"]
    description = "Allow k3s API server"
  }
}

# -----------------------------------------------------------------------------
# Volume (10 GB, /data/k3s + /data/campaigns + /data/preview)
# -----------------------------------------------------------------------------

resource "hcloud_volume" "data" {
  name              = "k3s-data"
  size              = 10
  location          = local.location
  format            = "ext4"
  delete_protection = true
  labels            = local.labels
}

# -----------------------------------------------------------------------------
# Server (k3s, cloud-init bootstrap)
# -----------------------------------------------------------------------------

resource "hcloud_server" "k3s" {
  name         = "loreweaver-k3s"
  server_type  = local.server_type
  image        = local.image
  location     = local.location
  ssh_keys     = [hcloud_ssh_key.personal.name, hcloud_ssh_key.deploy.name]
  firewall_ids = [hcloud_firewall.main.id]
  user_data    = local.cloud_init_script
  labels       = local.labels

  lifecycle {
    # user_data: cloud-init only runs at first boot; changes are meaningless
    #   on an existing server.
    # ssh_keys: ForceNew in the hcloud provider (hetznercloud/terraform-provider-hcloud#428).
    #   Hetzner injects keys into authorized_keys at creation and then forgets
    #   which hcloud_ssh_key resources were used. Import leaves this field
    #   empty, so any declared value looks like a change and forces server
    #   replacement. The keys are physically on the server already. Keeping
    #   ssh_keys in the resource block so fresh creates get them; ignoring
    #   changes so the imported server isn't destroyed.
    ignore_changes = [user_data, ssh_keys]
  }
}

locals {
  cloud_init_script = <<-CLOUDINIT
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
            address ${hcloud_floating_ip.main.ip_address}
            netmask 255.255.255.255
      - path: /etc/rancher/k3s/config.yaml
        content: |
          kubelet-arg:
            - "container-log-max-files=10"
            - "container-log-max-size=50Mi"
      - path: /etc/fstab
        append: true
        content: "${hcloud_volume.data.linux_device} /data ext4 defaults,nofail 0 2"
      # TODO: rename to tofu-admin at next server replacement (cloud-init only runs at first boot)
      - path: /var/lib/rancher/k3s/server/manifests/pulumi-admin.yaml
        content: |
          apiVersion: v1
          kind: ServiceAccount
          metadata:
            name: pulumi-admin
            namespace: kube-system
          ---
          apiVersion: rbac.authorization.k8s.io/v1
          kind: ClusterRoleBinding
          metadata:
            name: pulumi-admin-cluster-admin
          roleRef:
            apiGroup: rbac.authorization.k8s.io
            kind: ClusterRole
            name: cluster-admin
          subjects:
            - kind: ServiceAccount
              name: pulumi-admin
              namespace: kube-system
          ---
          apiVersion: v1
          kind: Secret
          metadata:
            name: pulumi-admin-token
            namespace: kube-system
            annotations:
              kubernetes.io/service-account.name: pulumi-admin
          type: kubernetes.io/service-account-token
    runcmd:
      - ip addr add ${hcloud_floating_ip.main.ip_address}/32 dev lo
      - mkdir -p /data
      - mount /data || true
      - mkdir -p /data/k3s /data/campaigns /data/preview
      - >-
        curl -sfL https://get.k3s.io |
        INSTALL_K3S_EXEC="--tls-san ${hcloud_floating_ip.main.ip_address}
        --data-dir /data/k3s
        --node-external-ip ${hcloud_floating_ip.main.ip_address}
        --node-label node-role.familiar.systems/role=platform" sh -
  CLOUDINIT
}

# -----------------------------------------------------------------------------
# Floating IP Assignment
# -----------------------------------------------------------------------------

resource "hcloud_floating_ip_assignment" "main" {
  floating_ip_id = hcloud_floating_ip.main.id
  server_id      = hcloud_server.k3s.id
}

# -----------------------------------------------------------------------------
# Volume Attachment
# -----------------------------------------------------------------------------

resource "hcloud_volume_attachment" "data" {
  volume_id = hcloud_volume.data.id
  server_id = hcloud_server.k3s.id
  automount = false
}
