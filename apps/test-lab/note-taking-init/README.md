# Note Taking Init Experiment

This is a test-lab example for the compiler/init path. It is intentionally not
part of the core database implementation.

It demonstrates the conservative lifecycle for a new app:

1. Start with the smallest useful schema: `schema.v1.tensack` contains only a
   blank `notebooks` table.
2. Run init and verify the database folder exists with the one table.
3. Grow the schema: `schema.v2.tensack` adds a `notes` table.
4. Run init again, write a notebook and note through the generated Rust SDK,
   and query the `notes.notebook_id` lookup.
5. Grow the schema again: `schema.tensack` adds a `tags` table.
6. Run init again, write a tag through the generated Rust SDK, and query the
   `tags.note_id` lookup.

The crate build script generates the latest Rust SDK from `schema.tensack`.
The runtime example also writes generated Rust schema output under
`target/test-lab/note-taking-init/generated/` for inspection and initializes a
local database under `target/test-lab/note-taking-init/notes-db/`.

By default, the binary prints only the current user-facing schema and database
paths. Versioned generated files and storage/index files are kept as internal
artifacts. Pass `--show-artifacts` when you intentionally want to inspect them.

Run both phases:

```sh
cargo run -p note-taking-init -- --reset
```

Show internal artifacts:

```sh
cargo run -p note-taking-init -- --reset --show-artifacts
```

Run one phase at a time:

```sh
cargo run -p note-taking-init -- --reset --phase v1
cargo run -p note-taking-init -- --phase v2
cargo run -p note-taking-init -- --phase v3
```

Expected final database shape:

```txt
generated/
  schema.rs
  artifacts/
    schema-v1.rs
    schema-v2.rs
    schema-v3.rs
notes-db/
  tensack.toml
  engine/
    notebooks.tenb
    notes.tenb
    tags.tenb
  tables/
    notebooks/
      zzz.ten
    notes/
      zzz.ten
    tags/
      zzz.ten
```

The latest schema has three tables:

- `notebooks`: `id`, `title`, `created_at`
- `notes`: `id`, `notebook_id`, `title`, `body`, `updated_at`
- `tags`: `id`, `note_id`, `label`, `created_at`
