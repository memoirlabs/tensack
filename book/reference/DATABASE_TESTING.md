# Database Testing

This repository is for sixpack, a local-first database project. The current root
workspace is a v0 Rust scaffold. The runtime is not fully implemented in the root
crates yet, but the repo direction is clear:

- sixpack data is local data.
- A database instance is a directory on disk.
- Durable data should be human-inspectable enough for local debugging.
- Readable `.6` table data is the source of truth in the target backend direction.
- Lookup files are rebuildable sidecars, not the logical data model.
- Tests must be able to create and destroy database instances without touching
  real user data.

The practical testing model should follow from that: a test database is a
temporary directory.

## Current Repo Shape

The root workspace currently contains:

- `packages/sixpack-core` - core data model boundaries.
- `packages/sixpack-format` - file format boundary and version/header shell.
- `packages/sixpack-store` - local storage boundary with a `LocalStore` handle.
- `apps/sixpack` - CLI shell with `help`, `--version`, and a future command set.
- `tests/contracts` - intended home for public behavior contract tests.
- `tests/snapshots` - intended home for reviewed stable output snapshots.

The current storage path opens a database at a caller-provided path, creates
directories such as `tables/` and `engine/`, writes `sixpack.toml`, appends table
rows to `.6` files, and writes rebuildable `.6b` lookup/index caches.

## Mental Model

For sixpack, the database boundary should be:

```txt
A sixpack database instance = one directory.
A test database = one temporary directory.
Resetting the database = deleting that directory.
```

That is different from a normal web app repository. A normal app usually points
at an external database service:

```txt
app -> Postgres/Supabase/SQLite/Neon/etc.
```

sixpack is the database product. The important behavior is the storage engine
itself:

```txt
sixpack API -> local directory -> sixpack.toml/tables/engine
```

So tests need to check filesystem-backed behavior directly.

## Rule For Test Isolation

Tests must not open real project data paths such as:

- `.sixpack`
- `./data`
- a user-provided persistent workspace path

Tests should create a unique temporary directory, open the database there, seed
whatever data the test needs, assert behavior, then let the directory be deleted.

In Rust, the likely tool for this is the `tempfile` crate:

```rust
#[test]
fn example_uses_a_disposable_database_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Open the database at `root`.
    // Write test data.
    // Assert behavior.

    // `dir` is deleted when it is dropped.
}
```

Keeping the `TempDir` value alive for the full test matters. If only the path is
kept, the temporary directory may be deleted too early.

## Recommended Test Layers

Use three levels of tests.

## Unit Tests

Unit tests should cover small, deterministic pieces that do not need a real
database directory.

Good examples:

- schema name validation
- lookup key validation
- value encoding and decoding
- row pointer parsing
- manifest parsing
- pure format helpers

These tests should be fast and should not depend on filesystem state unless the
unit being tested is specifically filesystem code.

## Integration Tests With Temporary Directories

This should be the main database testing environment.

Each test gets a fresh root:

```txt
/tmp/sixpack-test-random-id/
```

Then the test can do real database work:

- open an empty database
- verify initial files are created
- insert rows
- read rows by id
- read rows by lookup
- close and reopen
- verify data survives reopen
- test duplicate id behavior
- test duplicate unique lookup behavior
- test schema mismatch behavior
- test deletes and lookup invalidation when those features exist

This style gives realistic storage coverage without risking actual data.

## Contract Tests

Tests under `tests/contracts` should lock down public behavior across crate
boundaries. They should describe behavior from a caller's point of view and avoid
depending on private implementation details.

Good contract surfaces for this repo:

- CLI command output and exit behavior.
- File format compatibility behavior.
- Local store behavior.
- Eventually, generated API behavior once the schema/codegen/runtime surface is
  promoted into the root workspace.

## Snapshot Tests

Tests under `tests/snapshots` should be for stable reviewed output, not arbitrary
implementation internals.

Good snapshot candidates:

- CLI help output.
- stable format rendering.
- generated schema output once codegen is stable enough to review.

Snapshots should be updated intentionally and reviewed as part of a change.

## What To Avoid

Avoid shared mutable test databases. They make tests order-dependent and create
cleanup problems.

Avoid tests that write to `.sixpack` in the repository root. That path is for
real local workspace state, not disposable test state.

Avoid assuming lookup files are the source of truth. The current architecture
direction treats lookup files as rebuildable acceleration.

Avoid adding an external database server just to test sixpack. That would test a
different product shape than the one described by this repo.

Avoid hand-modeled SDK behavior that drifts from the schema/codegen direction.
The architecture notes say SDKs should come from a shared schema IR.

## Recommended Helper Shape

Once the root workspace has a real storage engine API, add a small test helper
instead of repeating setup in every test:

```rust
pub struct TestDb {
    _dir: tempfile::TempDir,
    pub root: std::path::PathBuf,
}

impl TestDb {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        Self { _dir: dir, root }
    }
}
```

Tests would then use:

```rust
let test_db = TestDb::new();
// Open sixpack at `&test_db.root`.
```

The `_dir` field intentionally keeps the temporary directory alive until the
helper is dropped.

## Current Verification Commands

The root `justfile` defines the standard local checks:

```sh
just fmt
just check
just test
just lint
```

Those map to:

```sh
cargo fmt --all
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

Use those commands for the root workspace.
