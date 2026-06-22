# sixpack

[![Rust](https://img.shields.io/badge/Rust-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![local-first](https://img.shields.io/badge/local--first-directory%20database-2f6f73?style=flat-square)](https://www.inkandswitch.com/local-first/)
[![append-only](https://img.shields.io/badge/append--only-.6%20segments-3b4252?style=flat-square)](packages/docs/file-format.md)
[![binary index](https://img.shields.io/badge/binary%20index-.6b-5e81ac?style=flat-square)](packages/docs/file-format.md#generated-cache)
[![single-app](https://img.shields.io/badge/single--app-fast%20local%20path-b48ead?style=flat-square)](book/14-write-engine.md)
[![Criterion](https://img.shields.io/badge/benchmarked-Criterion-d08770?style=flat-square)](https://bheisler.github.io/criterion.rs/book/)
[![SQLite baseline](https://img.shields.io/badge/baseline-SQLite-003b57?style=flat-square&logo=sqlite&logoColor=white)](https://www.sqlite.org/)

sixpack is a local-first database layer for small tools, agent runtimes,
desktop apps, research projects, and quantitative workflows that want a typed
API without giving up inspectable local data.

The idea is simple: write canonical data to readable `.6` append segments,
serve reads through generated binary `.6b` projections, and expose the whole
thing through schema-generated Rust selectors and changes.

```txt
schema -> generated API -> tiny runtime plan -> append-only .6 -> binary .6b reads
```

sixpack is early, but the core shape is already useful: a small local table
database with typed primitive fields, declared lookups, fast append writes,
rebuildable indexes, and a public API centered on `get`, `write`, and
`write_many`.

## The Shape

sixpack is built around the idea that the database API should look like the
application's own data model:

```rust
sixpack::schema! {
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

db.write_many(&[
    messages::edit(messages::key::id("m1"), patch),
    messages::remove(messages::key::id("m2")),
])?;
```

No SQL strings. No storage paths in application code. The schema creates the
selectors and changes; the runtime turns them into a small validated plan.

## Why It Exists

Most embedded databases are powerful because they expose a database language.
sixpack goes the other way: the schema is the language.

That syntax compiles down to a small internal plan. The store does not need SQL,
a VM, or ad hoc string parsing. The engine can stay narrow: validate, append,
publish a projection, and rebuild generated state when needed.

## Storage Model

sixpack keeps one source of truth and treats everything else as generated
acceleration.

```txt
schema.sixpack  logical schema truth
tables/*.6    canonical append-only row operations
engine/*.6b   generated binary row pointers and lookup indexes
engine/*.6x   optional generated full-text index, planned
sixpack.toml    compact recoverable engine metadata
```

The hot path is intentionally boring:

```txt
validate change
append .6
update runtime .6b projection
recover metadata/cache from .6 when needed
```

Readable `.6` data makes debugging and recovery straightforward. Binary
`.6b` projections keep normal id, lookup, scan, and count reads fast. The
mathematics is simple and predictable: appends are sequential, lookups are
sorted index probes, batches amortize validation and metadata work, and
generated files can always be discarded.

At runtime, sixpack keeps the hot projection in map form:

```txt
rows_by_id        row id -> .6 row pointer
lookup_ids        lookup field + key -> row ids
row_lookup_keys   row id -> lookup keys currently attached to that row
```

That is the edit fast path. A patch does not scan the table or rebuild the
database. It resolves the row, appends the replacement operation, removes only
that row's old lookup keys, inserts the new lookup keys, and updates the row
pointer. The persisted `.6b` file stays a compact generated binary snapshot;
the runtime map can be materialized back into `.6b` when the engine needs a
durable cache boundary.

## Status

Implemented today:

- schema primitives and row validation
- append-only `.6` put/delete rows
- generated binary `.6b` lookup caches
- runtime `.6b` maps for hot ids, lookups, counts, and edits
- id lookup, declared lookup reads, scans, and counts
- `db.get(...)`, `db.write(...)`, and `db.write_many(...)`
- same-table batched writes
- recoverable metadata counters
- schema compiler parser, validator, and raw Rust output
- cached generated schema accessors for compiled APIs
- CLI help/version surface

Planned or incomplete:

- `db.watch(...)` live subscriptions
- repair/inspect CLI
- admin UI
- `.6x` full-text search
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
use sixpack::{
    change, selector, DatabaseSchema, PrimitiveType, Record, TableSchema,
    Database, Value,
};

let mut schema = DatabaseSchema::new();
let mut messages = TableSchema::new("messages");
messages.add_field("id", PrimitiveType::Id)?;
messages.add_field("conversation_id", PrimitiveType::Id)?;
messages.add_field("body", PrimitiveType::Text)?;
messages.add_lookup("conversation_id", false)?;
schema.add_table(messages)?;

let db = Database::open_local_with_schema("./data", "chat", schema);

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
sixpack::schema! {
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

Benchmarks compare sixpack's current local storage path against SQLite baselines
for the same table shape. The hot-path benchmark preloads 10,000 rows once,
keeps the database handle open, then measures 1,000 operations per Criterion
iteration. Read/count cases stay fixed-size; write cases keep mutating the same
live handle so they measure ongoing append/update behavior instead of database
regeneration.

```sh
RUSTFLAGS='-C target-cpu=native' \
  cargo bench -p sixpack-benchmark --bench hot_path -- \
  --sample-size 10 --warm-up-time 0.2 --measurement-time 1.0
```

| Operation, live DB | sixpack | SQLite | What is being measured |
| --- | ---: | ---: | --- |
| get by id, 10k rows, 1,000 ops | ~0.50 ms | ~3.19 ms | runtime `.6b` row pointer + row cache vs indexed select |
| count, 10k rows, 1,000 ops | ~26 us | ~3.10 ms | runtime binary projection count vs `COUNT(*)` |
| add one-by-one, starts at 10k rows, 1,000 ops | ~38.7 ms | ~301.1 ms | append `.6`, update runtime projection |
| add batched, starts at 10k rows, 1,000 rows | ~4.28 ms | ~6.83 ms | one sixpack segment vs one SQLite transaction |
| edit one-by-one, 10k live rows, 1,000 ops | ~38.8 ms | ~5.69 ms | append replacements, update runtime projection |
| edit batched, 10k live rows, 1,000 rows | ~5.13 ms | ~0.52 ms | one sixpack patch batch vs one SQLite transaction |

The useful result is specific: hot reads and counts are already served from
binary generated state and are strong in this workload. Append batches are now
competitive in this local path because sixpack writes one readable segment and
updates a runtime projection map. SQLite transactions are still much faster for
large update batches. The next engine target is to keep the readable `.6`
source while reducing patch construction and row rewrite overhead.

## Repository Layout

```txt
packages/sixpack                 public runtime API
packages/sixpack-core            schema, records, values, domain types
packages/sixpack-format          .6 and .6b encoding boundary
packages/sixpack-store           local storage engine
packages/sixpack-cli             CLI command surface
packages/sixpack-schema-compiler schema! parser, validator, codegen
packages/sixpack-testkit         shared test helpers
apps/sixpack                     runnable CLI binary
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
cargo bench -p sixpack-benchmark --bench crud_vs_sqlite
```

sixpack is still a v0 scaffold, but the project direction is stable: local
data, typed schemas, append-first writes, generated binary indexes, and a small
API that stays pleasant as applications grow.
