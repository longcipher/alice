# Default recipe to display help
default:
  @just --list

# Format all code
format:
  rumdl fmt .
  taplo fmt
  cargo +nightly fmt --all

# Auto-fix linting issues
fix:
  rumdl check --fix .
  cargo +nightly clippy --fix --all --allow-dirty

# Run all lints
lint:
  typos
  rumdl check .
  taplo fmt --check
  cargo +nightly fmt --all -- --check
  cargo +nightly clippy --all -- -D warnings
  cargo machete

# Run tests with cargo-nextest
test:
  cargo nextest run --all-features

# Run tests with coverage
test-coverage:
  cargo tarpaulin --all-features --workspace --timeout 300

# Build entire workspace
build:
  cargo build --workspace

# Check all targets compile
check:
  cargo check --all-targets --all-features

# Publish all crates to crates.io (dry run)
publish-check:
  cargo publish --workspace --dry-run --allow-dirty

# Publish all crates to crates.io
publish:
  cargo publish --workspace

# Scans the Rust sources for Chinese characters.
#
# `alice-core/src/memory/service.rs` carries intentional CJK marker data.

# Fail if Chinese characters appear in the Rust sources
check-cn:
  ! rg --line-number --column "\p{Han}" --glob '!crates/alice-core/src/memory/service.rs' bin crates

# Full CI check
ci: lint test build

# ============================================================
# Maintenance & Tools
# ============================================================

# Clean build artifacts
clean:
  cargo clean

# Install all required development tools
setup:
  rustup toolchain install nightly
  cargo install cargo-machete
  cargo install cargo-nextest
  cargo install taplo-cli
  cargo install typos-cli
  cargo install rumdl
  cargo install cargo-tarpaulin
  cargo install cargo-audit

# Generate documentation for the workspace
docs:
  cargo doc --no-deps --open

# Run security audit
audit:
  cargo audit

# Check for outdated dependencies
outdated:
  cargo outdated

# Run clippy with all lints
clippy-all:
  cargo +nightly clippy --all-targets --all-features -- -W clippy::pedantic -W clippy::nursery -W clippy::cargo

# Run tests with nextest
test-nextest:
  cargo nextest run --all-features

# Run miri on unsafe code (needs `rustup +nightly component add miri`)
miri:
  cargo +nightly miri test --all-features

# Check for unused dependencies
unused-deps:
  cargo machete

# Update all dependencies
update-deps:
  cargo update

# Full check (lint + test + build + audit)
full-check: lint test build audit
