.PHONY: help setup-tools fmt fmt-check lint check-generated-html-js test test-rust test-update test-coverage test-wasm test-ci build-frontend build license-check

.DEFAULT_GOAL := help

WASM_TARGET := wasm32-unknown-unknown

help: ## Show available make targets.
	@printf 'Usage: make <target>\n\n'
	@awk 'BEGIN {FS = ":.*## "}/^[a-zA-Z0-9_.-]+:.*## / {printf "  %-14s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

setup-tools: ## Install required toolchains and development tools.
	rustup component add llvm-tools-preview
	rustup target add $(WASM_TARGET)
	cargo install cargo-llvm-cov cargo-deny --locked
	cargo install wasm-pack --locked

fmt: ## Format all code.
	cargo fmt --all
	cd crates/relune-render-html && pnpm fmt

fmt-check: ## Check formatting without modifying files.
	cargo fmt --all -- --check
	cd crates/relune-render-html && pnpm fmt:check

lint: ## Run Rust linting plus frontend lint and type checks.
	cargo clippy --workspace --all-targets -- -D warnings
	cargo clippy -p relune-wasm --target $(WASM_TARGET) -- -D warnings
	cd crates/relune-render-html && pnpm lint && pnpm typecheck
	$(MAKE) check-generated-html-js

check-generated-html-js: ## Verify committed HTML viewer bundles match TypeScript sources.
	$(MAKE) build-frontend
	git diff --exit-code -- crates/relune-render-html/src/js
	test -z "$$(git ls-files --others --exclude-standard -- crates/relune-render-html/src/js)"

test: test-rust test-wasm ## Run local Rust and wasm tests.

test-rust: ## Run workspace Rust tests.
	cargo test --workspace

test-update: ## Update insta snapshots and rerun workspace Rust tests.
	INSTA_UPDATE=always cargo test --workspace

test-coverage: ## Run workspace Rust tests with coverage and write lcov.info.
	cargo llvm-cov test --workspace --lcov --output-path lcov.info

test-wasm: ## Run relune-wasm tests in Node.js.
	cd crates/relune-wasm && wasm-pack test --node

test-ci: test-coverage test-wasm ## Run CI coverage and wasm tests.

build-frontend: ## Rebuild committed HTML viewer bundles from TypeScript.
	cd crates/relune-render-html && pnpm build

build: ## Build Rust and wasm artifacts using committed HTML viewer bundles.
	cargo build
	cargo build -p relune-wasm --target $(WASM_TARGET)

license-check: ## Validate dependency licenses.
	cargo deny check licenses
