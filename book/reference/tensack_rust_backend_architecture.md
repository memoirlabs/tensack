# Tensack Rust Backend Architecture

**Document purpose:** define the complete backend structure for `tensack`: a human-readable, TSV-backed local database with a tiny imported Rust-macro schema file, compile-time generated Rust registries, type-safe table/field/lookup APIs, and SDK generation from a shared schema IR.

**Status:** background reference. Current decisions live in `TENSACK_BOOK.md`
and the focused `TENSACK_*_SPEC.md` files.

**Current chunk naming update:** any older examples in this document that mention
`active.ten` or four-digit sealed segments are superseded by
`tensack_chunk_naming_spec.md`: table chunks use reverse lowercase base-36 paths
like `zz/zzz.ten`, `zz/zzy.ten`, and generation folders keep chunk filenames at
3 characters.

This document is written as an implementation spec. It assumes the public product goal is:

```txt
human-readable hot data
no SQL
no SQLite dependency
tiny Rust macro schema surface
compile-time safety in Rust
rebuildable lookup tables
simple SDK generation for many languages
```

---

## 1. Core Architecture

The correct structure is:

```txt
schema.tensack              small imported schema component
↓
Rust schema script        imports macros + includes schema.tensack
↓
Rust macro/codegen layer  expands + validates schema
↓
schema IR                tiny canonical model
↓
generated Rust registry  compile-time table/field/lookup definitions
↓
storage engine           TSV row files + lookup tables
↓
Rust codegen             generated Rust registry from same IR
```

The user-facing schema stays extremely basic. `schema.tensack` is Rust macro input, but it is not the full schema program; a normal Rust schema script imports Tensack macros and includes it.

The key design rule:

```txt
User-facing layer:
  tiny, dumb, declarative, readable Rust macros

Rust backend:
  strict, generated, type-safe, registry-driven

Storage:
  human-readable .ten table data as source of truth
  internal transactions/logs/sync/indexes in tensack.toml and .btf
  rebuildable lookup sidecars

SDKs:
  generated from canonical IR
  never hand-modeled independently
```

---

## 2. User-Facing Schema

Do **not** expose ordinary Rust type modeling as the schema contract.

Do **not** expose:

```txt
lifetimes
generics
serde
enums
traits
SQL
foreign keys
query planners
storage internals
B-trees
WAL details
page layout
```

The visible schema should stay small and declarative. `schema.tensack` is a macro component included by a Rust schema script, not a separate custom DSL and not the whole generator.

Example `schema.tensack`:

```rust
schema! {
  users {
    id id
    email text
    name text
    age int
    score float
    active bool

    lookup email unique
  }

  conversations {
    id id
    title text
    created_at int

    lookup created_at
  }

  messages {
    id id
    conversation_id id
    role text
    body text
    created_at int

    lookup conversation_id
    lookup created_at
  }
}
```

This is enough for v1.

---

## 3. Public Primitive Types

The first version should only support these public types:

```txt
id      stable string id, stored as text under the hood
text    UTF-8 string
int     signed 64-bit integer
float   64-bit float
bool    true / false
```

Maybe later:

```txt
blob    bytes
json    escape hatch
date    semantic wrapper around int
```

But for v1, do **not** include them.

The first version should be intentionally tiny.

---

## 4. Internal Type Mapping

User-facing types map to fixed internal Rust types.

```rust
pub enum SackType {
    Id,
    Text,
    Int,
    Float,
    Bool,
}
```

Concrete Rust mapping:

```rust
pub type SackId = String;
pub type SackText = String;
pub type SackInt = i64;
pub type SackFloat = f64;
pub type SackBool = bool;
```

Storage mapping:

```txt
id      TSV text cell, escaped
text    TSV text cell, escaped
int     decimal i64 text
float   decimal f64 text
bool    true / false
```

Example stored row:

```txt
id	email	name	age	score	active
u_1	a@test.com	Alice	30	98.5	true
```

Internal engine fields do not belong in `.ten` rows. They live in
`tensack.toml` and `.btf` state:

```txt
tx/log position       monotonically increasing transaction state
op/delete markers     operation log state
sync state            local/remote replication state
lookup state          id and declared lookup indexes
```

User fields cannot start with `_`.

This creates a hard separation:

```txt
user fields      normal names in .ten
engine fields    internal metadata / .btf
```

---

## 5. Workspace Structure

Use a Rust workspace.

