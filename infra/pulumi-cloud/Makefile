KUBECONFIG_FILE := $(HOME)/.kube/k3s-loreweaver.yaml
CONTEXT_NAME := loreweaver-preview
K3S_FLOATING_IP := $(shell pulumi stack output k3s_floating_ip 2>/dev/null)
SSH_KEY := $(HOME)/.ssh/loreweaver-deploy
SECRET_REGION := fr-par
KUBECONFIG_SECRET_NAME := k3s-kubeconfig

.PHONY: kubeconfig
kubeconfig: ## Export k3s kubeconfig from Scaleway SM and merge into ~/.kube/config
	@set -euo pipefail; \
	mkdir -p $(HOME)/.kube; \
	SECRET_ID=$$(scw secret secret list region=$(SECRET_REGION) -o json \
		| jq -er --arg n "$(KUBECONFIG_SECRET_NAME)" '.[] | select(.name==$$n) | .id'); \
	test -n "$$SECRET_ID" || { echo "Could not resolve SM secret '$(KUBECONFIG_SECRET_NAME)' in $(SECRET_REGION). Is your scw config / .envrc loaded?" >&2; exit 1; }; \
	scw secret version access "$$SECRET_ID" revision=latest region=$(SECRET_REGION) -o json \
		| jq -er '.data' | base64 -d > $(KUBECONFIG_FILE); \
	test -s $(KUBECONFIG_FILE) || { echo "Fetched kubeconfig is empty" >&2; exit 1; }; \
	kubectl --kubeconfig $(KUBECONFIG_FILE) config rename-context default $(CONTEXT_NAME) 2>/dev/null || true; \
	kubectl config delete-context $(CONTEXT_NAME) 2>/dev/null || true; \
	kubectl config delete-cluster k3s-loreweaver 2>/dev/null || true; \
	kubectl config delete-user pulumi-admin 2>/dev/null || true; \
	KUBECONFIG=$(HOME)/.kube/config:$(KUBECONFIG_FILE) kubectl config view --flatten > $(HOME)/.kube/config.merged; \
	mv $(HOME)/.kube/config.merged $(HOME)/.kube/config; \
	echo "Context '$(CONTEXT_NAME)' merged into ~/.kube/config"; \
	echo "Switch with: kubectl config use-context $(CONTEXT_NAME)"

.PHONY: preview
preview: ## Open k9s on the preview cluster
	k9s --context $(CONTEXT_NAME)

.PHONY: logs
logs: ## Open Kubetail log viewer in browser
	kubetail serve --context $(CONTEXT_NAME)

.PHONY: up
up: ## pulumi up
	pulumi up

.PHONY: lint
lint: ## Lint, format, and typecheck
	uv run ruff check --fix . && uv run ruff format . && uv run basedpyright

.PHONY: help
help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-12s\033[0m %s\n", $$1, $$2}'

.DEFAULT_GOAL := help
