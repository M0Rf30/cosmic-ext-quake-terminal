name := 'cosmic-ext-quake-terminal'
export APPID := 'com.github.m0rf30.CosmicExtQuakeTerminal'

rootdir := ''
prefix := '/usr'

base-dir := absolute_path(clean(rootdir / prefix))

cargo-target-dir := env('CARGO_TARGET_DIR', 'target')
bin-src := cargo-target-dir / 'release' / name
bin-dst := base-dir / 'bin' / name

# Default recipe
default: build-release

# Remove build artifacts
clean:
    cargo clean

# Remove vendored dependencies
clean-vendor:
    rm -rf .cargo vendor vendor.tar

# Full clean
clean-dist: clean clean-vendor

# Debug build
build-debug *args:
    cargo build {{args}}

# Release build
build-release *args: (build-debug '--release' args)

# Clippy
check *args:
    cargo clippy --all-features {{args}} -- -W clippy::pedantic

# Run with debug logging
run *args:
    env RUST_LOG=cosmic_ext_quake_terminal=debug RUST_BACKTRACE=full cargo run --release -- {{args}}

# Install
install:
    install -Dm0755 {{bin-src}} {{bin-dst}}
    just data/install

# Uninstall
uninstall:
    rm -f {{bin-dst}}
    just data/uninstall

# Vendor dependencies
vendor:
    #!/usr/bin/env bash
    mkdir -p .cargo
    cargo vendor --sync Cargo.toml | head -n -1 > .cargo/config.toml
    echo 'directory = "vendor"' >> .cargo/config.toml
    tar pcf vendor.tar .cargo vendor
    rm -rf .cargo vendor

# Extract vendored dependencies
vendor-extract:
    rm -rf vendor
    tar pxf vendor.tar
