"""Local k3d test for k8s resources.

Tests the k8s resource declarations from ../pulumi-cloud/k8s.py
against a local k3d cluster.

Setup:
  k3d cluster create loreweaver-test
  cd infra/test-k8s
  pulumi login --local
  pulumi stack init test
  pulumi config set --secret k3d-kubeconfig "$(k3d kubeconfig get loreweaver-test)"
  pulumi config set --secret bunny-api-key fake-test-key
  pulumi config set acme-email test@example.com
  export SCW_SECRET_KEY=noop  # dummy value for imagePullSecret
  pulumi up
"""

import k8s as loreweaver_k8s
import pulumi

config = pulumi.Config()
kubeconfig = config.require_secret("k3d-kubeconfig")

loreweaver_k8s.create_k8s_resources(
    kubeconfig=kubeconfig,
    registry_endpoint=pulumi.Output.from_input("rg.fr-par.scw.cloud/loreweaver"),
    bunny_api_key=config.require_secret("bunny-api-key"),
    acme_email=config.require("acme-email"),
)
