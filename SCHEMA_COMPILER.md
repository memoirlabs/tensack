# Schema Compiler (Rust)

`packages/tensack-schema-compiler` is the build-time schema compiler crate.

It currently does three things:

- parses the `schema! { ... }` style surface used in our examples,
- validates IDs, duplicates, and lookup references,
- emits a compact raw Rust row/module fragment for generated paths.

## Current shape

Input format is still basic and SQL-free:

```
schema! {
  users {
    id id
    email text
    name text
    score int

    lookup email unique
  }
}
```

`compile_schema` returns a validated `SchemaIr` and can be rendered into raw
generated Rust with `emit_raw_rust`.

This crate is standalone today; it is not yet wired into the app build pipeline.

## API (working)

- `compile_schema(source: &str) -> Result<SchemaIr, SchemaError>` (validates before returning)
- `validate_schema(ir: &SchemaIr) -> Result<(), SchemaError>`
- `emit_raw_rust(ir: &SchemaIr) -> String`

## End-to-end flow

Today the compiler can already handle a user schema with any number of tables
using the simple primitive types:

```rust
schema! {
  users {
    id id
    email text
    message_count int
    rating float
    disabled bool

    lookup email unique
    lookup disabled
  }

  messages {
    id id
    user_id id
    body text
    token_count int
    cost float
    flagged bool

    lookup user_id
    lookup flagged
  }
}
```

The intended build path is:

```txt
schema.tensack
  -> compile_schema(source)
  -> validated SchemaIr
  -> emit_raw_rust(ir)
  -> generated Rust modules/row structs/API helpers
  -> app imports generated Rust, runtime does not parse schema text
```

The current generated output is deliberately small. It proves the important
part first: every schema field resolves to the canonical Rust primitive mapping:

```txt
id    -> String
text  -> String
int   -> i64
float -> f64
bool  -> bool
```

Run the working example with:

```sh
cargo run -p tensack-schema-compiler --example compile_schema
```

That example compiles a multi-table schema, prints the IR summary, and prints
the raw Rust surface that future API generation will build from.

## Scope decision

This is intentionally small and build-time only for now:

- no runtime schema parsing in the database process,
- one canonical Rust representation,
- no host-DB, no JS runtime, no extra UI/tooling just for schema parsing.

If we later want stronger guarantees, we can extend this crate without changing
the `schema!` input contract first.
