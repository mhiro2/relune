.PHONY: help setup-tools fmt fmt-check lint test build license-check

.DEFAULT_GOAL := help

WASM_TARGET := wasm32-unknown-unknown

help:
	@echo 'Usage: make <target>'
	@echo ''
	@echo '  setup-tools    Install dev dependencies.'
	@echo '  fmt            Format all code.'
	@echo '  fmt-check      Check formatting without modifying files.'
	@echo '  lint           Lint all code.'
	@echo '  test           Run all tests with coverage and wasm-bindgen tests.'
	@echo '  build          Build everything.'
	@echo '  license-check  Validate dependency licenses.'

setup-tools:
	rustup component add llvm-tools-preview
	rustup target add $(WASM_TARGET)
	cargo install cargo-llvm-cov cargo-deny --locked
	cargo install wasm-pack --locked

fmt:
	cargo fmt --all
	cd crates/relune-render-html && pnpm fmt

fmt-check:
	cargo fmt --all -- --check
	cd crates/relune-render-html && pnpm fmt:check

lint:
	cargo clippy --workspace --all-targets -- -D warnings
	cargo clippy -p relune-wasm --target $(WASM_TARGET) -- -D warnings
	cd crates/relune-render-html && pnpm lint && pnpm typecheck

test:
	cargo llvm-cov test --workspace --lcov --output-path lcov.info
	cd crates/relune-wasm && wasm-pack test --node

build:
	cd crates/relune-render-html && pnpm build
	cargo build
	cargo build -p relune-wasm --target $(WASM_TARGET)

license-check:
	cargo deny check licenses
