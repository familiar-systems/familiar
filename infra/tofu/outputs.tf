output "k3s_floating_ip" {
  value = hcloud_floating_ip.main.ip_address
}

output "k3s_kubeconfig_secret_id" {
  value = scaleway_secret.k3s_kubeconfig.id
}