```txt
tensack/
  Cargo.toml

  crates/
    tensack-core/
      src/
        lib.rs
        value.rs
        ids.rs
        error.rs
        row.rs
        ptr.rs
        registry.rs
        table_spec.rs

    tensack-schema/
      src/
        lib.rs
        macro_input.rs
        validate.rs
        ir.rs
        names.rs

    tensack-codegen/
      src/
        lib.rs
        rust.rs
        typescript.rs
        go.rs
        python.rs
        manifest.rs

    tensack-engine/
      src/
        lib.rs
        db.rs
        table.rs
        chunk.rs
        writer.rs
        reader.rs
        lookup.rs
        wal.rs
        compact.rs
        manifest.rs
        lock.rs
        repair.rs

    tensack-cli/
      src/
        main.rs
        check.rs
        build.rs
        repair.rs
        compact.rs
        inspect.rs

  examples/
    chat/
      schema.tensack
      build.rs
      src/
        main.rs
```

Important boundaries:

```txt
tensack-schema   defines schema macro input, validation, and IR
tensack-codegen  generates Rust schema and registry types
tensack-core     shared primitive definitions and typed handles
tensack-engine   runtime storage engine
tensack-cli      human tools
```

Do **not** put schema expansion or validation inside the storage engine.

Do **not** make SDKs depend on storage internals.

Do **not** let every language invent its own data model.

Everything comes from the same IR.

---

## 6. Compile-Time Path

An app has:

```txt
examples/chat/
  schema.tensack
  build.rs
  build_schema.rs
  src/main.rs
```

`schema.tensack` is included by a real Rust schema script:

```rust
use tensack::schema;

include!("schema.tensack");

fn main() {
    let schema = database_schema();
    tensack_codegen::generate_rust(schema, "src/generated/schema.rs").unwrap();
}
```

`build.rs` reruns that script before Rust compilation:

```rust
fn main() {
    println!("cargo:rerun-if-changed=schema.tensack");
    println!("cargo:rerun-if-changed=build_schema.rs");
    tensack_codegen::run_schema_script("build_schema.rs").unwrap();
}
```

Then `main.rs` includes generated code:

```rust
mod generated {
    pub mod schema;
}

use generated::schema::*;
```

If the schema is invalid, the Rust build fails.

Compile-time failures should include:

```txt
duplicate table name
duplicate field name
lookup references missing field
lookup field has unsupported type
reserved field name like _tx
table has no id field
field type is unknown
invalid table name
invalid field name
invalid lookup name
unsupported schema syntax
```

This gives the desired effect:

```txt
user edits simple schema
build process parses schema
generated Rust registry is produced
Rust compiler enforces typed usage
bad schema fails before runtime
```

---

## 7. Canonical Schema IR

Everything should compile through one tiny intermediate representation.

```rust
pub struct SchemaIr {
    pub version: u32,
    pub tables: Vec<TableIr>,
}

pub struct TableIr {
    pub name: String,
    pub fields: Vec<FieldIr>,
    pub lookups: Vec<LookupIr>,
}

pub struct FieldIr {
    pub name: String,
    pub ty: SackType,
    pub required: bool,
}

pub struct LookupIr {
    pub name: String,
    pub fields: Vec<String>,
    pub unique: bool,
}
```

This IR is the source for every generated SDK.

Generate this file too:

```txt
.tensack/schema.ir.json
```

Example:

```json
{
  "version": 1,
  "tables": [
    {
      "name": "users",
      "fields": [
        {
          "name": "id",
          "type": "id",
          "required": true
        },
        {
          "name": "email",
          "type": "text",
          "required": true
        },
        {
          "name": "age",
          "type": "int",
          "required": true
        }
      ],
      "lookups": [
        {
          "name": "email",
          "fields": ["email"],
          "unique": true
        }
      ]
    }
  ]
}
```

Rust generated code should come from this file.

That prevents SDK drift.

---

## 8. Generated Rust Registry

For this schema:

```rust
users {
  id: id
  email: text
  name: text
  age: int

  lookup email unique
}
```

Generate this Rust:

```rust
#[derive(Debug, Clone)]
pub struct User {
    pub id: String,
    pub email: String,
    pub name: String,
    pub age: i64,
}

#[derive(Debug, Clone)]
pub struct NewUser {
    pub id: String,
    pub email: String,
    pub name: String,
    pub age: i64,
}

pub struct Users;

impl tensack_core::TableSpec for Users {
    const NAME: &'static str = "users";
    type Row = User;
    type NewRow = NewUser;
}

pub mod users_fields {
    pub const ID: tensack_core::Field<super::Users, String> =
        tensack_core::Field::new("id");

    pub const EMAIL: tensack_core::Field<super::Users, String> =
        tensack_core::Field::new("email");

    pub const NAME: tensack_core::Field<super::Users, String> =
        tensack_core::Field::new("name");

    pub const AGE: tensack_core::Field<super::Users, i64> =
        tensack_core::Field::new("age");
}

pub mod users_lookups {
    pub const ID: tensack_core::Lookup<super::Users, String> =
        tensack_core::Lookup::new("id");

    pub const EMAIL: tensack_core::Lookup<super::Users, String> =
        tensack_core::Lookup::new("email");
}
```

