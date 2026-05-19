#!/usr/bin/env bash
set -euo pipefail

# Builds and applies the preview Kustomize overlay for a PR environment.
# Called by ci_cd_preview.yml's deploy step.
#
# Required environment variables (set by the calling workflow):
#   NAMESPACE              preview-pr-${PR_NUMBER}
#   PR_NUMBER              the PR number
#   SITE_IMAGE             full image ref for site
#   WEB_IMAGE              full image ref for web
#   PLATFORM_IMAGE         full image ref for platform
#   CAMPAIGN_IMAGE         full image ref for campaign
#   DOCKERCONFIG_B64       base64-encoded docker config JSON
#   INTERNAL_BEARER_PRIMARY_B64  base64-encoded bearer token
#   INTERNAL_BEARER_CHECKSUM     sha256 of the bearer value
#   HANKO_API_URL_DEV      Hanko tenant URL for preview

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
K8S_DIR="${SCRIPT_DIR}/.."

# Validate required env vars
for var in NAMESPACE PR_NUMBER SITE_IMAGE WEB_IMAGE PLATFORM_IMAGE CAMPAIGN_IMAGE \
           DOCKERCONFIG_B64 INTERNAL_BEARER_PRIMARY_B64 INTERNAL_BEARER_CHECKSUM \
           HANKO_API_URL_DEV; do
  if [ -z "${!var:-}" ]; then
    echo "ERROR: ${var} is not set" >&2
    exit 1
  fi
done

# Copy the entire k8s directory to a temp dir so envsubst can modify
# the preview overlay without touching the repo. The base directory
# is included so relative paths in kustomization.yaml resolve correctly.
WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT
cp -r "${K8S_DIR}/base" "${K8S_DIR}/overlays" "$WORK_DIR/"

# envsubst only the preview overlay files (base is static, prod is untouched)
PREVIEW_DIR="${WORK_DIR}/overlays/preview"
export NAMESPACE PR_NUMBER SITE_IMAGE WEB_IMAGE PLATFORM_IMAGE CAMPAIGN_IMAGE \
       DOCKERCONFIG_B64 INTERNAL_BEARER_PRIMARY_B64 INTERNAL_BEARER_CHECKSUM \
       HANKO_API_URL_DEV

find "$PREVIEW_DIR" -name '*.yaml' -o -name '*.yml' | while read -r f; do
  envsubst < "$f" > "${f}.tmp"
  mv "${f}.tmp" "$f"
done
# Also process kustomization.yaml (contains ${NAMESPACE})
envsubst < "${PREVIEW_DIR}/kustomization.yaml" > "${PREVIEW_DIR}/kustomization.yaml.tmp"
mv "${PREVIEW_DIR}/kustomization.yaml.tmp" "${PREVIEW_DIR}/kustomization.yaml"

echo "==> Building preview overlay..."
kubectl kustomize "$PREVIEW_DIR" | kubectl apply --server-side --force-conflicts -f -

echo "==> Preview applied to namespace ${NAMESPACE}"
