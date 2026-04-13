"""Kubernetes resources for the k3s preview + production cluster.

Deploys cert-manager (with bunny.net DNS-01 webhook), a TLS certificate
covering all production and preview domains, and the static site.

All resources use an explicit k8s Provider bound to the k3s cluster's
kubeconfig, so nothing touches a default/ambient kubeconfig.
"""

from __future__ import annotations

import base64
import json
import os
from typing import TYPE_CHECKING

import pulumi
import pulumi_kubernetes as k8s

# CRD specs (ClusterIssuer, Certificate) use CustomResource -- not ConfigGroup,
# which can't resolve CRD schemas before cert-manager is installed. The specs
# are untyped dicts; always read upstream docs before modifying.
from pulumi_kubernetes.apiextensions import CustomResource

from config import PREVIEW_DOMAINS, PRODUCTION_DOMAINS

if TYPE_CHECKING:
    import pulumiverse_scaleway as scaleway

CERT_MANAGER_VERSION = "v1.17.2"
WEBHOOK_BUNNY_VERSION = "1.0.3"
WEBHOOK_BUNNY_GROUP_NAME = "com.bunny.webhook"
SITE_IMAGE_TAG = "latest"

# Cross-resource references: these names link resources together.
CERT_MANAGER_NS = "cert-manager"
BUNNY_SECRET_NAME = "bunny-api-key"  # noqa: S105
BUNNY_SECRET_KEY = "api-key"  # noqa: S105
WILDCARD_CERT_SECRET = "preview-wildcard-tls"  # noqa: S105
REGISTRY_PULL_SECRET = "scaleway-registry"  # noqa: S105
SITE_NAME = "site"
SITE_PORT = 80

# ACME issuers. Both ClusterIssuers are created; the wildcard cert references
# whichever name is in ACTIVE_CLUSTER_ISSUER_NAME. Switch to staging during
# infra changes that could re-issue the cert (rate limits on prod are 50/week);
# switch back to prod once changes are verified.
LETSENCRYPT_PROD_URL = "https://acme-v02.api.letsencrypt.org/directory"
LETSENCRYPT_STAGING_URL = "https://acme-staging-v02.api.letsencrypt.org/directory"
PROD_CLUSTER_ISSUER_NAME = "letsencrypt-dns"
STAGING_CLUSTER_ISSUER_NAME = "letsencrypt-staging-dns"
ACTIVE_CLUSTER_ISSUER_NAME = PROD_CLUSTER_ISSUER_NAME