Now this is valid:

```rust
let user = db.users().get_by(users_lookups::EMAIL, "a@test.com")?;
```

This should fail to compile:

```rust
db.messages().get_by(users_lookups::EMAIL, "a@test.com")?;
```

Because `users_lookups::EMAIL` is typed as:

```rust
Lookup<Users, String>
```

not:

```rust
Lookup<Messages, String>
```

That is the registry doing real work.

---

## 9. Core Traits

In `tensack-core`:

```rust
pub trait TableSpec {
    const NAME: &'static str;
    type Row;
    type NewRow;
}
```

Field marker:

```rust
pub struct Field<TTable, TValue> {
    pub name: &'static str,
    _table: std::marker::PhantomData<TTable>,
    _value: std::marker::PhantomData<TValue>,
}

impl<TTable, TValue> Field<TTable, TValue> {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            _table: std::marker::PhantomData,
            _value: std::marker::PhantomData,
        }
    }
}
```

Lookup marker:

```rust
pub struct Lookup<TTable, TKey> {
    pub name: &'static str,
    _table: std::marker::PhantomData<TTable>,
    _key: std::marker::PhantomData<TKey>,
}

impl<TTable, TKey> Lookup<TTable, TKey> {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            _table: std::marker::PhantomData,
            _key: std::marker::PhantomData,
        }
    }
}
```

This gives compile-time table/field/lookup pairing without exposing complexity to the user.

---

## 10. Runtime Registry

Generated code also exposes a runtime static registry.

```rust
pub static REGISTRY: tensack_core::Registry = tensack_core::Registry {
    tables: &[
        tensack_core::TableDef {
            id: 1,
            name: "users",
            fields: &[
                tensack_core::FieldDef {
                    name: "id",
                    ty: tensack_core::SackType::Id,
                    required: true,
                },
                tensack_core::FieldDef {
                    name: "email",
                    ty: tensack_core::SackType::Text,
                    required: true,
                },
                tensack_core::FieldDef {
                    name: "age",
                    ty: tensack_core::SackType::Int,
                    required: true,
                },
            ],
            lookups: &[
                tensack_core::LookupDef {
                    name: "id",
                    fields: &["id"],
                    unique: true,
                },
                tensack_core::LookupDef {
                    name: "email",
                    fields: &["email"],
                    unique: true,
                },
            ],
        },
    ],
};
```

The engine opens with the registry:

```rust
let db = tensack_engine::Db::open("./data", &REGISTRY)?;
```

On startup, the engine checks:

```txt
tensack.toml schema hash matches generated schema hash
all table folders exist
all .btf index files exist or can be rebuilt
active.ten is valid up to final full line
all row values match declared primitive types
unique lookups have no duplicates
```

---

## 11. Storage Folder Mapping

Given:

```rust
users {
  id: id
  email: text
  name: text
  age: int

  lookup email unique
}
```

Create:

```txt
my-chat.tensack/
  schema.tensack
  tensack.toml
  tables/
    users/
      active.ten
      0000.ten
      0001.ten
  engine/
    users.btf
```

Every table gets an implicit `id` lookup.

So this:

```rust
lookup email unique
```

creates:

```txt
engine/users.btf
```

The lookup is not required to be a visible text file. It can live inside the
table's binary Tensack file (`.btf`) as rebuildable index state.

The implicit id lookup creates:

```txt
engine/users.btf
```

The `.ten` files are the human-readable data source of truth.

Lookup files are acceleration structures and must be rebuildable.

---

## 12. Row Pointer Model

Every lookup points to a physical row location.

```rust
pub struct RowPtr {
    pub table_id: u16,
    pub chunk_id: u32,
    pub offset: u64,
    pub len: u32,
    pub tx: u64,
}
```

Example lookup entry conceptually:

```txt
email:a@test.com -> users/0000.ten offset=0 len=36 tx=1
```

For v1, lookup state can be simple internally, but it should not become part of
the normal user-visible table data:

```txt
a@test.com  0000.ten  0   36  1
b@test.com  0000.ten  37  34  2
```

For v2, lookup becomes binary:

```txt
hash(key) -> RowPtr
```

Important rule:

```txt
lookups are rebuildable
.ten row segments are the source of truth
```

If a lookup gets corrupted, rebuild it from chunks.

---

## 13. Write Path

Inserting a user:

```rust
let user = NewUser {
    id: "u_1".to_string(),
    email: "a@test.com".to_string(),
    name: "Alice".to_string(),
    age: 30,
};

db.users().insert(user)?;
```

Engine flow:

