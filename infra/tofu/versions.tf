terraform {
  required_version = ">= 1.11"

  required_providers {
    hcloud = {
      source = "hetznercloud/hcloud"
    }
    scaleway = {
      source = "scaleway/scaleway"
    }
    minio = {
      source = "aminueza/minio"
    }
  }

  backend "s3" {
    bucket                      = "familiar-systems-tofu-state"
    key                         = "prod/terraform.tfstate"
    region                      = "fr-par"
    endpoints                   = { s3 = "https://s3.fr-par.scw.cloud" }
    skip_credentials_validation = true
    skip_region_validation      = true
    skip_requesting_account_id  = true
    skip_metadata_api_check     = true
    skip_s3_checksum            = true
  }

  encryption {
    key_provider "pbkdf2" "main" {
      passphrase = var.state_encryption_passphrase
    }
    method "aes_gcm" "main" {
      keys = key_provider.pbkdf2.main
    }
    state {
      method   = method.aes_gcm.main
      enforced = true
    }
    plan {
      method   = method.aes_gcm.main
      enforced = true
    }
  }
}
