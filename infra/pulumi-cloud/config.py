"""Shared configuration constants for familiar.systems cloud infrastructure."""

import base64

import pulumi
from pulumiverse_scaleway import secrets as scw_secrets

config = pulumi.Config()

LOCATION = "hel1"
SERVER_TYPE = "cx23"
IMAGE = "ubuntu-24.04"
LABELS = {"project": "familiar-systems", "managed-by": "pulumi"}


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
    decoded = version.apply(lambda r: base64.b64decode(r.data).decode("utf-8"))
    return pulumi.Output.secret(decoded)