```txt
1. Validate table exists in registry
2. Validate all required fields exist
3. Validate primitive types
4. Validate unique lookup constraints
5. Serialize one .ten row in physical header order
6. Append to active.ten
7. fsync depending on durability mode
8. Update id lookup in .btf
9. Update declared lookup indexes in .btf
10. Update tensack.toml/checkpoint state when needed
```

Stored line:

```txt
1	put	u_1	a@test.com	Alice	30
```

The append operation must preserve:

```txt
one logical row = one physical line
newline terminates committed row
broken final line can be dropped during recovery
```

---

## 14. Read Path

Point read by id:

```rust
let user = db.users().get("u_1")?;
```

Engine flow:

```txt
1. Use users.btf id lookup
2. Get RowPtr
3. Seek to byte offset
4. Read exact line
5. Split TSV-style fields
6. Decode into User
```

Lookup read:

```rust
let user = db.users().get_by(users_lookups::EMAIL, "a@test.com")?;
```

Engine flow:

```txt
1. Use users.btf email lookup
2. Get RowPtr
3. Seek into .ten segment
4. Read line
5. Decode User
```

No full file scan is required for lookup-backed reads.

---

## 15. Update Path

Do not rewrite rows in place.

Append a full replacement in v1.

```rust
db.users().put(User {
    id: "u_1".to_string(),
    email: "new@test.com".to_string(),
    name: "Alice".to_string(),
    age: 31,
})?;
```

Stored in `.ten`:

```txt
id	email	name	age
u_1	a@test.com	Alice	30
u_1	new@test.com	Alice	31
```

Internal lookup/log state now points to tx `2`.

Old row stays until compaction.

For v1, avoid patch format. Full replacement is simpler, easier to type-check, and easier for SDKs.

Possible later patch format belongs in the internal operation log, not in the
readable `.ten` table segment:

```txt
patch id=u_1 set.age=32
```

Do not start there.

---

## 16. Delete Path

Delete is a tombstone.

```rust
db.users().delete("u_1")?;
```

Stored internally:

```txt
delete id=u_1 tx=4
```

Engine behavior:

```txt
append/delete marker in internal log state
remove id lookup entry
remove secondary lookup entries
keep old rows until compaction
```

Compaction physically removes old rows.

---

## 17. Compaction

Before compaction, the readable `.ten` file may contain superseded rows:

```txt
id	email	name	age
u_1	a@test.com	Alice	30
u_1	new@test.com	Alice	31
u_2	b@test.com	Bob	41
```

After compaction:

```txt
id	email	name	age
u_1	new@test.com	Alice	31
```

Compaction writes a new chunk:

```txt
tables/users/0003.tmp
```

Then atomically renames:

```txt
0003.tmp -> 0003.ten
```

Then rebuilds lookups.

Compaction rules:

```txt
never mutate existing sealed chunks directly
write new tmp files
fsync tmp file
rename atomically
fsync containing directory if supported
update manifest
rebuild lookup tables
delete old chunks only after manifest points to new chunks
```

---

## 18. Workspace Metadata

`tensack.toml` tracks compact readable engine state:

```toml
version = 1
schema_hash = "abc123"
next_tx = 128

[tables.users]
id = 1
path = "tables/users"
active = "active.ten"
segments = ["0000.ten", "0001.ten"]
header = "id\temail\tname\tage"

[tables.users.index]
state = "ready"
file = "engine/users.btf"
```

On open:

```txt
read tensack.toml
compare schema hash
open registry
verify table folders
load/rebuild .btf indexes
recover active.ten if needed
```

`tensack.toml` is not the source of data truth and not a second schema.

It is the readable source of engine layout/checkpoint truth.

---

## 19. Generated Rust API

From the same IR, generate Rust types and registry modules. Keep generated code downstream of the macro declarations in `schema.tensack` so runtime code does not hand-model schema shapes.

Generate Go:

```go
type User struct {
    Id     string  `json:"id"`
    Email  string  `json:"email"`
    Name   string  `json:"name"`
    Age    int64   `json:"age"`
    Score  float64 `json:"score"`
    Active bool    `json:"active"`
}
```

Generate Rust:

```rust
pub struct User {
    pub id: String,
    pub email: String,
    pub name: String,
    pub age: i64,
    pub score: f64,
    pub active: bool,
}
```

The SDKs do not define the database.

They mirror the schema.

The schema IR defines everything.

---

## 20. Rust Table API Shape

Generated extension methods:

```rust
impl tensack_engine::Db {
    pub fn users(&self) -> tensack_engine::TableHandle<Users> {
        self.table::<Users>()
    }

    pub fn messages(&self) -> tensack_engine::TableHandle<Messages> {
        self.table::<Messages>()
    }
}
```

Generic engine API:

