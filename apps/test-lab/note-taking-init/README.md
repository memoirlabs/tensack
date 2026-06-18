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

Run both phases:

```sh
cargo run -p note-taking-init -- --reset
```

Run one phase at a time:

```sh
cargo run -p note-taking-init -- --reset --phase v1
cargo run -p note-taking-init -- --phase v2
cargo run -p note-taking-init -- --phase v3
```

Expected final database shape:

```txt
notes-db/
  tensack.toml
  engine/
    notebooks.tenb
    notes.tenb
    tags.tenb
  tables/
    notebooks/
      active.ten
    notes/
      active.ten
    tags/
      active.ten
```

The latest schema has three tables:

- `notebooks`: `id`, `title`, `created_at`
- `notes`: `id`, `notebook_id`, `title`, `body`, `updated_at`
- `tags`: `id`, `note_id`, `label`, `created_at`
