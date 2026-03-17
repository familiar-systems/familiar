"""Kubernetes resources for the k3s preview cluster.

Deploys cert-manager (with bunny.net DNS-01 webhook), a wildcard TLS
certificate for *.preview.loreweaver.no, and the static site.

All resources use an explicit k8s Provider bound to the k3s cluster's
kubeconfig, so nothing touches a default/ambient kubeconfig.
"""

from __future__ import annotations

import base64
import json
import os

import pulumi
import pulumi_kubernetes as k8s
from pulumi_kubernetes.apiextensions import CustomResource

PREVIEW_DOMAIN = "preview.loreweaver.no"
CERT_MANAGER_VERSION = "v1.17.2"
WEBHOOK_BUNNY_VERSION = "1.0.3"
SITE_IMAGE_TAG = "latest"


def create_k8s_resources(
    *,
    kubeconfig: pulumi.Output[str],
    registry_endpoint: pulumi.Output[str],
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
        metadata=k8s.meta.v1.ObjectMetaArgs(name="cert-manager"),
        opts=k8s_opts,
    )

    cert_manager = k8s.helm.v3.Release(
        "cert-manager",
        chart="cert-manager",
        version=CERT_MANAGER_VERSION,
        namespace="cert-manager",
        repository_opts=k8s.helm.v3.RepositoryOptsArgs(
            repo="https://charts.jetstack.io",
        ),
        values={
            "crds": {"enabled": True},
        },
        opts=pulumi.ResourceOptions(provider=provider, depends_on=[cert_manager_ns]),
    )

    # -- bunny.net API key as k8s Secret (for the webhook) --------------------
    bunny_secret = k8s.core.v1.Secret(
        "bunny-api-key-secret",
        metadata=k8s.meta.v1.ObjectMetaArgs(
            name="bunny-api-key",
            namespace="cert-manager",
        ),
        type="Opaque",
        string_data={"api-key": bunny_api_key},
        opts=pulumi.ResourceOptions(
            provider=provider,
            depends_on=[cert_manager_ns],
        ),
    )

    # -- cert-manager-webhook-bunny -------------------------------------------
    webhook_bunny = k8s.helm.v3.Release(
        "cert-manager-webhook-bunny",
        chart="cert-manager-webhook-bunny",
        version=WEBHOOK_BUNNY_VERSION,
        namespace="cert-manager",
        repository_opts=k8s.helm.v3.RepositoryOptsArgs(
            repo="https://davidhidvegi.github.io/cert-manager-webhook-bunny/charts/",
        ),
        values={
            "bunny": {
                "apiKeySecretRef": {
                    "name": "bunny-api-key",
                    "key": "api-key",
                },
            },
        },
        opts=pulumi.ResourceOptions(
            provider=provider,
            depends_on=[cert_manager, bunny_secret],
        ),
    )

    # -- ClusterIssuer (Let's Encrypt staging first, switch to prod later) ----
    cluster_issuer = CustomResource(
        "letsencrypt-dns",
        api_version="cert-manager.io/v1",
        kind="ClusterIssuer",
        metadata={"name": "letsencrypt-dns"},
        spec={
            "acme": {
                "server": "https://acme-staging-v02.api.letsencrypt.org/directory",
                "email": acme_email,
                "privateKeySecretRef": {"name": "letsencrypt-dns-account-key"},
                "solvers": [
                    {
                        "dns01": {
                            "webhook": {
                                "groupName": "acme.bunny.net",
                                "solverName": "bunny",
                                "config": {
                                    "apiKeySecretRef": {
                                        "name": "bunny-api-key",
                                        "key": "api-key",
                                    },
                                },
                            },
                        },
                    },
                ],
            },
        },
        opts=pulumi.ResourceOptions(
            provider=provider,
            depends_on=[webhook_bunny],
        ),
    )

    # -- Wildcard Certificate -------------------------------------------------
    _wildcard_cert = CustomResource(
        "preview-wildcard-cert",
        api_version="cert-manager.io/v1",
        kind="Certificate",
        metadata={"name": "preview-wildcard-tls", "namespace": "default"},
        spec={
            "secretName": "preview-wildcard-tls",
            "issuerRef": {
                "name": "letsencrypt-dns",
                "kind": "ClusterIssuer",
            },
            "dnsNames": [
                PREVIEW_DOMAIN,
                f"*.{PREVIEW_DOMAIN}",
            ],
        },
        opts=pulumi.ResourceOptions(
            provider=provider,
            depends_on=[cluster_issuer],
        ),
    )

    # -- Scaleway Container Registry imagePullSecret --------------------------
    # Auth: username is always "nologin", password is SCW_SECRET_KEY.
    # See: https://www.scaleway.com/en/docs/container-registry/how-to/connect-docker-cli/
    scw_secret_key = pulumi.Output.secret(os.environ["SCW_SECRET_KEY"])
    docker_config = pulumi.Output.all(
        endpoint=registry_endpoint,
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
            name="scaleway-registry",
            namespace="default",
        ),
        type="kubernetes.io/dockerconfigjson",
        string_data={".dockerconfigjson": docker_config},
        opts=k8s_opts,
    )

    # -- Site Deployment + Service + Ingress ----------------------------------
    site_labels = {"app": "site"}

    site_image = registry_endpoint.apply(lambda ep: f"{ep}/site:{SITE_IMAGE_TAG}")

    _site_deployment = k8s.apps.v1.Deployment(
        "site-deployment",
        metadata=k8s.meta.v1.ObjectMetaArgs(
            name="site",
            namespace="default",
        ),
        spec=k8s.apps.v1.DeploymentSpecArgs(
            replicas=1,
            selector=k8s.meta.v1.LabelSelectorArgs(match_labels=site_labels),
            template=k8s.core.v1.PodTemplateSpecArgs(
                metadata=k8s.meta.v1.ObjectMetaArgs(labels=site_labels),
                spec=k8s.core.v1.PodSpecArgs(
                    image_pull_secrets=[
                        k8s.core.v1.LocalObjectReferenceArgs(name="scaleway-registry"),
                    ],
                    containers=[
                        k8s.core.v1.ContainerArgs(
                            name="site",
                            image=site_image,
                            image_pull_policy="IfNotPresent",
                            ports=[k8s.core.v1.ContainerPortArgs(container_port=80)],
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
            name="site",
            namespace="default",
        ),
        spec=k8s.core.v1.ServiceSpecArgs(
            selector=site_labels,
            ports=[
                k8s.core.v1.ServicePortArgs(
                    port=80,
                    target_port=80,
                ),
            ],
        ),
        opts=k8s_opts,
    )

    _site_ingress = k8s.networking.v1.Ingress(
        "site-ingress",
        metadata=k8s.meta.v1.ObjectMetaArgs(
            name="site",
            namespace="default",
            annotations={
                "traefik.ingress.kubernetes.io/router.entrypoints": "websecure",
            },
        ),
        spec=k8s.networking.v1.IngressSpecArgs(
            tls=[
                k8s.networking.v1.IngressTLSArgs(
                    hosts=[PREVIEW_DOMAIN],
                    secret_name="preview-wildcard-tls",  # noqa: S106
                ),
            ],
            rules=[
                k8s.networking.v1.IngressRuleArgs(
                    host=PREVIEW_DOMAIN,
                    http=k8s.networking.v1.HTTPIngressRuleValueArgs(
                        paths=[
                            k8s.networking.v1.HTTPIngressPathArgs(
                                path="/",
                                path_type="Prefix",
                                backend=k8s.networking.v1.IngressBackendArgs(
                                    service=k8s.networking.v1.IngressServiceBackendArgs(
                                        name="site",
                                        port=k8s.networking.v1.ServiceBackendPortArgs(
                                            number=80,
                                        ),
                                    ),
                                ),
                            ),
                        ],
                    ),
                ),
            ],
        ),
        opts=k8s_opts,
    )


def _docker_config_json(*, registry: str, username: str, password: str) -> str:
    """Build a Docker config.json for imagePullSecrets."""
    auth = base64.b64encode(f"{username}:{password}".encode()).decode()
    return json.dumps({"auths": {registry: {"auth": auth}}})
