"""Kubernetes resources for the k3s preview + production cluster.

Deploys cert-manager (with bunny.net DNS-01 webhook), a TLS certificate
covering all production and preview domains, and the static site.

All resources use an explicit k8s Provider bound to the k3s cluster's
kubeconfig, so nothing touches a default/ambient kubeconfig.
"""

from __future__ import annotations

import base64
import json
from typing import TYPE_CHECKING

import pulumi
import pulumi_kubernetes as k8s

# CRD specs (ClusterIssuer, Certificate) use CustomResource -- not ConfigGroup,
# which can't resolve CRD schemas before cert-manager is installed. The specs
# are untyped dicts; always read upstream docs before modifying.
from pulumi_kubernetes.apiextensions import CustomResource

from config import (
    APP_PROD_DOMAINS,
    HANKO_API_URL_PROD,
    MARKETING_PREVIEW_DOMAINS,
    MARKETING_PROD_DOMAINS,
    PREVIEW_DOMAINS,
    PRODUCTION_DOMAINS,
)

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
WEB_NAME = "web"
WEB_PORT = 80
WEB_IMAGE_TAG = "latest"
PLATFORM_NAME = "platform"
PLATFORM_PORT = 3000
PLATFORM_IMAGE_TAG = "latest"

# Traefik Middleware reference for the platform Ingress. The `@kubernetescrd`
# suffix tells Traefik to resolve the name against the Kubernetes CRD store.
PLATFORM_STRIP_API_MIDDLEWARE = "default-strip-api-prefix@kubernetescrd"

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
    registry_pull_key: pulumi.Input[str],
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

    # -- TLS Certificate ------------------------------------------------------
    # SANs cover all four apex domains: marketing + app, prod + preview.
    # Path-based routing within each apex removes the need for per-PR or
    # per-service subdomains, so the SAN list is exactly the apex set.
    # See docs/plans/2026-04-11-app-server-prd.md "URL architecture".
    #
    # The secret name remains `preview-wildcard-tls` for continuity with
    # existing Ingress references, despite the cert being neither a wildcard
    # nor preview-only.
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
            ],
        },
        opts=pulumi.ResourceOptions(
            provider=provider,
            depends_on=[prod_cluster_issuer, staging_cluster_issuer],
        ),
    )

    # -- Scaleway Container Registry imagePullSecret --------------------------
    # Auth: username is always "nologin", password is a pull-scoped SCW API
    # key owned end-to-end by Pulumi (see cloud.py::registry_pull_api_key).
    # Rotation = `pulumi up`, no operator console toil.
    #
    # Pulumi's k8s provider treats changes to `Secret.data` as replace-
    # triggering (not in-place update), empirically confirmed on this
    # resource. Rotating the credential therefore replaces this single Secret
    # -- a ~1-second window where `scaleway-registry` doesn't exist and newly
    # scheduled pods hit ImagePullBackOff and retry. Already-running pods are
    # unaffected. The replace is confined to this one resource: the k8s
    # Provider is NOT being replaced, so nothing parented to it cascades.
    # See: https://www.scaleway.com/en/docs/container-registry/how-to/connect-docker-cli/
    docker_config = pulumi.Output.all(
        endpoint=registry.endpoint,
        password=registry_pull_key,
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
        opts=pulumi.ResourceOptions(
            provider=provider,
            depends_on=[image_pull_secret],
            ignore_changes=["spec.template.spec.containers[0].image"],
        ),
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

    # Site (Astro) serves the marketing apexes only. The app apexes
    # (app.familiar.systems, app.preview.familiar.systems) belong to the
    # SPA + platform + campaign ingresses on the other side of the split.
    all_site_hosts = [*MARKETING_PROD_DOMAINS, *MARKETING_PREVIEW_DOMAINS]
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

    # -- Platform PersistentVolume + PersistentVolumeClaim + Deployment + Service + Ingress --
    _platform_pv = k8s.core.v1.PersistentVolume(
        "platform-pv",
        metadata=k8s.meta.v1.ObjectMetaArgs(
            name="platform-pv",
            labels={"app": PLATFORM_NAME},
        ),
        spec=k8s.core.v1.PersistentVolumeSpecArgs(
            capacity={"storage": "1Gi"},
            access_modes=["ReadWriteOnce"],
            persistent_volume_reclaim_policy="Retain",
            storage_class_name="",
            host_path=k8s.core.v1.HostPathVolumeSourceArgs(
                path="/data/platform",
                type="DirectoryOrCreate",
            ),
            claim_ref=k8s.core.v1.ObjectReferenceArgs(
                namespace="default",
                name="platform-pvc",
            ),
        ),
        opts=pulumi.ResourceOptions(provider=provider, depends_on=[_wildcard_cert]),
    )

    _platform_pvc = k8s.core.v1.PersistentVolumeClaim(
        "platform-pvc",
        metadata=k8s.meta.v1.ObjectMetaArgs(
            name="platform-pvc",
            namespace="default",
        ),
        spec=k8s.core.v1.PersistentVolumeClaimSpecArgs(
            access_modes=["ReadWriteOnce"],
            storage_class_name="",
            volume_name="platform-pv",
            resources=k8s.core.v1.VolumeResourceRequirementsArgs(
                requests={"storage": "1Gi"},
            ),
        ),
        opts=pulumi.ResourceOptions(provider=provider, depends_on=[_platform_pv]),
    )

    platform_labels = {"app": PLATFORM_NAME}
    platform_image = registry.endpoint.apply(
        lambda ep: f"{ep}/{PLATFORM_NAME}:{PLATFORM_IMAGE_TAG}"
    )

    _platform_deployment = k8s.apps.v1.Deployment(
        "platform-deployment",
        metadata=k8s.meta.v1.ObjectMetaArgs(
            name=PLATFORM_NAME,
            namespace="default",
        ),
        spec=k8s.apps.v1.DeploymentSpecArgs(
            replicas=1,
            selector=k8s.meta.v1.LabelSelectorArgs(match_labels=platform_labels),
            template=k8s.core.v1.PodTemplateSpecArgs(
                metadata=k8s.meta.v1.ObjectMetaArgs(labels=platform_labels),
                spec=k8s.core.v1.PodSpecArgs(
                    image_pull_secrets=[
                        k8s.core.v1.LocalObjectReferenceArgs(name=REGISTRY_PULL_SECRET),
                    ],
                    # The platform container runs as the distroless `nonroot`
                    # user (UID 65532). The HostPath PV is auto-created by the
                    # kubelet as root:root, so SQLite's `mode=rwc` open of
                    # platform.db fails with SQLITE_CANTOPEN. fsGroup is
                    # unreliable for the in-tree HostPath driver; an init
                    # container running as root that chowns the mount is the
                    # robust fix and survives node turnover.
                    init_containers=[
                        k8s.core.v1.ContainerArgs(
                            name="chown-data",
                            image="busybox:1.36",
                            command=["sh", "-c", "chown -R 65532:65532 /data/platform"],
                            security_context=k8s.core.v1.SecurityContextArgs(
                                run_as_user=0,
                            ),
                            volume_mounts=[
                                k8s.core.v1.VolumeMountArgs(
                                    name="platform-data",
                                    mount_path="/data/platform",
                                ),
                            ],
                        ),
                    ],
                    containers=[
                        k8s.core.v1.ContainerArgs(
                            name=PLATFORM_NAME,
                            image=platform_image,
                            image_pull_policy="IfNotPresent",
                            ports=[k8s.core.v1.ContainerPortArgs(container_port=PLATFORM_PORT)],
                            env=[
                                k8s.core.v1.EnvVarArgs(
                                    name="HANKO_API_URL",
                                    value=HANKO_API_URL_PROD,
                                ),
                                k8s.core.v1.EnvVarArgs(
                                    name="DATABASE_URL",
                                    value="sqlite:///data/platform/platform.db?mode=rwc",
                                ),
                                k8s.core.v1.EnvVarArgs(
                                    name="CORS_ORIGINS",
                                    # Browser traffic is same-origin within
                                    # the app apex, so CORS preflights do
                                    # not fire; this entry is for non-
                                    # browser consumers that send an Origin
                                    # header.
                                    value="https://app.familiar.systems",
                                ),
                                k8s.core.v1.EnvVarArgs(name="PORT", value=str(PLATFORM_PORT)),
                                k8s.core.v1.EnvVarArgs(name="RUST_LOG", value="info"),
                            ],
                            volume_mounts=[
                                k8s.core.v1.VolumeMountArgs(
                                    name="platform-data",
                                    mount_path="/data/platform",
                                ),
                            ],
                            resources=k8s.core.v1.ResourceRequirementsArgs(
                                requests={"cpu": "10m", "memory": "32Mi"},
                                limits={"memory": "64Mi"},
                            ),
                        ),
                    ],
                    volumes=[
                        k8s.core.v1.VolumeArgs(
                            name="platform-data",
                            persistent_volume_claim=k8s.core.v1.PersistentVolumeClaimVolumeSourceArgs(
                                claim_name="platform-pvc",
                            ),
                        ),
                    ],
                ),
            ),
        ),
        opts=pulumi.ResourceOptions(
            provider=provider,
            depends_on=[image_pull_secret, _platform_pvc],
            ignore_changes=["spec.template.spec.containers[0].image"],
        ),
    )

    _platform_service = k8s.core.v1.Service(
        "platform-service",
        metadata=k8s.meta.v1.ObjectMetaArgs(
            name="platform-service",
            namespace="default",
        ),
        spec=k8s.core.v1.ServiceSpecArgs(
            selector=platform_labels,
            ports=[
                k8s.core.v1.ServicePortArgs(
                    port=PLATFORM_PORT,
                    target_port=PLATFORM_PORT,
                ),
            ],
        ),
        opts=k8s_opts,
    )

    # Strip `/api` from request paths before they hit the platform backend.
    # Traefik Middleware is a CRD; it lives alongside the Ingress that
    # references it via the router.middlewares annotation. The annotation
    # value is "<namespace>-<name>@kubernetescrd".
    _platform_strip_prefix = CustomResource(
        "platform-strip-api-prefix",
        api_version="traefik.io/v1alpha1",
        kind="Middleware",
        metadata={"name": "strip-api-prefix", "namespace": "default"},
        spec={"stripPrefix": {"prefixes": ["/api"]}},
        opts=k8s_opts,
    )

    # Platform reachable at <app-apex>/api/* (path-based within the app
    # apex, not a per-service subdomain). The SPA, platform, and campaign
    # shards all share the app apex so browser calls are same-origin.
    # Longer PathPrefix wins in Traefik's default router-priority-by-rule-
    # length model, so /api/* lands here while /campaign/* and the SPA
    # catch-all bind separately.
    _platform_ingress = k8s.networking.v1.Ingress(
        "platform-ingress",
        metadata=k8s.meta.v1.ObjectMetaArgs(
            name=PLATFORM_NAME,
            namespace="default",
            annotations={
                "traefik.ingress.kubernetes.io/router.entrypoints": "websecure",
                "traefik.ingress.kubernetes.io/router.middlewares": PLATFORM_STRIP_API_MIDDLEWARE,
            },
        ),
        spec=k8s.networking.v1.IngressSpecArgs(
            tls=[
                k8s.networking.v1.IngressTLSArgs(
                    hosts=list(APP_PROD_DOMAINS),
                    secret_name=WILDCARD_CERT_SECRET,
                ),
            ],
            rules=[
                k8s.networking.v1.IngressRuleArgs(
                    host=host,
                    http=k8s.networking.v1.HTTPIngressRuleValueArgs(
                        paths=[
                            k8s.networking.v1.HTTPIngressPathArgs(
                                path="/api",
                                path_type="Prefix",
                                backend=k8s.networking.v1.IngressBackendArgs(
                                    service=k8s.networking.v1.IngressServiceBackendArgs(
                                        name="platform-service",
                                        port=k8s.networking.v1.ServiceBackendPortArgs(
                                            number=PLATFORM_PORT,
                                        ),
                                    ),
                                ),
                            ),
                        ],
                    ),
                )
                for host in APP_PROD_DOMAINS
            ],
        ),
        opts=pulumi.ResourceOptions(provider=provider, depends_on=[_platform_strip_prefix]),
    )

    # -- Web (SPA) Deployment + Service + Ingress ----------------------------
    # Static nginx image serving the built Vite SPA bundle at the catch-all
    # `/` on the app apex. Longer PathPrefix wins in Traefik's default
    # router-priority-by-rule-length, so the platform's `/api` rule above
    # continues to win for `/api/*`; everything else lands here.
    web_labels = {"app": WEB_NAME}
    web_image = registry.endpoint.apply(lambda ep: f"{ep}/{WEB_NAME}:{WEB_IMAGE_TAG}")

    _web_deployment = k8s.apps.v1.Deployment(
        "web-deployment",
        metadata=k8s.meta.v1.ObjectMetaArgs(
            name=WEB_NAME,
            namespace="default",
        ),
        spec=k8s.apps.v1.DeploymentSpecArgs(
            replicas=1,
            selector=k8s.meta.v1.LabelSelectorArgs(match_labels=web_labels),
            template=k8s.core.v1.PodTemplateSpecArgs(
                metadata=k8s.meta.v1.ObjectMetaArgs(labels=web_labels),
                spec=k8s.core.v1.PodSpecArgs(
                    image_pull_secrets=[
                        k8s.core.v1.LocalObjectReferenceArgs(name=REGISTRY_PULL_SECRET),
                    ],
                    containers=[
                        k8s.core.v1.ContainerArgs(
                            name=WEB_NAME,
                            image=web_image,
                            image_pull_policy="IfNotPresent",
                            ports=[k8s.core.v1.ContainerPortArgs(container_port=WEB_PORT)],
                            resources=k8s.core.v1.ResourceRequirementsArgs(
                                requests={"cpu": "10m", "memory": "32Mi"},
                                limits={"memory": "64Mi"},
                            ),
                        ),
                    ],
                ),
            ),
        ),
        opts=pulumi.ResourceOptions(
            provider=provider,
            depends_on=[image_pull_secret],
            ignore_changes=["spec.template.spec.containers[0].image"],
        ),
    )

    _web_service = k8s.core.v1.Service(
        "web-service",
        metadata=k8s.meta.v1.ObjectMetaArgs(
            name=WEB_NAME,
            namespace="default",
        ),
        spec=k8s.core.v1.ServiceSpecArgs(
            selector=web_labels,
            ports=[
                k8s.core.v1.ServicePortArgs(
                    port=WEB_PORT,
                    target_port=WEB_PORT,
                ),
            ],
        ),
        opts=k8s_opts,
    )

    _web_ingress = k8s.networking.v1.Ingress(
        "web-ingress",
        metadata=k8s.meta.v1.ObjectMetaArgs(
            name=WEB_NAME,
            namespace="default",
            annotations={
                "traefik.ingress.kubernetes.io/router.entrypoints": "websecure",
            },
        ),
        spec=k8s.networking.v1.IngressSpecArgs(
            tls=[
                k8s.networking.v1.IngressTLSArgs(
                    hosts=list(APP_PROD_DOMAINS),
                    secret_name=WILDCARD_CERT_SECRET,
                ),
            ],
            rules=[
                k8s.networking.v1.IngressRuleArgs(
                    host=host,
                    http=k8s.networking.v1.HTTPIngressRuleValueArgs(
                        paths=[
                            k8s.networking.v1.HTTPIngressPathArgs(
                                path="/",
                                path_type="Prefix",
                                backend=k8s.networking.v1.IngressBackendArgs(
                                    service=k8s.networking.v1.IngressServiceBackendArgs(
                                        name=WEB_NAME,
                                        port=k8s.networking.v1.ServiceBackendPortArgs(
                                            number=WEB_PORT,
                                        ),
                                    ),
                                ),
                            ),
                        ],
                    ),
                )
                for host in APP_PROD_DOMAINS
            ],
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
