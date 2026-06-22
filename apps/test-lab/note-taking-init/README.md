# Note Taking Init Experiment

This is a test-lab example for the compiler/init path. It is intentionally not
part of the core database implementation.

It demonstrates the conservative lifecycle for a new app:

1. Start with the smallest useful schema: `schema.v1.sixpack` contains only a
   blank `notebooks` table.
2. Run init and verify the database folder exists with the one table.
3. Grow the schema: `schema.v2.sixpack` adds a `notes` table.
4. Run init again, write a notebook and note through the generated Rust SDK,
   and query the `notes.notebook_id` lookup.
5. Grow the schema again: `schema.sixpack` adds a `tags` table.
6. Run init again, write a tag through the generated Rust SDK, and query the
   `tags.note_id` lookup.

The crate build script generates the latest Rust SDK from `schema.sixpack`.
The runtime example also writes generated Rust schema output under
`target/test-lab/note-taking-init/generated/` for inspection and initializes a
local database under `target/test-lab/note-taking-init/notes-db/`.

By default, the binary prints only the current user-facing schema and database
paths. Versioned generated files and storage/index files are kept as internal
details. Pass `--show-internals` when you intentionally want to inspect them.

Run both phases:

```sh
cargo run -p note-taking-init -- --reset
```

Show internal details:

```sh
cargo run -p note-taking-init -- --reset --show-internals
```

Run a small update-speed pass and generate an HTML report:

```sh
cargo run -p note-taking-init -- --reset --speed-updates 1000 --show-internals
```

Run the same pass and compact the notes table afterward:

```sh
cargo run -p note-taking-init -- --reset --speed-updates 1000 --compact --show-internals
```

The report is written to:

```txt
target/test-lab/note-taking-init/generated/report.html
target/test-lab/note-taking-init/generated/speed-report.json
```

This is intentionally a test-lab page, not part of the public landing site.
It measures repeated generated-SDK `edit` writes against `note-1`. With
`--compact`, the report also shows canonical `.6` bytes before and after
rewriting live rows into a single compacted chunk.

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
  report.html
  speed-report.json
  internals/
    schema-v1.rs
    schema-v2.rs
    schema-v3.rs
notes-db/
  sixpack.toml
  engine/
    notebooks.6b
    notes.6b
    tags.6b
  tables/
    notebooks/
      zzz.6
    notes/
      zzz.6
    tags/
      zzz.6
```

The latest schema has three tables:

- `notebooks`: `id`, `title`, `created_at`
- `notes`: `id`, `notebook_id`, `title`, `body`, `updated_at`
- `tags`: `id`, `note_id`, `label`, `created_at`
