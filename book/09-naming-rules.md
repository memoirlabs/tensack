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
- `Selector`
- `Change`
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

### `get`

Use for current state once:

```txt
db.get(selector)
```

### `watch`

Reserve for live subscriptions:

```txt
db.watch(selector)
```

Do not claim this is implemented until subscriptions actually update after
writes.

### `write`

Use for applying declared changes:

```txt
db.write(change)
```

### Change Words

Generated changes may use obvious words under `db.write(...)`:

```txt
add
set
edit
remove
```
