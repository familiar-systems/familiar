# Coolify Setup Runbook

Post-infrastructure setup for Coolify on Hetzner VPS. Run after `pulumi up` provisions the server.

## Prerequisites

- Server provisioned via Pulumi (`pulumi up` completed)
- SSH access to server (`ssh root@$(pulumi stack output server_ip)`)
- Scaleway CLI configured (`scw init`)
- Pulumi stack outputs available (`pulumi stack output`)

## 1. Verify Coolify Installation

SSH into the server and confirm Coolify is running:

```bash
ssh root@$(pulumi stack output server_ip)
docker ps  # Should show coolify containers
```

If Coolify isn't running yet, cloud-init may still be executing:

```bash
cloud-init status --wait  # Wait for completion
docker ps                 # Check again
```

## 2. Access Coolify Dashboard

Coolify listens on port 8000 (not exposed in firewall). Access via SSH tunnel:

```bash
ssh -L 8000:localhost:8000 root@$(pulumi stack output server_ip)
# Open http://localhost:8000
```

## 3. Create Admin Account

On first access, Coolify prompts for admin account creation. Create the account and log in.

## 4. Enable API Access

1. Go to **Settings > Configuration > Advanced**
2. Enable **API Access**
3. Save

## 5. Create API Token

1. Go to **Security > API Tokens**
2. Create a new token with **Deploy** permission
3. Copy the token value

Fill the Pulumi-managed secret:

```bash
scw secret version create \
  secret-id="$(pulumi stack output coolify_api_token_secret_id)" \
  data="$(echo -n '<paste-token-here>' | base64)"
```

## 6. Configure Traefik DNS-01 (bunny.net)

This enables wildcard SSL certificates via DNS-01 ACME challenges.

1. Get your bunny.net API key from the [bunny.net dashboard](https://dash.bunny.net)
2. Fill the Pulumi-managed secret:

```bash
scw secret version create \
  secret-id="$(pulumi stack output bunny_api_key_secret_id)" \
  data="$(echo -n '<bunny-api-key>' | base64)"
```

3. In Coolify, configure Traefik for DNS-01:
   - Go to **Servers > localhost > Proxy > Dynamic Configurations**
   - Configure ACME DNS provider as `bunny` with the API key

## 7. Configure Scaleway Container Registry

1. Go to **Servers > localhost > Docker Registries**
2. Add new registry:
   - **URL:** `rg.fr-par.scw.cloud`
   - **Username:** `nologin`
   - **Password:** Your Scaleway secret key

## 8. Create Site Application Resource

1. Go to **Projects > Default > New Resource**
2. Select **Docker Image**
3. Configure:
   - **Image:** `rg.fr-par.scw.cloud/loreweaver/site:latest`
   - **Port:** `80`
   - **Domain:** `loreweaver.no`
   - **Auto-deploy:** Disabled (CD workflow triggers deploys via webhook)

## 9. Configure Deploy Webhook

1. In the application resource settings, find the **Webhook URL**
2. Copy the full webhook URL
3. Fill the Pulumi-managed secret:

```bash
scw secret version create \
  secret-id="$(pulumi stack output coolify_site_webhook_secret_id)" \
  data="$(echo -n '<webhook-url>' | base64)"
```

## 10. Configure DNS

Add an A record in bunny.net DNS:

| Type | Name | Value |
|------|------|-------|
| A    | @    | `<floating-ip>` |

Get the floating IP:

```bash
pulumi stack output floating_ip
```

## 11. Verify End-to-End

1. Push a commit to `main`
2. Watch CI workflow pass
3. Watch CD workflow trigger and complete
4. Verify site loads at `https://loreweaver.no`
5. Check SSL certificate is valid (issued via DNS-01)
6. Verify `/_astro/*` assets return `Cache-Control: public, immutable`
7. Verify `/nonexistent-page` returns 404 with custom error page