def create_k8s_resources(
    *,
    kubeconfig: pulumi.Output[str],
    registry: scaleway.registry.Namespace,
    bunny_api_key: pulumi.Input[str],
    acme_email: str,
) -> None:
    """Declare all Kubernetes resources for the preview cluster."""
    # -- Provider -------------------------------------------------------------
    provider = k8s.Provider(
        "k3s-provider",
        kubeconfig=kubeconfig,
    )
    k8s_opts = pulumi.ResourceOptions(provider=provider)

    # -- cert-manager ---------------------------------------------------------
    cert_manager_ns = k8s.core.v1.Namespace(
        "cert-manager-ns",
        metadata=k8s.meta.v1.ObjectMetaArgs(name=CERT_MANAGER_NS),
        opts=k8s_opts,
    )

    cert_manager = k8s.helm.v3.Release(
        "cert-manager",
        chart="cert-manager",
        version=CERT_MANAGER_VERSION,
        namespace=CERT_MANAGER_NS,
        repository_opts=k8s.helm.v3.RepositoryOptsArgs(
            repo="https://charts.jetstack.io",
        ),
        values={
            "crds": {"enabled": True},
            # The post-install startupapicheck job verifies the cert-manager
            # API is responsive. It times out on small servers (CX23) where
            # the webhook takes longer than 1 min to become ready. Disabling
            # it is safe -- cert-manager itself works fine; only the
            # smoke-test job fails.
            "startupapicheck": {"enabled": False},
        },
        opts=pulumi.ResourceOptions(provider=provider, depends_on=[cert_manager_ns]),
    )

    # -- bunny.net API key as k8s Secret (for the webhook) --------------------
    bunny_secret = k8s.core.v1.Secret(
        "bunny-api-key-secret",
        metadata=k8s.meta.v1.ObjectMetaArgs(
            name=BUNNY_SECRET_NAME,
            namespace=CERT_MANAGER_NS,
        ),
        type="Opaque",
        string_data={BUNNY_SECRET_KEY: bunny_api_key},
        opts=pulumi.ResourceOptions(
            provider=provider,
            depends_on=[cert_manager_ns],
        ),
    )

    # -- cert-manager-webhook-bunny -------------------------------------------
    # The webhook needs to know the cert-manager SA name for RBAC.
    # Pulumi suffixes Helm release names, so the SA isn't just "cert-manager".
    cert_manager_sa = cert_manager.status.apply(lambda s: str(s.name) if s else "cert-manager")

    webhook_bunny = k8s.helm.v3.Release(
        "cert-manager-webhook-bunny",
        chart="cert-manager-webhook-bunny",
        version=WEBHOOK_BUNNY_VERSION,
        namespace=CERT_MANAGER_NS,
        repository_opts=k8s.helm.v3.RepositoryOptsArgs(
            repo="https://davidhidvegi.github.io/cert-manager-webhook-bunny/charts/",
        ),
        values={
            "groupName": WEBHOOK_BUNNY_GROUP_NAME,
            "certManager": {
                "serviceAccountName": cert_manager_sa,
            },
            "bunny": {
                "apiKeySecretRef": {
                    "name": BUNNY_SECRET_NAME,
                    "key": BUNNY_SECRET_KEY,
                },
            },
        },
        opts=pulumi.ResourceOptions(
            provider=provider,
            depends_on=[cert_manager, bunny_secret],
        ),
    )

    # -- ClusterIssuers (prod + staging) --------------------------------------
    # Both are created so the wildcard cert can switch between them via
    # ACTIVE_CLUSTER_ISSUER_NAME. Use staging during infra changes that could
    # trigger cert re-issuance; switch back to prod once verified.
    prod_cluster_issuer = CustomResource(
        PROD_CLUSTER_ISSUER_NAME,
        api_version="cert-manager.io/v1",
        kind="ClusterIssuer",
        metadata={"name": PROD_CLUSTER_ISSUER_NAME},
        spec=_acme_cluster_issuer_spec(
            server_url=LETSENCRYPT_PROD_URL,
            account_key_secret_name="letsencrypt-dns-account-key",  # noqa: S106
            email=acme_email,
        ),
        opts=pulumi.ResourceOptions(
            provider=provider,
            depends_on=[webhook_bunny],
        ),
    )

    staging_cluster_issuer = CustomResource(
        STAGING_CLUSTER_ISSUER_NAME,
        api_version="cert-manager.io/v1",
        kind="ClusterIssuer",
        metadata={"name": STAGING_CLUSTER_ISSUER_NAME},
        spec=_acme_cluster_issuer_spec(
            server_url=LETSENCRYPT_STAGING_URL,
            account_key_secret_name="letsencrypt-staging-dns-account-key",  # noqa: S106
            email=acme_email,
        ),
        opts=pulumi.ResourceOptions(
            provider=provider,
            depends_on=[webhook_bunny],
        ),
    )

    # -- Wildcard Certificate -------------------------------------------------
    # SANs cover both production and preview domains plus the preview wildcard
    # subdomains. Adding a domain to PRODUCTION_DOMAINS or PREVIEW_DOMAINS in
    # config.py extends the cert automatically on next pulumi up.
    _wildcard_cert = CustomResource(
        "preview-wildcard-cert",
        api_version="cert-manager.io/v1",
        kind="Certificate",
        metadata={"name": WILDCARD_CERT_SECRET, "namespace": "default"},
        spec={
            "secretName": WILDCARD_CERT_SECRET,
            "issuerRef": {
                "name": ACTIVE_CLUSTER_ISSUER_NAME,
                "kind": "ClusterIssuer",
            },
            "dnsNames": [
                *PRODUCTION_DOMAINS,
                *PREVIEW_DOMAINS,
                *[f"*.{d}" for d in PREVIEW_DOMAINS],
            ],
        },
        opts=pulumi.ResourceOptions(
            provider=provider,
            depends_on=[prod_cluster_issuer, staging_cluster_issuer],
        ),
    )

    # -- Scaleway Container Registry imagePullSecret --------------------------
    # Auth: username is always "nologin", password is SCW_SECRET_KEY.
    # See: https://www.scaleway.com/en/docs/container-registry/how-to/connect-docker-cli/
    scw_secret_key = pulumi.Output.secret(os.environ["SCW_SECRET_KEY"])
    docker_config = pulumi.Output.all(
        endpoint=registry.endpoint,
        password=scw_secret_key,
    ).apply(
        lambda args: _docker_config_json(
            registry=str(args["endpoint"]),  # pyright: ignore[reportAny]
            username="nologin",
            password=str(args["password"]),  # pyright: ignore[reportAny]
        )
    )

    image_pull_secret = k8s.core.v1.Secret(
        "registry-pull-secret",
        metadata=k8s.meta.v1.ObjectMetaArgs(
            name=REGISTRY_PULL_SECRET,
            namespace="default",
        ),
        type="kubernetes.io/dockerconfigjson",
        string_data={".dockerconfigjson": docker_config},
        opts=k8s_opts,
    )

    # -- Site Deployment + Service + Ingress ----------------------------------
    site_labels = {"app": SITE_NAME}

    site_image = registry.endpoint.apply(lambda ep: f"{ep}/{SITE_NAME}:{SITE_IMAGE_TAG}")

    _site_deployment = k8s.apps.v1.Deployment(
        "site-deployment",
        metadata=k8s.meta.v1.ObjectMetaArgs(
            name=SITE_NAME,
            namespace="default",
        ),
        spec=k8s.apps.v1.DeploymentSpecArgs(
            replicas=1,
            selector=k8s.meta.v1.LabelSelectorArgs(match_labels=site_labels),
            template=k8s.core.v1.PodTemplateSpecArgs(
                metadata=k8s.meta.v1.ObjectMetaArgs(labels=site_labels),
                spec=k8s.core.v1.PodSpecArgs(
                    image_pull_secrets=[
                        k8s.core.v1.LocalObjectReferenceArgs(name=REGISTRY_PULL_SECRET),
                    ],
                    containers=[
                        k8s.core.v1.ContainerArgs(
                            name=SITE_NAME,
                            image=site_image,
                            image_pull_policy="IfNotPresent",
                            ports=[k8s.core.v1.ContainerPortArgs(container_port=SITE_PORT)],
                            resources=k8s.core.v1.ResourceRequirementsArgs(
                                requests={"cpu": "10m", "memory": "32Mi"},
                                limits={"memory": "64Mi"},
                            ),
                        ),
                    ],
                ),
            ),
        ),
        opts=pulumi.ResourceOptions(provider=provider, depends_on=[image_pull_secret]),
    )

    _site_service = k8s.core.v1.Service(
        "site-service",
        metadata=k8s.meta.v1.ObjectMetaArgs(
            name=SITE_NAME,
            namespace="default",
        ),
        spec=k8s.core.v1.ServiceSpecArgs(
            selector=site_labels,
            ports=[
                k8s.core.v1.ServicePortArgs(
                    port=SITE_PORT,
                    target_port=SITE_PORT,
                ),
            ],
        ),
        opts=k8s_opts,
    )

    all_site_hosts = [*PRODUCTION_DOMAINS, *PREVIEW_DOMAINS]
    _site_ingress = k8s.networking.v1.Ingress(
        "site-ingress",
        metadata=k8s.meta.v1.ObjectMetaArgs(
            name=SITE_NAME,
            namespace="default",
            annotations={
                "traefik.ingress.kubernetes.io/router.entrypoints": "websecure",
            },
        ),
        spec=k8s.networking.v1.IngressSpecArgs(
            tls=[
                k8s.networking.v1.IngressTLSArgs(
                    hosts=[host],
                    secret_name=WILDCARD_CERT_SECRET,
                )
                for host in all_site_hosts
            ],
            rules=[_site_ingress_rule(host) for host in all_site_hosts],
        ),
        opts=k8s_opts,
    )


