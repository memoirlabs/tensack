# Schema Compiler

The schema compiler is build-time infrastructure.

It should parse schema input, validate it, and emit generated Rust. The runtime
should not parse schema text as its normal path.

## Current Crate

```txt
packages/sixpack-schema-compiler
```

## Current Responsibilities

- parse `schema! { ... }`
- validate table names
- validate field names
- validate primitive types
- validate duplicate tables and fields
- validate lookup targets
- build `SchemaIr`
- convert IR to runtime `DatabaseSchema`
- emit raw Rust generated API code

## Current API

```rust
compile_schema(source)
validate_schema(ir)
database_schema_from_ir(ir)
emit_raw_rust(ir)
```

## Generated Shape

Generated Rust currently includes:

- typed `Row`
- `Row::into_record`
- `Row::from_record`
- `Patch`
- unique lookup keys
- generated `by` selectors for `db.get(...)`
- generated `all` and `count` selectors
- generated `add`, `set`, `edit`, and `remove` changes for `db.write(...)`
- table extension trait

## Next Compiler Work

- stable snapshot tests for generated output
- normal build integration path
- final generated API naming pass
- less stringly runtime glue where Rust types can carry the information
