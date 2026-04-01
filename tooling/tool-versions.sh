#!/usr/bin/env bash
# Extract tool versions from mise.toml for CI.
# Prints key=value lines. Pipe to $GITHUB_OUTPUT in Actions.
#
# Usage:
#   ./tooling/tool-versions.sh              # prints to stdout
#   ./tooling/tool-versions.sh >> "$GITHUB_OUTPUT"  # in CI
set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
MISE_FILE="${REPO_ROOT}/mise.toml"

if [[ ! -f "$MISE_FILE" ]]; then
  echo "ERROR: mise.toml not found at ${MISE_FILE}" >&2
  exit 1
fi

get_version() {
  local tool="$1"
  # Handles both: node = "24" and "npm:pnpm" = "10.32"
  grep -E "^\"?${tool}\"?\s*=" "$MISE_FILE" | sed 's/.*=\s*"\([^"]*\)"/\1/' | head -1
}

echo "node=$(get_version node)"
echo "pnpm=$(get_version 'npm:pnpm')"
echo "rust=$(get_version rust)"
echo "python=$(get_version python)"