```rust
impl<T: TableSpec> TableHandle<T> {
    pub fn insert(&self, row: T::NewRow) -> Result<T::Row>;

    pub fn put(&self, row: T::Row) -> Result<T::Row>;

    pub fn get(&self, id: &str) -> Result<Option<T::Row>>;

    pub fn delete(&self, id: &str) -> Result<()>;
}
```

Lookup API:

```rust
impl<T: TableSpec> TableHandle<T> {
    pub fn get_by<K>(
        &self,
        lookup: Lookup<T, K>,
        key: K,
    ) -> Result<Option<T::Row>>;
}
```

Clean usage:

```rust
let user = db.users().get("u_1")?;

let user = db
    .users()
    .get_by(users_lookups::EMAIL, "a@test.com".to_string())?;
```

Bad usage should fail:

```rust
db.messages().get_by(users_lookups::EMAIL, "a@test.com".to_string())?;
```

Because the lookup belongs to `Users`, not `Messages`.

---

## 21. Dynamic Core, Typed Shell

Internally, the engine should use dynamic values:

```rust
pub enum Value {
    Id(String),
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}
```

Rows internally:

```rust
pub struct DynamicRow {
    pub table: &'static str,
    pub fields: std::collections::BTreeMap<String, Value>,
}
```

But generated SDKs expose typed structs:

```rust
pub struct User {
    pub id: String,
    pub email: String,
    pub age: i64,
}
```

Correct split:

```txt
inside engine: dynamic Value map
outside engine: generated typed structs
```

This keeps the engine reusable while making application code type-safe.

---

## 22. Required Schema Validation

The schema compiler should validate:

```txt
table names are lowercase snake_case
field names are lowercase snake_case
every table has id: id
id field is first or automatically normalized first
no reserved fields: _tx, _op, _deleted
only known primitive types
lookup fields exist
lookup fields are primitive scalar fields
unique lookups use one field in v1
no duplicate lookup names
no duplicate table names
no duplicate field names
no unknown declarations
no invalid characters in names
no user field starts with _
```

Recommended v1 naming rule:

```txt
table:  [a-z][a-z0-9_]* plural preferred but not enforced
field:  [a-z][a-z0-9_]*
lookup: derived from field name
```

---

## 23. Required Runtime Validation

The runtime should validate:

```txt
row matches table schema
all required fields exist
no unknown fields unless loose mode is enabled
int is actually i64
float is finite f64
bool is bool
text is valid UTF-8
id is not empty
unique lookup does not collide
id lookup does not collide
lookup sidecars point to valid row positions
row _tx is monotonic
row _op is known
```

Ban these in v1:

```txt
NaN
Infinity
-Infinity
null
arrays
objects inside user fields
```

Reason:

```txt
JSON does not safely represent NaN or Infinity
null creates optional field semantics too early
nested objects create schema and SDK complexity too early
```

---

## 24. Features to Avoid in Version 1

Do not start with:

```txt
optional fields
arrays
nested objects
foreign keys
joins
multi-field lookups
computed fields
schema migrations
custom validators
enums
dates
decimal
null
JSON blobs
compression
columnar storage
query planner
multi-process high-concurrency writes
complex transaction isolation
```

The v1 goal is:

```txt
basic typed tables
human-readable JSONL
lookup tables
compile-time generated registry
simple SDK generation
safe rebuild/repair
```

That is already enough.

---

## 25. Migration Strategy Without Migration History

The clean model:

```txt
schema.tensack changes
↓
tensack check detects mismatch
↓
user runs one explicit upgrade function
↓
engine rewrites affected chunks
↓
old migration code is not kept
```

Generated schema has a hash:

```rust
pub const SCHEMA_HASH: &str = "abc123";
```

Manifest stores old hash:

```json
{
  "schema_hash": "old456"
}
```

If mismatched:

```txt
open fails in normal mode
repair/upgrade mode can rewrite
```

No infinite migration folder is required.

For v1, support only:

```txt
add field with default
remove field
rename field manually through upgrade script
rebuild lookup
```

Do not overbuild this in the first implementation.

---

## 26. CLI Commands

The CLI should be boring and direct.

```txt
tensack check
tensack build
tensack repair
tensack compact
tensack inspect
tensack rebuild-lookups
```

### `tensack check`

Checks:

```txt
schema parses
schema validates
manifest exists
schema hash matches manifest
JSONL rows are valid
lookup files are valid or rebuildable
```

### `tensack build`

Generates:

```txt
.tensack/schema.ir.json
src/generated/schema.rs
sdk/go/schema.go
```

### `tensack repair`

Can:

```txt
drop broken final line
rebuild lookup tables
recompute manifest tx if safe
validate chunks
rewrite clean current file
```

### `tensack compact`

Can:

```txt
merge old rows
remove tombstoned rows
seal chunks
rebuild lookup files
```

