KUBECONFIG_FILE := $(HOME)/.kube/k3s-loreweaver.yaml
CONTEXT_NAME := loreweaver-preview
K3S_FLOATING_IP := $(shell pulumi stack output k3s_floating_ip 2>/dev/null)
K3S_KUBECONFIG_SECRET_ID := $(shell pulumi stack output k3s_kubeconfig_secret_id 2>/dev/null | cut -d/ -f2)
SSH_KEY := $(HOME)/.ssh/loreweaver-deploy

.PHONY: kubeconfig
kubeconfig: ## Export k3s kubeconfig and merge into ~/.kube/config
	@mkdir -p $(HOME)/.kube
	pulumi stack output k3s_kubeconfig --show-secrets > $(KUBECONFIG_FILE)
	kubectl --kubeconfig $(KUBECONFIG_FILE) config rename-context default $(CONTEXT_NAME) 2>/dev/null || true
	KUBECONFIG=$(HOME)/.kube/config:$(KUBECONFIG_FILE) kubectl config view --flatten > $(HOME)/.kube/config.merged
	mv $(HOME)/.kube/config.merged $(HOME)/.kube/config
	@echo "Context '$(CONTEXT_NAME)' merged into ~/.kube/config"
	@echo "Switch with: kubectl config use-context $(CONTEXT_NAME)"

.PHONY: preview
preview: ## Open k9s on the preview cluster
	k9s --context $(CONTEXT_NAME)

.PHONY: up
up: ## pulumi up
	pulumi up

.PHONY: lint
lint: ## Lint, format, and typecheck
	uv run ruff check --fix . && uv run ruff format . && uv run basedpyright

.PHONY: rotate-certs
rotate-certs: ## Rotate k3s client certs, re-extract kubeconfig, push to Scaleway SM
	ssh root@$(K3S_FLOATING_IP) -i $(SSH_KEY) \
		"systemctl stop k3s && k3s certificate rotate --service admin && systemctl start k3s"
	@echo "Waiting for k3s to restart..."
	@sleep 5
	ssh root@$(K3S_FLOATING_IP) -i $(SSH_KEY) \
		"cat /etc/rancher/k3s/k3s.yaml" \
		| sed 's/127\.0\.0\.1/$(K3S_FLOATING_IP)/g' \
		> $(KUBECONFIG_FILE)
	scw secret version create $(K3S_KUBECONFIG_SECRET_ID) data=@$(KUBECONFIG_FILE)
	@echo "Kubeconfig rotated and pushed to Scaleway SM"
	$(MAKE) kubeconfig

.PHONY: help
help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-12s\033[0m %s\n", $$1, $$2}'

.DEFAULT_GOAL := help
