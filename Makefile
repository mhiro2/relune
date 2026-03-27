.PHONY: help setup-tools fmt fmt-check lint test test-rust test-update test-coverage test-wasm test-ci build license-check

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

build: ## Build frontend, Rust, and wasm artifacts.
	cd crates/relune-render-html && pnpm build
	cargo build
	cargo build -p relune-wasm --target $(WASM_TARGET)

license-check: ## Validate dependency licenses.
	cargo deny check licenses
