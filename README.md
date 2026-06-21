# Tensack

[![Rust](https://img.shields.io/badge/Rust-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![local-first](https://img.shields.io/badge/local--first-directory%20database-2f6f73?style=flat-square)](https://www.inkandswitch.com/local-first/)
[![append-only](https://img.shields.io/badge/append--only-.ten%20segments-3b4252?style=flat-square)](packages/docs/file-format.md)
[![binary index](https://img.shields.io/badge/binary%20index-.tenb-5e81ac?style=flat-square)](packages/docs/file-format.md#generated-cache)
[![single-app](https://img.shields.io/badge/single--app-fast%20local%20path-b48ead?style=flat-square)](book/14-write-engine.md)
[![Criterion](https://img.shields.io/badge/benchmarked-Criterion-d08770?style=flat-square)](https://bheisler.github.io/criterion.rs/book/)
[![SQLite baseline](https://img.shields.io/badge/baseline-SQLite-003b57?style=flat-square&logo=sqlite&logoColor=white)](https://www.sqlite.org/)

Tensack is a local-first database layer for small tools, agent runtimes,
desktop apps, research projects, and quantitative workflows that want a typed
API without giving up inspectable local data.

The idea is simple: write canonical data to readable `.ten` append segments,
serve reads through generated binary `.tenb` projections, and expose the whole
thing through schema-generated Rust selectors and changes.

```txt
schema -> generated API -> tiny runtime plan -> append-only .ten -> binary .tenb reads
```

Tensack is early, but the core shape is already useful: a small local table
database with typed primitive fields, declared lookups, fast append writes,
rebuildable indexes, and a public API centered on `get`, `write`, and
`write_many`.

## Why It Exists

Most embedded databases are powerful because they expose a database language.
Tensack goes the other way: the schema is the language.

Application code should read like the local model:

```rust
db.get(messages::by::conversation_id("cv1"))?;
db.write(messages::add(row))?;
db.write_many(&[
    messages::edit(messages::key::id("m1"), patch),
    messages::remove(messages::key::id("m2")),
])?;
```

That syntax compiles down to a small internal plan. The store does not need SQL,
a VM, or ad hoc string parsing. The engine can stay narrow: validate, append,
publish a projection, and rebuild generated state when needed.

## Storage Model

Tensack keeps one source of truth and treats everything else as generated
acceleration.

```txt
schema.tensack  logical schema truth
tables/*.ten    canonical append-only row operations
engine/*.tenb   generated binary row pointers and lookup indexes
engine/*.tenx   optional generated full-text index, planned
tensack.toml    compact recoverable engine metadata
```

The hot path is intentionally boring:

```txt
validate change
append .ten
publish in-memory .tenb projection
recover metadata/cache from .ten when needed
```

Readable `.ten` data makes debugging and recovery straightforward. Binary
`.tenb` projections keep normal id, lookup, scan, and count reads fast. The
mathematics is simple and predictable: appends are sequential, lookups are
sorted index probes, batches amortize validation and metadata work, and
generated files can always be discarded.

## Status

Implemented today:

- schema primitives and row validation
- append-only `.ten` put/delete rows
- generated binary `.tenb` lookup caches
- id lookup, declared lookup reads, scans, and counts
- `db.get(...)`, `db.write(...)`, and `db.write_many(...)`
- same-table batched writes
- recoverable metadata counters
- schema compiler parser, validator, and raw Rust output
- CLI help/version surface

Planned or incomplete:

- `db.watch(...)` live subscriptions
- repair/inspect CLI
- admin UI
- `.tenx` full-text search
- compaction and segment sealing
- stable generated API snapshots

## API Shape

The current runtime API is intentionally small:

```txt
db.get(selector)       read current state once
db.watch(selector)     planned live subscription
db.write(change)       apply one declared change
db.write_many(changes) apply one-table changes as a storage batch
```

The low-level runtime helpers are available today:

```rust
use tensack::{
    change, selector, DatabaseSchema, PrimitiveType, Record, TableSchema,
    TensackDatabase, Value,
};

let mut schema = DatabaseSchema::new();
let mut messages = TableSchema::new("messages");
messages.add_field("id", PrimitiveType::Id)?;
messages.add_field("conversation_id", PrimitiveType::Id)?;
messages.add_field("body", PrimitiveType::Text)?;
messages.add_lookup("conversation_id", false)?;
schema.add_table(messages)?;

let db = TensackDatabase::open_local_with_schema("./data", "chat", schema);

let row = Record::new("messages")
    .with_id("m1")?
    .with_field("conversation_id", Value::Id("cv1".to_owned()))?
    .with_field("body", "ship the local index")?;

db.write(change::add(row))?;

let messages = db.get(selector::many("messages", "conversation_id", "cv1"))?;
let count = db.get(selector::count("messages"))?;

db.write_many(&[
    change::edit_id(
        "messages",
        "m1",
        std::collections::BTreeMap::from([
            ("body".to_owned(), Value::Text("batched patch".to_owned())),
        ]),
    ),
])?;
```

The generated API is the target ergonomic layer:

```rust
tensack::schema! {
    messages {
        id id
        conversation_id id
        body text

        lookup conversation_id many
    }
}

db.write(messages::add(messages::Row {
    id: "m1".to_owned(),
    conversation_id: "cv1".to_owned(),
    body: "typed local state".to_owned(),
}))?;

let thread = db.get(messages::by::conversation_id("cv1"))?;
```

## Performance Snapshot

Benchmarks compare Tensack's current local storage path against SQLite baselines
for the same small table operations. Results below are from a shortened native
release Criterion run and should be read as directional, not as a guarantee.

```sh
RUSTFLAGS='-C target-cpu=native' \
  cargo bench -p tensack-benchmark --bench crud_vs_sqlite -- \
  --sample-size 10 --warm-up-time 0.1 --measurement-time 0.2
```

| Operation, 100 rows | Tensack fast path | SQLite baseline | What is being measured |
| --- | ---: | ---: | --- |
| add one-by-one | ~5.38 ms | ~22.8 ms | append `.ten`, publish in-memory `.tenb` |
| add batched | ~1.00 ms | ~54 us | one Tensack batch vs SQLite in-memory transaction |
| get by id | ~0.46 ms | ~0.43 ms | generated index lookup vs indexed select |
| count | ~0.40 ms | ~0.13 ms | binary projection count vs `COUNT(*)` |
| edit one-by-one | ~7.61 ms | ~21.5 ms | append replacements, update projection |
| edit batched | ~1.16 ms | ~21.5 ms | one patch batch |
| remove one-by-one | ~5.89 ms | ~23.9 ms | append tombstones, update projection |
| remove batched | ~0.87 ms | ~23.9 ms | one tombstone batch |

The important result is not that Tensack universally beats SQLite. SQLite is a
complete transactional SQL database and remains extremely strong, especially for
transactional batches and mature query planning. Tensack is optimizing a
different path: one local application writer, schema-declared operations,
append-only data, and generated projections that make small local state feel
like native Rust code.

## Repository Layout

```txt
packages/tensack                 public runtime API
packages/tensack-core            schema, records, values, domain types
packages/tensack-format          .ten and .tenb encoding boundary
packages/tensack-store           local storage engine
packages/tensack-cli             CLI command surface
packages/tensack-schema-compiler schema! parser, validator, codegen
packages/tensack-testkit         shared test helpers
apps/tensack                     runnable CLI binary
apps/test-lab                    isolated experiments and generated examples
benchmark                        Criterion benchmarks
tests/contracts                  public behavior contracts
packages/docs                    public format and command docs
book                             design book and implementation notes
```

Start with:

- [File layout](packages/docs/file-format.md)
- [Command surface](packages/docs/commands.md)
- [Product shape](book/01-product-shape.md)
- [Write engine](book/14-write-engine.md)
- [SQLite mapping](book/13-sqlite-mapping.md)
- [Implementation status](book/07-implementation-status.md)

## Development

Run the full workspace checks:

```sh
cargo fmt --all
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

Run the benchmark suite:

```sh
cargo bench -p tensack-benchmark --bench crud_vs_sqlite
```

Tensack is still a v0 scaffold, but the project direction is stable: local
data, typed schemas, append-first writes, generated binary indexes, and a small
API that stays pleasant as applications grow.
