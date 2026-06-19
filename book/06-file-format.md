# File Format

The durable readable row format is `.ten`.

The generated lookup/cache format is `.tenb`.

The future generated full-text format is `.tenx`.

## `.ten`

Current profile:

```txt
TEN<TAB>1<TAB>table<TAB>messages<TAB><schema_hash>
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

## `.tenb`

`.tenb` is generated cache state.

Current implementation uses binary-packed v2 caches.

It stores:

- cache version
- table name
- schema hash
- source hash for `.ten` chunks
- live row id to row pointer entries
- lookup field/key to row id entries

The encoding is internal. It can change as long as it remains rebuildable from
schema plus `.ten`.

## `.tenx`

Reserved for optional generated full-text search.

Not required for normal id lookup, declared lookups, CRUD, scan, or count.

## Format Boundary

Format behavior belongs in:

```txt
packages/tensack-format
```

It should not know application API decisions.

