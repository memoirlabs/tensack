set shell := ["bash", "-cu"]

default:
    just --list

fmt:
    cargo fmt --all

check:
    cargo check --workspace --all-targets

test:
    cargo test --workspace --all-targets

lint:
    cargo clippy --workspace --all-targets -- -D warnings

install:
    ./packaging/install.sh
