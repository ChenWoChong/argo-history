APP_NAME ?= argo-history
NAMESPACE ?= argo-history
HELM_RELEASE ?= argo-history
CHART_DIR ?= chart
VALUES_FILE ?= chart/values.yaml
NODEPORT ?= 32080
LINUX_PLATFORM ?= linux/amd64
RUST_IMAGE ?= rust:1.94-bookworm
IMG ?= truth-ai-registry.cn-hangzhou.cr.aliyuncs.com/test/argo-history:dev

SHELL := /usr/bin/env bash
.SHELLFLAGS := -eo pipefail -c
.DEFAULT_GOAL := help

.PHONY: help
help: ## Show this help.
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<TARGET>\033[0m\n"} \
	/^[a-zA-Z0-9_.-]+:.*##/ { printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2 } \
	/^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) }' $(MAKEFILE_LIST)

##@ Development

.PHONY: fmt
fmt: ## Format Rust code.
	cargo fmt --all

.PHONY: test
test: ## Run unit tests.
	cargo test

.PHONY: check
check: ## Run cargo check.
	cargo check

.PHONY: clippy
clippy: ## Run clippy lints.
	cargo clippy --all-targets --all-features -- -D warnings

.PHONY: verify
verify: fmt test check ## Run common local verification steps.

.PHONY: run
run: ## Run the server locally using config/config.yaml.
	cargo run -- --config config/config.yaml

##@ Build

.PHONY: build
build: ## Build a local release binary for the current OS.
	cargo build --release

.PHONY: build-linux
build-linux: ## Build a Linux release binary into target-linux/.
	@echo "Building for platform: $(LINUX_PLATFORM)"
	docker run --rm --platform $(LINUX_PLATFORM) \
		-v "$(CURDIR):/workspace" \
		-v "$(HOME)/.cargo/registry:/usr/local/cargo/registry" \
		-v "$(HOME)/.cargo/git:/usr/local/cargo/git" \
		-w /workspace \
		$(RUST_IMAGE) \
		cargo build --release --target-dir target-linux

.PHONY: docker-build
docker-build: build-linux ## Build the runtime image tagged as $(IMG).
	@echo "Building Docker image $(IMG) for platform: $(LINUX_PLATFORM)"
	docker build --platform $(LINUX_PLATFORM) -t $(IMG) .

.PHONY: docker-push
docker-push: docker-build ## Push the current image to remote registry. Usage: make docker-push IMG=your-registry/image:tag
	@test -n "$(IMG)" || (echo "IMG is required, e.g. make docker-push IMG=your-registry/image:tag" && exit 1)
	docker push $(IMG)

.PHONY: docker-tag
docker-tag: ## Retag the current image. Usage: make docker-tag IMG=your-registry/image:tag
	@test -n "$(IMG)" || (echo "IMG is required, e.g. make docker-tag IMG=your-registry/image:tag" && exit 1)
	docker tag $(IMG) $(TAG)

##@ Chart

.PHONY: chart-lint
chart-lint: ## Run helm lint on the chart.
	helm lint $(CHART_DIR) -f $(VALUES_FILE)

.PHONY: helm-template
helm-template: ## Render the chart locally.
	helm template $(HELM_RELEASE) $(CHART_DIR) -n $(NAMESPACE) --create-namespace -f $(VALUES_FILE)

.PHONY: helm-package
helm-package: ## Package the chart into dist/.
	mkdir -p dist
	helm package $(CHART_DIR) -d dist

##@ Deploy

.PHONY: helm-install
helm-install: ## Install or upgrade the chart in $(NAMESPACE).
	helm upgrade -i $(HELM_RELEASE) $(CHART_DIR) -n $(NAMESPACE) --create-namespace -f $(VALUES_FILE)

.PHONY: helm-install-img
helm-install-img: ## Install or upgrade using IMG=<repo:tag>.
	@img='$(IMG)'; \
	helm upgrade -i $(HELM_RELEASE) $(CHART_DIR) -n $(NAMESPACE) --create-namespace -f $(VALUES_FILE) \
		--set image.repository="$${img%:*}" \
		--set image.tag="$${img##*:}"

.PHONY: helm-uninstall
helm-uninstall: ## Uninstall the release from $(NAMESPACE).
	helm uninstall $(HELM_RELEASE) -n $(NAMESPACE)

.PHONY: deploy-local
deploy-local: docker-build helm-install-img ## Build image and deploy it to Orb k8s.

.PHONY: deploy-remote
deploy-remote: ## Deploy remote image. Usage: make deploy-remote IMG=your-registry/image:tag
	@test -n "$(IMG)" || (echo "IMG is required, e.g. make deploy-remote IMG=your-registry/image:tag" && exit 1)
	@img='$(IMG)'; \
	helm upgrade -i $(HELM_RELEASE) $(CHART_DIR) -n $(NAMESPACE) --create-namespace -f $(VALUES_FILE) \
		--set image.repository="$${img%:*}" \
		--set image.tag="$${img##*:}"

.PHONY: restart
restart: ## Restart the deployment.
	kubectl rollout restart deployment/$(HELM_RELEASE) -n $(NAMESPACE)

.PHONY: rollout-status
rollout-status: ## Wait for deployment rollout to complete.
	kubectl rollout status deployment/$(HELM_RELEASE) -n $(NAMESPACE) --timeout=180s

.PHONY: redeploy
redeploy: docker-build helm-install-img restart rollout-status ## Rebuild image, upgrade chart and restart pods.

##@ Operations

.PHONY: status
status: ## Show high-level deployment status.
	kubectl get pods,svc,deploy,pvc -n $(NAMESPACE)

.PHONY: pods
pods: ## List pods in the namespace.
	kubectl get pods -n $(NAMESPACE) -o wide

.PHONY: logs
logs: ## Tail deployment logs.
	kubectl logs deployment/$(HELM_RELEASE) -n $(NAMESPACE) --tail=200 -f

.PHONY: describe
describe: ## Describe the deployment.
	kubectl describe deployment/$(HELM_RELEASE) -n $(NAMESPACE)

.PHONY: ui-url
ui-url: ## Print the local UI URL for the NodePort service.
	@echo "http://127.0.0.1:$(NODEPORT)"

.PHONY: api-apps
api-apps: ## Query the apps API via NodePort.
	curl -fsSL http://127.0.0.1:$(NODEPORT)/api/v1/apps | jq .

.PHONY: api-appsets
api-appsets: ## Query the appsets API via NodePort.
	curl -fsSL http://127.0.0.1:$(NODEPORT)/api/v1/appsets | jq .

.PHONY: smoke-test
smoke-test: ## Run the end-to-end smoke test against the current cluster.
	bash hack/smoke-test.sh

##@ Cleanup

.PHONY: clean
clean: ## Remove cargo build outputs for the host OS.
	cargo clean

.PHONY: clean-linux
clean-linux: ## Remove Linux cross-build outputs.
	rm -rf target-linux

.PHONY: clean-dist
clean-dist: ## Remove packaged chart artifacts.
	rm -rf dist

.PHONY: clean-all
clean-all: clean clean-linux clean-dist ## Remove all generated artifacts.