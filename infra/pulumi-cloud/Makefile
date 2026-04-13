KUBECONFIG_FILE := $(HOME)/.kube/k3s-loreweaver.yaml
CONTEXT_NAME := loreweaver-preview
K3S_FLOATING_IP := $(shell pulumi stack output k3s_floating_ip 2>/dev/null)
K3S_KUBECONFIG_SECRET_ID := $(shell pulumi stack output k3s_kubeconfig_secret_id 2>/dev/null | cut -d/ -f2)
SSH_KEY := $(HOME)/.ssh/loreweaver-deploy

.PHONY: kubeconfig
kubeconfig: ## Export k3s kubeconfig from Scaleway SM and merge into ~/.kube/config
	@mkdir -p $(HOME)/.kube
	scw secret version access $(K3S_KUBECONFIG_SECRET_ID) revision=latest region=fr-par -o json \
		| jq -r '.data' | base64 -d > $(KUBECONFIG_FILE)
	kubectl --kubeconfig $(KUBECONFIG_FILE) config rename-context default $(CONTEXT_NAME) 2>/dev/null || true
	KUBECONFIG=$(HOME)/.kube/config:$(KUBECONFIG_FILE) kubectl config view --flatten > $(HOME)/.kube/config.merged
	mv $(HOME)/.kube/config.merged $(HOME)/.kube/config
	@echo "Context '$(CONTEXT_NAME)' merged into ~/.kube/config"
	@echo "Switch with: kubectl config use-context $(CONTEXT_NAME)"

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