def _site_ingress_rule(host: str) -> k8s.networking.v1.IngressRuleArgs:
    """Build an Ingress rule routing all traffic for *host* to the site service."""
    return k8s.networking.v1.IngressRuleArgs(
        host=host,
        http=k8s.networking.v1.HTTPIngressRuleValueArgs(
            paths=[
                k8s.networking.v1.HTTPIngressPathArgs(
                    path="/",
                    path_type="Prefix",
                    backend=k8s.networking.v1.IngressBackendArgs(
                        service=k8s.networking.v1.IngressServiceBackendArgs(
                            name=SITE_NAME,
                            port=k8s.networking.v1.ServiceBackendPortArgs(
                                number=SITE_PORT,
                            ),
                        ),
                    ),
                ),
            ],
        ),
    )


def _docker_config_json(*, registry: str, username: str, password: str) -> str:
    """Build a Docker config.json for imagePullSecrets."""
    auth = base64.b64encode(f"{username}:{password}".encode()).decode()
    return json.dumps({"auths": {registry: {"auth": auth}}})


def _acme_cluster_issuer_spec(
    *,
    server_url: str,
    account_key_secret_name: str,
    email: str,
) -> dict[str, object]:
    """Build a ClusterIssuer spec for an ACME issuer using the bunny DNS-01 webhook."""
    return {
        "acme": {
            "server": server_url,
            "email": email,
            "privateKeySecretRef": {"name": account_key_secret_name},
            "solvers": [
                {
                    "dns01": {
                        "webhook": {
                            "groupName": WEBHOOK_BUNNY_GROUP_NAME,
                            "solverName": "bunny",
                            "config": {
                                "secretRef": BUNNY_SECRET_NAME,
                                "secretNamespace": CERT_MANAGER_NS,
                            },
                        },
                    },
                },
            ],
        },
    }
