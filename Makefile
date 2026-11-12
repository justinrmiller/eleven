# eleven — developer tasks. Run `make help` for a summary.
CARGO ?= cargo
# Files that need real audio hardware or a desktop session; excluded from the coverage gate.
COV_IGNORE := (cpal_out|media_keys|main)\.rs$$
COV_MIN ?= 90

.PHONY: help all build run release test lint fmt clippy cov cov-html fixtures bundle-macos dist-macos dist-linux clean

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  %-14s %s\n", $$1, $$2}'

all: lint test ## Lint and test everything

build: ## Debug build of the whole workspace
	$(CARGO) build --workspace

run: ## Run the player (release build)
	$(CARGO) run -p eleven --release

release: ## Optimised build of the player
	$(CARGO) build -p eleven --release

test: ## Run all tests
	$(CARGO) test --workspace

fmt: ## Format all code
	$(CARGO) fmt --all

clippy: ## Lint with clippy, warnings are errors
	$(CARGO) clippy --workspace --all-targets -- -D warnings

lint: ## rustfmt check + clippy
	$(CARGO) fmt --all -- --check
	$(MAKE) clippy

cov: ## Line-coverage summary with a fail-under gate (needs cargo-llvm-cov)
	$(CARGO) llvm-cov --workspace --ignore-filename-regex '$(COV_IGNORE)' \
		--fail-under-lines $(COV_MIN) --summary-only

cov-html: ## HTML coverage report in target/llvm-cov/html
	$(CARGO) llvm-cov --workspace --ignore-filename-regex '$(COV_IGNORE)' --html

fixtures: ## Regenerate audio test fixtures (needs ffmpeg)
	./scripts/gen-fixtures.sh

bundle-macos: ## Build dist/eleven.app (macOS only)
	./scripts/bundle-macos.sh

dist-macos: ## Universal .app as .dmg and .zip in dist/ (macOS only)
	./scripts/package-macos.sh

dist-linux: ## .tar.gz and .deb in dist/ (Linux only)
	./scripts/package-linux.sh

clean: ## Remove build output
	$(CARGO) clean
	rm -rf dist
