# Testing

Tensack is the database product, so tests must exercise local directory-backed
behavior directly.

## Core Rule

Use temporary directories for data-bearing tests.

Never write disposable test data to:

```txt
.tensack
.data
repo root paths
real user workspace paths
```

## Mental Model

```txt
A Tensack database instance = one directory.
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
- insert rows
- read by id
- read by lookup
- patch rows
- remove rows
- scan and count
- rebuild caches
- verify unique lookup conflicts

### Contract Tests

Use for public behavior:

- CLI output
- file format compatibility
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

