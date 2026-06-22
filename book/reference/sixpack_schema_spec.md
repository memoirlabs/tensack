# sixpack Schema Spec

The schema defines logical tables, fields, primitive types, and lookup indexes.
It is the source used to generate user-facing table APIs.

## Goals

- Keep schema authoring small and SQL-free.
- Validate names, fields, lookups, and primitive types before runtime.
- Generate typed row structs and table-specific APIs.
- Keep runtime schema parsing out of the final application path.

## Authoring Shape

The current schema surface is a Rust-adjacent `schema!` body:

```rust
schema! {
  users {
    id id
    email text
    name text
    active bool

    lookup email unique
    lookup active
  }

  messages {
    id id
    conversation_id id
    body text
    created_at int

    lookup conversation_id
    lookup created_at
  }
}
```

Semicolons are optional in the current parser.

## Primitive Types

```txt
id    -> String
text  -> String
int   -> i64
float -> f64
bool  -> bool
```

## Table Rules

- Every table must have an `id` field.
- `id` must use primitive type `id`.
- Table names must be public snake_case identifiers.
- Field names must be public snake_case identifiers.
- Field names cannot start with `_`.
- Duplicate table names are rejected.
- Duplicate field names are rejected.
- Duplicate lookup declarations are rejected.

## Lookup Rules

Every table has an implicit unique `id` lookup.

Explicit lookups are declared inside the table:

```txt
lookup email unique
lookup conversation_id
```

- `unique` means at most one live row can use the key.
- Omitted `unique` means many rows can share the key.
- A lookup must refer to a declared field.
- `get` APIs are generated for unique lookups.
- `find` APIs are generated for non-unique lookups.

## Build-Time Flow

```txt
schema.sixpack
  -> compile_schema(source)
  -> validate_schema(ir)
  -> SchemaIr
  -> generated Rust row/table API
  -> application uses generated code
```

The runtime should use generated Rust and canonical schema metadata. It should
not parse schema text on every database open.

## Current Implementation

Implemented today:

- `packages/sixpack-schema-compiler`
- `compile_schema`
- `validate_schema`
- `database_schema_from_ir`
- raw Rust output through `emit_raw_rust`
- `schema!` macro support in `packages/sixpack`

Not complete yet:

- final generated API shape from [sixpack_api_spec.md](sixpack_api_spec.md)
- stable snapshot-reviewed generated output
- full build integration for normal apps