### `tensack inspect`

Displays:

```txt
tables
fields
lookup definitions
chunk count
row count estimate
current tx
manifest schema hash
```

---

## 27. Durability Modes

Add simple durability modes later, but design for them now.

```txt
fast
  append buffered
  fsync occasionally
  highest speed
  can lose recent writes on crash

safe
  fsync active.ten after batch
  good default

paranoid
  fsync row or tiny batch
  slower but safer
```

Do not expose too much at first.

Default should be:

```txt
safe
```

---

## 28. Locking Model

Start with:

```txt
one writer
many readers
```

Use a process lock:

```txt
data/.tensack.lock
```

Rules:

```txt
writer must hold lock
readers can read sealed chunks and lookup snapshots
compaction requires writer lock
lookup rebuild requires writer lock
```

Do not attempt complex multi-writer concurrency in v1.

---

## 29. Lookup Table Strategy

Lookup state is engine-owned. It can begin simple internally, but the normal file
contract should treat it as `.btf`, not user data.

Example:

```txt
a@test.com	0000.ten	0	93	1
b@test.com	0000.ten	94	88	2
```

The durable target is binary `.btf`.

Conceptually:

```txt
hash(key) -> RowPtr
```

Potential binary row pointer:

```rust
pub struct RowPtr {
    pub table_id: u16,
    pub chunk_id: u32,
    pub offset: u64,
    pub len: u32,
    pub tx: u64,
}
```

Lookup principles:

```txt
lookup files are not source of truth
lookup files are always rebuildable
unique lookup detects duplicate keys
id lookup is implicit for every table
declared lookups are explicit in schema.tensack
```

Use the product word:

```txt
lookup
```

Avoid user-facing language like:

```txt
index
B-tree
secondary index
```

The implementation can use fast structures internally later, but the product model should remain lookup-oriented.

---

## 30. File Format Rules

Canonical data format:

```txt
chunked .ten row segments
```

Rules:

```txt
one row per line
header line names the schema fields in order
each data line is tab-separated text
newline means committed readable row
no in-place row mutation
append puts
record deletes, logs, and sync state internally
compact later
```

Example:

```txt
id	email	name	age
u_1	a@test.com	Alice	30
u_2	b@test.com	Bob	41
```

Avoid for hot storage:

```txt
YAML
pretty JSON arrays
CSV
Markdown
TOML tables
```

Use TOML or JSON only for config/metadata if needed.

---

## 31. Hot File Layout

Recommended v1 layout:

```txt
my-chat.tensack/
  schema.tensack
  tensack.toml
  .tensack.lock

  tables/
    users/
      active.ten
      0000.ten

    messages/
      active.ten
      0000.ten

  engine/
    users.btf
    messages.btf
    cache.btf
```

Sealed `.ten` segment names are four digits: `0000.ten` through `9999.ten`.
Do not put more than 10,000 sealed segments in one folder. If a table grows past
`9999.ten`, start a new group folder such as `g0002/` and begin again at
`0000.ten`.

Later optional analytics sidecar:

```txt
my-chat.tensack/
  analytics/
    messages.parquet
    candles.parquet
```

But this is a sidecar, not the canonical hot store.

---

## 32. Workspace Metadata

Use one root metadata file named `tensack.toml`.

`tensack.toml` is not schema truth. It is a compact physical map for fast open,
repair, and debugging.

Example:

```toml
version = 1
schema_hash = "abc123"
next_tx = 12500001

[tables.users]
id = 1
path = "tables/users"
active = "active.ten"
segments = ["0000.ten"]
header = "id\temail\tname\tcreated_at"

[tables.users.index]
state = "ready"
file = "engine/users.btf"
```

This is operational metadata. The engine can rebuild much of it from
`schema.tensack` and `.ten` files if needed.

---

## 33. Schema Hash

Every generated registry should include:

```rust
pub const SCHEMA_HASH: &str = "abc123";
```

The manifest should include the same hash.

```json
{
  "schema_hash": "abc123"
}
```

On database open:

```txt
if manifest.schema_hash == generated.SCHEMA_HASH:
  open normally

if mismatch:
  fail in normal mode
  require explicit upgrade/repair command
```

This prevents silently reading old files with new types.

---

## 34. Minimal Schema Macro Surface

The v1 macro surface can be very small.

Informal shape:

```txt
schema      = "schema!" "{" table* "}"
table       = ident "{" item* "}"
item        = field | lookup
field       = ident type
lookup      = "lookup" ident unique?
unique      = "unique"
type        = "id" | "text" | "int" | "float" | "bool"
ident       = [a-z][a-z0-9_]*
```

Example:

```rust
schema! {
  users {
    id id
    email text
    age int

    lookup email unique
  }
}
```

