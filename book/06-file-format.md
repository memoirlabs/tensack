# File Format

The durable readable row format is `.6`.

The generated lookup/cache format is `.6b`.

The future generated full-text format is `.6x`.

## `.6`

Current profile:

```txt
SIX<TAB>1<TAB>table<TAB>messages<TAB><schema_hash>
@field<TAB>id<TAB>id
@field<TAB>body<TAB>text
@lookup<TAB>id<TAB>unique
@data
R<TAB>1<TAB>m1<TAB>hello
D<TAB>2<TAB>m1
```

Rules:

- `R` is a full replacement row.
- `D` is a delete tombstone by id.
- Field values are written in schema field order.
- Values escape tab, newline, carriage return, and backslash.

## `.6b`

`.6b` is generated cache state.

Current implementation uses binary-packed v2 caches.

It stores:

- cache version
- table name
- schema hash
- source hash for `.6` chunks
- live row id to row pointer entries
- lookup field/key to row id entries

The encoding is internal. It can change as long as it remains rebuildable from
schema plus `.6`.

## `.6x`

Reserved for optional generated full-text search.

Not required for normal id lookup, declared lookups, `get` selectors, or
`write` changes.

## Format Boundary

Format behavior belongs in:

```txt
packages/sixpack-format
```

It should not know application API decisions.
