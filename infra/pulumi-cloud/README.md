# Loreweaver Cloud Infrastructure

Pulumi (Python) project for provisioning Loreweaver's cloud infrastructure on Hetzner.

## Architecture

- **State backend**: Scaleway Object Storage (`s3://loreweaver-pulumi-state` in `fr-par`)
- **Secrets encryption**: Passphrase-based (AES-256-GCM), passphrase stored in Scaleway Secrets Manager
- **Infrastructure target**: Hetzner Cloud
- **CI secrets**: Scaleway credentials stored as GitHub repository secrets

Scaleway acts as the control plane (state + secrets), Hetzner as the data plane (compute + storage).

## Prerequisites

- [Pulumi CLI](https://www.pulumi.com/docs/install/)
- [uv](https://docs.astral.sh/uv/) (Python project manager)
- [scw](https://github.com/scaleway/scaleway-cli) (Scaleway CLI)
- [jq](https://jqlang.github.io/jq/)
- [direnv](https://direnv.net/)

## First-time setup (new project)

```bash
scw init                      # Configure Scaleway credentials
./scripts/bootstrap.sh        # Creates bucket, passphrase secret, and .envrc
direnv allow                  # Trust the generated .envrc
uv sync                       # Install Python dependencies
pulumi login                  # Connects using PULUMI_BACKEND_URL from .envrc
pulumi stack init prod        # Initialize the stack
```

## Machine setup (existing project)

```bash
scw init                      # Configure Scaleway credentials
./scripts/setup.sh            # Generates .envrc from existing resources
direnv allow
uv sync
pulumi login
```

## Usage

```bash
pulumi preview                # Dry-run
pulumi up                     # Apply changes
pulumi config set --secret <key> <value>   # Add an encrypted config value
```

## Scripts

| Script                 | Purpose                                                                      |
| ---------------------- | ---------------------------------------------------------------------------- |
| `scripts/bootstrap.sh` | One-time: creates Scaleway bucket + passphrase secret, then calls `setup.sh` |
| `scripts/setup.sh`     | Per-machine: generates `.envrc` from existing Scaleway resources             |

## How credentials flow

1. `scw init` writes Scaleway API keys to `~/.config/scw/config.yaml`
2. `setup.sh` generates `.envrc` that reads from scw config + Scaleway Secrets Manager
3. `direnv` evaluates `.envrc` on `cd`, exporting `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `PULUMI_CONFIG_PASSPHRASE`, and `PULUMI_BACKEND_URL`
4. Pulumi uses `AWS_*` vars to access Scaleway S3, passphrase to decrypt config secrets
5. Provider credentials (e.g. `hcloud:token`) are stored encrypted in `Pulumi.prod.yaml`