Macro input should become a structured schema model first, then validated IR.

Do not codegen directly from raw macro tokens.

Pipeline:

```txt
macro tokens -> schema model -> validated IR -> generated code
```

---

## 35. AST vs IR

The macro input model represents exactly what the user wrote.

```rust
pub struct SchemaInput {
    pub tables: Vec<TableAst>,
}

pub struct TableAst {
    pub name: String,
    pub items: Vec<TableItemAst>,
}

pub enum TableItemAst {
    Field(FieldAst),
    Lookup(LookupAst),
}
```

IR represents normalized truth.

```rust
pub struct SchemaIr {
    pub version: u32,
    pub tables: Vec<TableIr>,
}
```

Validation converts AST into IR.

During validation:

```txt
id lookup is added automatically
id field is required
lookup names are normalized
schema hash is computed
table ids are assigned deterministically
```

---

## 36. Deterministic Table IDs

Assign table IDs deterministically from schema order or stable hash.

Simplest v1:

```txt
first table  = 1
second table = 2
third table  = 3
```

But this changes IDs if tables are reordered.

Better:

```txt
table_id = stable hash of table name truncated to u16/u32
```

For v1, either is fine if schema changes require explicit rebuild.

Recommendation:

```txt
Use deterministic hash-based table IDs internally.
Keep table name as the human truth.
```

---

## 37. Error Model

Define one central error type in `tensack-core` or `tensack-engine`.

```rust
pub enum TensackError {
    Schema(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    TypeMismatch {
        table: String,
        field: String,
        expected: SackType,
        found: String,
    },
    MissingField {
        table: String,
        field: String,
    },
    DuplicateKey {
        table: String,
        lookup: String,
        key: String,
    },
    NotFound {
        table: String,
        id: String,
    },
    SchemaHashMismatch {
        expected: String,
        found: String,
    },
    CorruptLookup(String),
    CorruptRow(String),
}
```

Keep user-facing errors blunt and actionable:

```txt
users.email duplicate key: a@test.com
messages.conversation_id lookup references missing field
schema hash mismatch: run tensack check or tensack upgrade
```

---

## 38. Minimal Dependencies

For the first Rust implementation, keep dependencies small.

Reasonable dependencies:

```txt
serde
serde_json
thiserror
camino or camino-like path handling, optional
memmap2 later, optional
```

Avoid early:

```txt
sled
rocksdb
lmdb
sqlx
diesel
arrow
parquet
heavy parser frameworks
async runtime dependency in core engine
```

The storage engine should be synchronous first.

Async wrappers can come later.

---

## 39. SDK Boundary

SDKs should not understand storage internals.

They should understand:

```txt
table names
field names
primitive types
lookup names
request/response shapes
```

The storage server/engine understands:

```txt
JSONL files
row pointers
lookup files
manifest
locks
compaction
repair
```

Generated SDKs should never be allowed to define schema independently.

Correct:

```txt
schema.tensack macro declarations -> schema.ir.json -> all SDKs
```

Wrong:

```txt
Rust struct handwritten
Go type handwritten
schema.tensack exists separately
```

That causes drift.

---

## 40. Type Mapping

Public primitive mapping should start with the Rust runtime types. Add other language mappings only if this project intentionally grows non-Rust clients later.

```txt
sack id      Rust String
sack text    Rust String
sack int     Rust i64
sack float   Rust f64
sack bool    Rust bool
```

---

## 41. Example Full Schema

```rust
users {
  id: id
  email: text
  name: text
  age: int
  score: float
  active: bool

  lookup email unique
}

conversations {
  id: id
  user_id: id
  title: text
  created_at: int

  lookup user_id
  lookup created_at
}

messages {
  id: id
  conversation_id: id
  role: text
  body: text
  created_at: int

  lookup conversation_id
  lookup created_at
}
```

Storage result:

```txt
my-chat.tensack/
  schema.tensack
  tensack.toml
  .tensack.lock

  tables/
    users/
      active.ten

    conversations/
      active.ten

    messages/
      active.ten

  engine/
    users.btf
    conversations.btf
    messages.btf
```

---

## 42. Example Generated Rust Usage

```rust
mod generated {
    pub mod schema;
}

use generated::schema::*;

fn main() -> tensack_engine::Result<()> {
    let db = tensack_engine::Db::open("./data", &REGISTRY)?;

    db.users().insert(NewUser {
        id: "u_1".to_string(),
        email: "a@test.com".to_string(),
        name: "Alice".to_string(),
        age: 30,
        score: 98.5,
        active: true,
    })?;

    let user = db.users().get("u_1")?;

    let same_user = db
        .users()
        .get_by(users_lookups::EMAIL, "a@test.com".to_string())?;

    Ok(())
}
```

Invalid usage that should fail compile-time type checks:

