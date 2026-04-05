APP_NAME ?= argo-history
NAMESPACE ?= argo-history
IMG ?= argo-history:dev
HELM_RELEASE ?= argo-history
CHART_DIR ?= chart
ORB_VALUES ?= chart/values-orb-dev.yaml
NODEPORT ?= 32080

.PHONY: fmt
fmt:
	cargo fmt --all

.PHONY: test
test:
	cargo test

.PHONY: check
check:
	cargo check

.PHONY: build
build:
	cargo build --release

.PHONY: build-linux
build-linux:
	docker run --rm --platform linux/arm64 \
		-v "$(CURDIR):/workspace" \
		-v "$(HOME)/.cargo/registry:/usr/local/cargo/registry" \
		-v "$(HOME)/.cargo/git:/usr/local/cargo/git" \
		-w /workspace \
		rust:1.94-bookworm \
		cargo build --release --target-dir target-linux

.PHONY: docker-build
docker-build: build-linux
	docker build -t $(IMG) .

.PHONY: helm-template
helm-template:
	helm template $(HELM_RELEASE) $(CHART_DIR) -f $(ORB_VALUES)

.PHONY: helm-package
helm-package:
	mkdir -p dist
	helm package $(CHART_DIR) -d dist

.PHONY: helm-install
helm-install:
	helm upgrade -i $(HELM_RELEASE) $(CHART_DIR) -n $(NAMESPACE) --create-namespace -f $(ORB_VALUES)

.PHONY: helm-uninstall
helm-uninstall:
	helm uninstall $(HELM_RELEASE) -n $(NAMESPACE)

.PHONY: deploy-local
deploy-local: docker-build helm-install

.PHONY: smoke-test
smoke-test:
	bash hack/smoke-test.sh
