# Naming Rules

Names should make the system easier to understand.

## Approved Public Terms

- `Database`
- `Schema`
- `Table`
- `Field`
- `Lookup`
- `Record`
- `Value`
- `Plan`
- `Store`
- `Format`
- `Chunk`
- `Cache`

## Avoid

- cute internal shorthand
- product-name puns
- vague package names where a precise one is available
- generic public string APIs as the main product surface

## Specific Decisions

### `Value`

Use:

```rust
Value
```

### `upsert`

Generated public API uses:

```txt
upsert
```

Runtime compatibility can keep:

```txt
put
```

### `remove`

Generated public API uses:

```txt
remove
```

The store can still write delete tombstones.

### `get` and `find`

Use:

```txt
get.<unique_lookup>
find.<lookup>
```

`get` returns one or none.

`find` returns many.
