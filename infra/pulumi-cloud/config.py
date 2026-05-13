"""Shared configuration constants for familiar.systems cloud infrastructure."""

import base64

import pulumi
from pulumiverse_scaleway import secrets as scw_secrets

config = pulumi.Config()

LOCATION = "hel1"
SERVER_TYPE = "cx23"
IMAGE = "ubuntu-24.04"
LABELS = {"project": "familiar-systems", "managed-by": "pulumi"}

# Domains served by the cluster. Add to these lists to extend coverage.
# The TLS cert (k8s.py) and Ingress rules (k8s.py) iterate over them.
# Prerequisite: the bunny.net account managing `bunny-api-key` must control
# the DNS zone for any domain added here, so DNS-01 ACME challenges work.
#
# Each environment terminates traffic on two apexes: a marketing apex (Astro
# site) and an app apex (SPA, platform API, campaign shards). Routing within
# each apex is path-based. See docs/plans/2026-04-11-app-server-prd.md
# "URL architecture". Hanko tenant subdomains (auth.*, auth.preview.*) are
# not listed here because Hanko manages their DNS and TLS.
MARKETING_PROD_DOMAINS: list[str] = [
    "loreweaver.no",
    "familiar.systems",
]
APP_PROD_DOMAINS: list[str] = ["app.familiar.systems"]

MARKETING_PREVIEW_DOMAINS: list[str] = [
    "preview.loreweaver.no",
    "preview.familiar.systems",
]
APP_PREVIEW_DOMAINS: list[str] = ["app.preview.familiar.systems"]

# Aggregates - used by the TLS cert's dnsNames and anywhere the full set of
# SANs is needed.
PRODUCTION_DOMAINS: list[str] = [*MARKETING_PROD_DOMAINS, *APP_PROD_DOMAINS]
PREVIEW_DOMAINS: list[str] = [*MARKETING_PREVIEW_DOMAINS, *APP_PREVIEW_DOMAINS]

# Hanko tenant URL for production (public per plan §4.8 -- appears in TLS SNI
# on every browser request). Custom domain CNAMEd to the prod Hanko tenant.
# The contributor preview URL (auth.preview.familiar.systems) is intentionally
# not declared here: Pulumi has no consumer for it. PR previews are GHA-driven
# and read the dev URL from .github/workflows/deploy-preview.yml, which in
# turn names mise.toml [env].HANKO_API_URL_DEV as the canonical source.
HANKO_API_URL_PROD: str = "https://auth.familiar.systems"


def read_secret(name: str) -> pulumi.Output[str]:
    """Read a secret value from Scaleway Secrets Manager.

    Looks up the latest version of the named secret and returns the
    decoded plaintext as a Pulumi Output explicitly marked sensitive so
    it is redacted in `pulumi preview` / `pulumi up` output.
    """
    version = scw_secrets.get_version_output(
        secret_name=name,
        revision="latest",
        region="fr-par",
    )
    # SecretVersion.data is base64 from the Scaleway API. `.apply` does not
    # reliably preserve the secret flag through the transform, so we wrap
    # the result with `Output.secret(...)` to mark it sensitive ourselves --
    # otherwise the plaintext can surface verbatim when the value flows into
    # a resource input (e.g. as a Provider credential).
    decoded = version.apply(lambda r: base64.b64decode(r.data).decode("utf-8"))
    return pulumi.Output.secret(decoded)
