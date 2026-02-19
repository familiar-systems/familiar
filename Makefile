.PHONY: install dev build typecheck lint format test check clean

install: ## Install all dependencies
	pnpm install

dev: ## Start all dev servers (web:5173, api:3001, collab:3002)
	pnpm turbo dev

build: ## Build all apps (web → Vite, api/collab/worker → tsup)
	pnpm turbo build

typecheck: ## Type-check all packages and apps
	pnpm turbo typecheck

lint: ## Lint all packages with oxlint
	pnpm turbo lint

format: ## Format all files with oxfmt
	pnpm oxfmt .

test: ## Run all tests with vitest
	pnpm turbo test

check: typecheck lint ## Type-check + lint (CI gate)

clean: ## Remove build artifacts and caches
	rm -rf node_modules/.cache
	find . -name 'dist' -type d -not -path '*/node_modules/*' -exec rm -rf {} +
	find . -name '.turbo' -type d -exec rm -rf {} +
	find . -name '*.tsbuildinfo' -delete

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-12s\033[0m %s\n", $$1, $$2}'

.DEFAULT_GOAL := help
