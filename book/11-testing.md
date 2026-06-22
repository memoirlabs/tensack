# Testing

sixpack is the database product, so tests must exercise local directory-backed
behavior directly.

## Core Rule

Use temporary directories for data-bearing tests.

Never write disposable test data to:

```txt
.sixpack
.data
repo root paths
real user workspace paths
```

## Mental Model

```txt
A sixpack database instance = one directory.
A test database = one temporary directory.
Resetting the database = deleting that directory.
```

## Test Layers

### Unit Tests

Use for pure deterministic pieces:

- schema validation
- primitive type mapping
- value parsing
- row encoding
- row pointer parsing
- chunk path encoding

### Integration Tests

Use temporary directories and real store/runtime calls:

- open database
- initialize layout
- write add/set/edit/remove changes
- get by id selector
- get by lookup selector
- get page and count selectors
- rebuild caches
- verify unique lookup conflicts

### Contract Tests

Use for public behavior:

- CLI output
- file format stability
- generated API behavior once stable

### Snapshot Tests

Use only for stable reviewed output:

- CLI help
- generated schema output
- stable format rendering

## Required Checks

Run from repo root:

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```
