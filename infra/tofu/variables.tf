variable "state_encryption_passphrase" {
  description = "Passphrase for OpenTofu state encryption (set via TF_VAR_state_encryption_passphrase)"
  type        = string
  sensitive   = true
}

variable "personal_ssh_public_key" {
  description = "SSH public key for personal desktop access"
  type        = string
}

variable "deploy_ssh_public_key" {
  description = "SSH public key for break-glass deploy access"
  type        = string
}

variable "acme_email" {
  description = "Email address for ACME certificate registration"
  type        = string
}

variable "hetzner_project_id" {
  description = "Numeric Hetzner Cloud project ID (used in S3 bucket policy principal ARNs)"
  type        = string
}
