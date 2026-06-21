# Schema

Schema defines the logical tables, fields, primitive types, and lookups.

Schema is build-time input for generated APIs. The runtime should use generated
Rust and canonical metadata, not parse schema text on every open.

## Authoring Shape

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

Semicolons may be accepted by the parser, but the clean style above is the
preferred shape.

## Primitive Types

```txt
id    -> String
text  -> String
int   -> i64
float -> f64
bool  -> bool
```

## Rules

- Every table has an `id` field.
- `id` must be primitive type `id`.
- Table names are public snake_case identifiers.
- Field names are public snake_case identifiers.
- Field names cannot start with `_`.
- Duplicate tables are rejected.
- Duplicate fields are rejected.
- Duplicate lookups are rejected.
- Lookups must refer to declared fields.

## Lookups

Every table has an implicit unique `id` lookup.

Explicit lookups:

```txt
lookup email unique
lookup conversation_id
```

- `unique` means at most one live row can use the key.
- Omitted `unique` means many rows can share the key.
- unique lookups generate selectors that return zero or one row.
- non-unique lookups generate selectors that return many rows.

## Implementation

Current crate:

```txt
packages/tensack-schema-compiler
```

Current important functions:

```txt
compile_schema(source)
validate_schema(ir)
database_schema_from_ir(ir)
emit_raw_rust(ir)
```

The generated output already includes selectors, changes, patches, lookup keys,
page/count selectors, and table extension traits. It still needs stable
snapshots.