```rust
let bad = db
    .messages()
    .get_by(users_lookups::EMAIL, "a@test.com".to_string())?;
```

Reason:

```txt
users_lookups::EMAIL belongs to Users
db.messages() expects Lookup<Messages, K>
```

---

## 43. Minimal Engine API

```rust
pub struct Db {
    root: PathBuf,
    registry: &'static Registry,
}

impl Db {
    pub fn open<P: AsRef<Path>>(
        root: P,
        registry: &'static Registry,
    ) -> Result<Self>;

    pub fn table<T: TableSpec>(&self) -> TableHandle<T>;
}
```

```rust
pub struct TableHandle<T: TableSpec> {
    db: Db,
    _table: PhantomData<T>,
}

impl<T: TableSpec> TableHandle<T> {
    pub fn insert(&self, row: T::NewRow) -> Result<T::Row>;
    pub fn put(&self, row: T::Row) -> Result<T::Row>;
    pub fn get(&self, id: &str) -> Result<Option<T::Row>>;
    pub fn delete(&self, id: &str) -> Result<()>;

    pub fn get_by<K>(
        &self,
        lookup: Lookup<T, K>,
        key: K,
    ) -> Result<Option<T::Row>>;
}
```

---

## 44. Minimum Viable Implementation Order

Build in this order:

```txt
1. schema.tensack macro surface
2. schema validator
3. schema IR JSON output
4. Rust codegen for structs + registry
5. .ten append-only table writer
6. id lookup in .btf
7. get by id
8. declared lookup indexes in .btf
9. delete tombstones
10. compaction
11. repair command
12. inspect command
13. binary .btf index backend
```

Do not start with:

```txt
query language
fancy storage
multi-process concurrency
distributed sync
parquet
compression
migration framework
```

Start with typed append/get/delete.

---

## 45. V1 Cutline

The first real release should include:

```txt
schema.tensack
primitive types: id, text, int, float, bool
schema! table declarations
lookup declarations
unique lookup declarations
generated Rust structs
generated Rust registry
compile-time schema validation through build.rs
.ten table storage
tensack.toml physical layout metadata
implicit id lookup
declared lookup indexes in .btf
insert
put
get
get_by
delete
repair broken final line
rebuild lookups
compact table
tensack.toml schema hash check
```

Do not include more than this unless absolutely necessary.

---

## 46. Final Architecture Summary

The clean backend is:

```txt
schema.tensack
  ↓
Rust schema script include
  ↓
Rust macro/codegen expansion
  ↓
schema IR
  ↓
compile-time generated Rust registry
  ↓
typed table handles
  ↓
.ten row segments as source of truth
  ↓
.btf lookup/index files as rebuildable acceleration
  ↓
SDKs generated from same IR
```

The important design rule:

```txt
User-facing system:
  table, field, lookup, int, float, text, bool, id

Rust backend:
  registry, traits, generated structs, row pointers, manifest, WAL, compaction, lookup rebuilding

SDK system:
  generate everything from schema IR
```

This gives the right balance:

```txt
human-readable storage
compile-time safety
tiny public type system
many SDKs
simple Rust engine
fast enough reads through lookup tables
clean long-term evolution
```

---

## 47. One-Sentence Product Definition

`tensack` is a tiny local database that stores canonical data as human-readable TSV-style row files, uses explicit rebuildable lookup tables for speed, and compiles a small imported `schema.tensack` component into type-safe Rust registries and generated SDKs.

---

## 48. Non-Negotiable Design Rules

```txt
No SQL.
No SQLite dependency.
No RON.
No YAML for hot data.
No pretty JSON arrays for hot data.
No user-facing Rust schema.
No user-facing index terminology.
Use lookup tables in docs and schema.
Keep public types tiny.
Make TSV-style row files the source of truth.
Make lookup files rebuildable.
Generate every SDK from the same IR.
Fail at compile time when schema usage is wrong.
```

---

## 49. Recommended First Engineering Milestone

Build a tiny chat example:

```rust
conversations {
  id: id
  title: text
  created_at: int

  lookup created_at
}

messages {
  id: id
  conversation_id: id
  role: text
  body: text
  created_at: int

  lookup conversation_id
  lookup created_at
}
```

Then implement:

```txt
tensack build
cargo run
insert conversation
insert message
get message by id
get messages by conversation_id
delete message
compact messages
rebuild lookups
```

That proves the whole architecture without overbuilding.

---

## 50. Recommended Later Milestones

After v1 works:

```txt
binary lookup backend
memory-mapped lookup reads
range lookup scans
batch writes
snapshot export
HTTP wrapper
Parquet analytical sidecar
optional compression for sealed chunks
schema upgrade helper
```

Keep these later.

Do not pollute v1 with them.

---

# End of Document
