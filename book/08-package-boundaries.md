# Package Boundaries

Boundaries should be named after what they own.

## Current Packages

```txt
packages/sixpack-core
packages/sixpack-format
packages/sixpack-store
packages/sixpack
packages/sixpack-cli
packages/sixpack-schema-compiler
packages/sixpack-testkit
```

## What Each Owns

### `sixpack-core`

Current domain model:

- schema types
- record type
- value type
- workspace type
- schema errors

Concern: `core` is vague. A better future name is probably
`sixpack-schema` or `sixpack-model`.

Do not let this become a junk drawer.

### `sixpack-format`

Durable encoding and decoding:

- `.6`
- `.6b`
- row pointers
- source hashes

It should not own runtime orchestration.

### `sixpack-store`

Local storage engine:

- database directory paths
- chunk paths
- appends
- table scans
- cache rebuilds
- lookup reads

It should not expose storage internals as the normal app API.

### `sixpack`

Composed runtime API:

- `Database`
- `get` and `write` request execution
- plan executor
- public re-exports

### `sixpack-schema-compiler`

Build-time schema compiler:

- parse schema
- validate schema
- emit generated Rust

It should not be required for runtime schema parsing.

### `sixpack-cli`

CLI command parsing and execution.

Keep it small until the runtime contract is stable.

## Preferred Future Rename

Consider:

```txt
packages/sixpack-core -> packages/sixpack-schema
crate sixpack_core    -> sixpack_schema
```

Only do this as an intentional rename, not mixed into unrelated behavior work.
