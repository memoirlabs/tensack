# Package Boundaries

Boundaries should be named after what they own.

## Current Packages

```txt
packages/tensack-core
packages/tensack-format
packages/tensack-store
packages/tensack
packages/tensack-cli
packages/tensack-schema-compiler
packages/tensack-testkit
```

## What Each Owns

### `tensack-core`

Current domain model:

- schema types
- record type
- value type
- workspace type
- schema errors

Concern: `core` is vague. A better future name is probably
`tensack-schema` or `tensack-model`.

Do not let this become a junk drawer.

### `tensack-format`

Durable encoding and decoding:

- `.ten`
- `.tenb`
- row pointers
- source hashes

It should not own runtime orchestration.

### `tensack-store`

Local storage engine:

- database directory paths
- chunk paths
- appends
- table scans
- cache rebuilds
- lookup reads

It should not expose storage internals as the normal app API.

### `tensack`

Composed runtime API:

- `TensackDatabase`
- `get` and `write` request execution
- plan executor
- public re-exports

### `tensack-schema-compiler`

Build-time schema compiler:

- parse schema
- validate schema
- emit generated Rust

It should not be required for runtime schema parsing.

### `tensack-cli`

CLI command parsing and execution.

Keep it small until the runtime contract is stable.

## Preferred Future Rename

Consider:

```txt
packages/tensack-core -> packages/tensack-schema
crate tensack_core    -> tensack_schema
```

Only do this as an intentional rename, not mixed into unrelated behavior work.
