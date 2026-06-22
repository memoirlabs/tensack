# Atlas

This is the current map of the repository and architecture. When storage,
package boundaries, public APIs, or file layout change, update this page in the
same commit as the code and tests.

## Current Repository Map

```txt
packages/sixpack-core            schema, records, values, domain types
packages/sixpack-format          .6 and .6b encoding boundary
packages/sixpack-store           local storage engine
packages/sixpack                 public Database API composition
packages/sixpack-cli             CLI command behavior
packages/sixpack-schema-compiler schema! parser, validator, raw Rust output
packages/sixpack-testkit         shared test helpers

apps/sixpack                     runnable CLI binary
apps/test-lab                    experiments and generated examples
apps/admin-ui                    planned local viewer/admin surface
apps/landing-page                public docs/site surface

benchmark                        Criterion benchmarks
tests/contracts                  public behavior contract location
tests/snapshots                  reviewed snapshot location
packages/docs                    public command and file-format docs
book                             active maintainer design map
book/reference                   archived background drafts, not source of truth
```

## Source Of Truth

```txt
README.md                         public overview
AGENTS.md                         agent implementation contract
packages/docs/commands.md         shipped CLI contract
packages/docs/file-format.md      public physical format contract
book/05-storage.md                storage architecture
book/06-file-format.md            internal format notes
book/07-implementation-status.md  shipped vs not shipped status
book/decisions/*                  accepted architecture decisions
```

Archived files under `book/reference` are background only. If they conflict
with the files above, the active files win.

## Current Runtime Flow

```txt
schema declaration
  -> generated selectors and changes
  -> Database get/write/write_many
  -> validated internal plan
  -> append readable .6 row operations
  -> update runtime lookup/count projection
  -> persist rebuildable generated binary state
```

## Current Disk Layout

The implementation today writes one generated `.6b` cache per table.

```txt
my-db.sixpack/
  sixpack.toml
  tables/
    messages/
      zzz.6
    users/
      zzz.6
  engine/
    messages.6b
    users.6b
```

Current truth:

- `.6` files are canonical row operation data.
- `sixpack.toml` is compact recoverable metadata.
- `.6b` files are generated and rebuildable.
- table chunk files stay flat under `tables/<table>/`.

## Target Disk Layout

The desired direction is still a folder database with readable table folders.
Generated engine state should collapse into one private rebuildable binary pack
instead of many table-local cache files.

```txt
my-db.sixpack/
  sixpack.toml
  schema.sixpack
  tables/
    messages/
      zzz.6
      zzy.6
    users/
      zzz.6
  engine/
    state.6pack
```

Target truth:

- the database is a directory, not an opaque primary file;
- `tables/<table>/*.6` remains the readable canonical data;
- `schema.sixpack` remains the logical schema source when present;
- `engine/state.6pack` is private generated engine state;
- deleting `engine/state.6pack` must not lose user data;
- startup can rebuild generated state from schema plus `.6` data.

## Open Implementation Work

- Introduce the engine state pack format behind the store boundary.
- Stop letting code outside the cache boundary assume one `.6b` file per table.
- Add layout contract tests for current and target storage behavior as the
  implementation changes.
- Keep `book/05-storage.md`, `book/06-file-format.md`,
  `packages/docs/file-format.md`, and this atlas synced with storage changes.
