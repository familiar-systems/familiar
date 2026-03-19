"""Shared configuration constants for Loreweaver cloud infrastructure."""

import base64

import pulumi
from pulumiverse_scaleway import secrets as scw_secrets

config = pulumi.Config()

LOCATION = "hel1"
SERVER_TYPE = "cx23"
IMAGE = "ubuntu-24.04"
LABELS = {"project": "loreweaver", "managed-by": "pulumi"}


def read_secret(name: str) -> pulumi.Output[str]:
    """Read a secret value from Scaleway Secrets Manager.

    Looks up the latest version of the named secret and returns the
    decoded plaintext as a Pulumi Output (automatically marked sensitive).
    """
    version = scw_secrets.get_version_output(
        secret_name=name,
        revision="latest",
        region="fr-par",
    )
    # SecretVersion.data is returned as base64 by the Scaleway API, but the
    # Pulumi provider's `data` INPUT field takes raw plaintext (not base64).
    return version.apply(lambda r: base64.b64decode(r.data).decode("utf-8"))
