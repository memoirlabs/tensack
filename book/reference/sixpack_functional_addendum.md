# sixpack addendum: functional interface, chat rows, and hidden lookup mechanics

status: background reference. current decisions live in `sixpack_book.md` and
the focused `sixpack_*_spec.md` files.

this document is an addendum to the existing `sixpack` rust backend architecture. it does not restate the base design. it only adds or changes the parts that matter after tightening the direction:

chunk layout note: old examples in this addendum that mention `active.6` or
`0000.6` are superseded by `sixpack_chunk_naming_spec.md`. current table
chunks use reverse lowercase base-36 paths such as `zz/zzz.6` and
`zz/zzy.6`.

```txt
normal logical tables
functional declarative public interfaces
no sql-shaped public language
no all-caps generated markers
lookup-first reads
.6 TSV-style row segments as source truth
hidden lookup mechanics allowed
parquet later as a derived projection, not the hot lookup layer
```

schema snippets in this addendum use the inner `schema!` body shape. a real
`schema.sixpack` file wraps table blocks in one outer `schema! { ... }`, and a
normal rust schema script includes that file for generation.

---

## 1. correction: the public interface is functional, not command-shaped

the user-facing database interface should be made out of values, builders, and composable read/change descriptions.

avoid public interfaces shaped like this:

```rust
db.messages().get_by(messages_lookups::conversation_id, cv)
db.messages().where_eq("conversation_id", cv)
db.messages().select(...)
db.query("...")
```

prefer interfaces shaped like this:

```rust
let conversation_messages = messages::by::conversation_id(cv.clone())
    .ordered(messages::field::created_at().asc())
    .take(200);

let rows = db.read(conversation_messages)?;
```

for writes, prefer immutable change descriptions:

```rust
let change = change()
    .save(conversations::row(NewConversation {
        id: cv.clone(),
        user_id: user.clone(),
        title: "database design".to_string(),
        created_at: now,
        updated_at: now,
        archived: false,
    }))
    .save(messages::row(NewMessage {
        id: msg.clone(),
        conversation_id: cv.clone(),
        role: "user".to_string(),
        body: "build the thing".to_string(),
        created_at: now,
    }));

db.apply(change)?;
```

the storage engine still performs a mutation internally, but the public interface is a declarative value:

```txt
describe the desired change
hand it to the engine
engine validates and applies it
```

do not expose a string query grammar. do not make users build text expressions. do not make table access feel like a database console language.

---

## 2. naming rule: generated public markers should be lowercase functions

the earlier generated examples used uppercase constants because that is common rust style for constants. do not do that for the public sixpack interface.

instead of:

```rust
users_lookups::email
messages_fields::created_at
```

use generated modules with lowercase functions:

```rust
users::lookup::email()
users::field::email()

messages::lookup::conversation_id()
messages::field::created_at()
```

better still, expose the most common lookup paths through `by` modules:

```rust
users::by::email("a@test.com".to_string()).one()

messages::by::conversation_id(cv.clone())
    .ordered(messages::field::created_at().asc())
    .many()
```

this keeps the public style visually consistent:

```txt
table::by::lookup(value)
table::field::field_name()
read_plan.ordered(field.asc())
read_plan.take(n)
db.read(plan)
```

no all-caps generated symbols are needed.

---

## 3. unique and many lookups should be different types

the base spec has one lookup marker. keep the schema syntax simple, but split the generated runtime types.

schema stays:

```rust
users {
  id: id
  email: text
  name: text

  lookup email unique
}

messages {
  id: id
  conversation_id: id
  body: text
  created_at: int

  lookup conversation_id
  lookup created_at
}
```

generated rust should distinguish:

```rust
pub struct one_lookup<table, key> {
    name: &'static str,
    _table: core::marker::PhantomData<table>,
    _key: core::marker::PhantomData<key>,
}

pub struct many_lookup<table, key> {
    name: &'static str,
    _table: core::marker::PhantomData<table>,
    _key: core::marker::PhantomData<key>,
}
```

public read plans:

```rust
users::by::email("a@test.com".to_string()).one()
messages::by::conversation_id(cv.clone()).many()
```

expected return shapes:

```rust
db.read(users::by::email(email).one())        // result<option<user>>
db.read(messages::by::conversation_id(cv))    // result<vec<message>>
```

the public user never has to think about `one_lookup` or `many_lookup` unless they are using the lower-level generated rust layer.

---

## 4. chat should remain row-based

do not physically store one file per conversation in the first version.

use normal rows:

```rust
conversations {
  id: id
  user_id: id
  title: text
  created_at: int
  updated_at: int
  archived: bool

  lookup user_id
  lookup updated_at
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

opening a conversation should be powered by:

```rust
messages::by::conversation_id(cv)
    .ordered(messages::field::created_at().asc())
```

not by walking folders.

the reason is simple:

```txt
one file per conversation:
  good for opening exactly one conversation
  bad for huge file counts
  bad for backup/sync tools
  bad for cross-conversation reads
  not the normal mental model

row table plus lookup:
  normal logical model
  few files
  fast enough for personal/couple-user databases
  easy to compact
  easy to rebuild
  easy for every sdk to understand
```

the lookup table is the optimization, not the logical model.

---

## 5. richer chat data model without nested objects

v1 should not introduce nested json, arrays, optional fields, or enum syntax. model extra chat data as normal tables.

recommended additive chat tables:

```rust
message_revisions {
  id: id
  message_id: id
  body: text
  created_at: int

  lookup message_id
  lookup created_at
}

tool_calls {
  id: id
  conversation_id: id
  message_id: id
  name: text
  status: text
  started_at: int
  completed_at: int
  args_blob_id: id
  result_blob_id: id

  lookup conversation_id
  lookup message_id
  lookup status
  lookup started_at
}

artifacts {
  id: id
  conversation_id: id
  message_id: id
  name: text
  mime: text
  blob_id: id
  created_at: int

  lookup conversation_id
  lookup message_id
  lookup created_at
}

blobs {
  id: id
  hash: text
  path: text
  mime: text
  size: int
  created_at: int

  lookup hash unique
  lookup created_at
}

memories {
  id: id
  user_id: id
  kind: text
  body: text
  confidence: float
  created_at: int
  updated_at: int

  lookup user_id
  lookup kind
  lookup updated_at
}
```

why this shape works:

```txt
messages stay simple
edits go into message_revisions
tool payloads do not bloat message rows
artifact bytes live in blobs
memory is searchable by user and kind
everything is still primitive scalar fields
every sdk can generate the same shape
```

when multimodal content becomes necessary, do not turn `messages.body` into nested json first. add a table:

```rust
message_parts {
  id: id
  message_id: id
  position: int
  kind: text
  text: text
  blob_id: id

  lookup message_id
  lookup kind
}
```

then a message can have:

```txt
text part
image blob part
file blob part
tool reference part
```

without changing the primitive v1 type system.

---

## 6. lookup files should be append logs plus compact snapshots

a lookup should not require rewriting the whole lookup file on every row write.

use two files per lookup:

```txt
data/lookups/messages.conversation_id.snapshot
data/lookups/messages.conversation_id.log
```

the snapshot is the compacted current state.

the log is recent changes.

on open:

```txt
load snapshot
replay log
build memory map
```

on compaction:

```txt
write new snapshot tmp
fsync
rename into place
truncate lookup log
```

this keeps writes cheap and startup fast.

---

## 7. lookup record shape

lookup files are engine-owned. The durable target is `.btf`, but the logical
record shape should stay simple enough to inspect with engine tools.

use base64url for keys and ids so tabs/newlines/unusual characters never break parsing.

line format:

```txt
op	key_b64	id_b64	table_id	chunk_id	chunk_name	offset	len	tx
```

example:

```txt
set	Y3ZfMQ	bXNnXzE	50120	0	0000.6	0	117	1
set	Y3ZfMQ	bXNnXzI	50120	0	0000.6	118	121	2
del	Y3ZfMQ	bXNnXzI	50120	0	0000.6	118	121	3
```

meaning:

```txt
set cv_1 msg_1 -> row pointer
set cv_1 msg_2 -> row pointer
remove cv_1 msg_2 from this lookup
```

for a unique lookup:

```txt
set	YUB0ZXN0LmNvbQ	dV8x	10492	0	0000.6	0	83	1
```

the third column is still the row id. even unique lookups should include it because updates need to remove the old key for that row.

---

## 8. lookup memory state

on database open, each lookup becomes one of two memory forms.

unique lookup:

```rust
hash_map<key, row_ptr>
```

many lookup:

```rust
hash_map<key, ordered_map<id, row_ptr>>
```

conceptually:

```txt
users.email:
  "a@test.com" -> ptr(u_1)

messages.conversation_id:
  "cv_1" -> {
    "msg_1" -> ptr(msg_1)
    "msg_2" -> ptr(msg_2)
    "msg_3" -> ptr(msg_3)
  }
```

the inner map must be keyed by row id, not just a list, because updates and deletes target a specific row.

for a personal database, this is acceptable:

```txt
100k messages:
  lookup memory probably tens of mb

1m messages:
  lookup memory may become hundreds of mb

later:
  binary lookup plus mmap-backed loading
```

the v1 goal is correctness and simplicity. `.6` stays readable, while binary
lookup/index/log state can mature behind `.btf` files after the functional
interface is proven.

---

## 9. update behavior for secondary lookups

when replacing a row, the engine must remove old lookup entries and add new ones.

old row in `.6`:

```txt
id	email	name
u_1	old@test.com	alice
```

replacement row in `.6`:

```txt
u_1	new@test.com	alice
```

lookup operations:

```txt
del	b2xkQHRlc3QuY29t	dV8x	10492	0	0000.6	0	72	2
set	bmV3QHRlc3QuY29t	dV8x	10492	0	0000.6	73	73	2
```

the engine needs the previous live row for `u_1` before appending the replacement. that previous row tells the engine which secondary lookup keys to remove.

for delete, the marker belongs in internal engine state:

```txt
delete id=u_1 tx=3
```

lookup operations:

```txt
del	bmV3QHRlc3QuY29t	dV8x	10492	0	0000.6	73	73	3
del	dV8x	dV8x	10492	0	0000.6	73	73	3
```

the implicit id lookup is also updated.

---

## 10. stale pointer protection

lookup entries are acceleration data. they should never be trusted blindly.

after a lookup returns a row pointer, the read path must verify:

```txt
row id matches expected id
row is still live according to internal engine state
row transaction equals pointer transaction
row table matches expected table
lookup field still equals requested key
```

if verification fails:

```txt
ignore that pointer
mark lookup dirty
allow repair to rebuild it
```

this protects against:

```txt
partial lookup update
stale pointer after crash
manual lookup file edit
buggy compaction
renamed chunk without rebuilt lookup
```

the canonical `.6` row plus internal live/delete state still decides truth.

---

## 11. chunking strategy for v1

keep chunking boring and predictable.

active file:

```txt
data/tables/<table>/active.6
```

seal thresholds:

```txt
current file reaches 16 mib
or current file reaches 100_000 rows
or explicit compact/seal call
```

sealed files:

```txt
0000.6
0001.6
0002.6
```

chunk id rules:

```txt
0000.6 has chunk id 0
0001.6 has chunk id 1
0002.6 has chunk id 2
```

seal process:

```txt
hold writer lock
flush active.6
fsync active.6 if durability requires it
rename active.6 to next four-digit sealed chunk
create new active.6
rebuild lookup snapshots for that table
write sixpack.toml tmp
rename sixpack.toml tmp into place
release writer lock
```

because row pointers contain segment names, any operation that renames
`active.6` must rebuild lookup snapshots. this is acceptable in v1.

later optimization:

```txt
write rows directly to numbered active chunk
make active.6 a stable manifest value
avoid pointer rewrite during seal
```

do not start there unless seal cost becomes visible.

---

## 12. compaction strategy for personal chat data

v1 compaction is per table.

document-style tables:

```txt
users
conversations
messages
message_revisions
tool_calls
artifacts
blobs
memories
```

all use:

```txt
latest put by id wins
delete removes id
```

compaction process:

```txt
hold writer lock
scan sealed chunks in chunk order
scan current file
build live map by id
drop deleted ids
write compacted tmp jsonl
fsync compacted tmp
rename tmp to next sealed chunk
replace table metadata with compacted chunk plus empty current
rebuild lookup snapshots
clear lookup logs
release writer lock
```

chat messages are not special in v1.

if full historical message edits matter, keep them in `message_revisions`. do not make the `messages` table itself event-sourced yet.

---

## 13. read plans instead of queries

the public interface should treat reads as typed values.

core shapes:

```rust
pub trait read_plan {
    type output;
}

pub struct one<table> {
    // unique lookup plus key
}

pub struct many<table> {
    // many lookup plus key
}

pub struct ordered_many<table> {
    // many lookup plus ordering field
}
```

public usage:

```rust
let plan = messages::by::conversation_id(cv.clone())
    .ordered(messages::field::created_at().asc())
    .take(200);

let rows = db.read(plan)?;
```

another example:

```rust
let maybe_user = db.read(
    users::by::email("a@test.com".to_string())
)?;
```

`users::by::email(...)` knows it is unique, so its output is:

```rust
option<user>
```

`messages::by::conversation_id(...)` knows it is many, so its output is:

```rust
vec<message>
```

no string filters.

no text expression grammar.

no table names passed as raw strings in normal generated use.

---

## 14. declarative change plans

writes should also be values.

core shape:

```rust
let plan = change()
    .save(users::row(NewUser { ... }))
    .save(conversations::row(NewConversation { ... }))
    .remove(messages::id(msg_id));

db.apply(plan)?;
```

recommended generated helpers:

```rust
users::row(new_user)
users::id("u_1".to_string())

messages::row(new_message)
messages::id("msg_1".to_string())
```

the engine can still expose lower-level methods internally, but sdk-level code should be built around `read` and `apply`.

this gives a consistent mental model:

```txt
read(plan)
apply(change)
```

everything else is generated plan construction.

---

## 15. typescript shape

typescript should mirror the same functional style.

```ts
const rows = await db.read(
  messages.by.conversation_id(cv)
    .ordered(messages.field.created_at.asc())
    .take(200)
)
```

unique lookup:

```ts
const user = await db.read(
  users.by.email("a@test.com")
)
```

change plan:

```ts
await db.apply(
  change()
    .save(users.row({
      id: "u_1",
      email: "a@test.com",
      name: "alice",
    }))
    .save(messages.row({
      id: "msg_1",
      conversation_id: "cv_1",
      role: "user",
      body: "hello",
      created_at: Date.now(),
    }))
)
```

avoid generated sdk APIs like:

```ts
db.table("messages").where("conversation_id", "=", cv)
db.query("...")
db.messages.insert(...)
```

the implementation may internally call a process protocol, but the user-facing shape should stay declarative.

---

## 16. cross-language shape

all sdks should expose the same conceptual interface:

```txt
db.read(plan)
db.apply(change)
table.by.lookup(value)
table.field.field_name.asc()
table.row(value)
table.id(value)
```

rust:

```rust
db.read(messages::by::conversation_id(cv).take(200))?;
```

typescript:

```ts
await db.read(messages.by.conversation_id(cv).take(200))
```

python:

```python
db.read(messages.by.conversation_id(cv).take(200))
```

go can be slightly more explicit because of language style:

```go
db.Read(messages.By.ConversationId(cv).Take(200))
```

the names can adapt to language conventions, but the model stays:

```txt
read plan
change plan
lookup path
field path
row constructor
```

the sdk must not invent its own query language.

---

## 17. protocol shape for language sdks

language sdks should send plans to the engine as plain json.

read request:

```json
{
  "kind": "read",
  "table": "messages",
  "lookup": "conversation_id",
  "cardinality": "many",
  "key": "cv_1",
  "order": {
    "field": "created_at",
    "direction": "asc"
  },
  "take": 200
}
```

unique read request:

```json
{
  "kind": "read",
  "table": "users",
  "lookup": "email",
  "cardinality": "one",
  "key": "a@test.com"
}
```

change request:

```json
{
  "kind": "change",
  "steps": [
    {
      "op": "save",
      "table": "users",
      "row": {
        "id": "u_1",
        "email": "a@test.com",
        "name": "alice"
      }
    },
    {
      "op": "remove",
      "table": "messages",
      "id": "msg_1"
    }
  ]
}
```

this json protocol is not the user-facing language. it is the shared engine boundary.

the sdk can generate these requests from typed builders.

---

## 18. ordering without compound lookups

v1 does not need compound lookups.

this schema:

```rust
messages {
  id: id
  conversation_id: id
  body: text
  created_at: int

  lookup conversation_id
  lookup created_at
}
```

supports:

```rust
messages::by::conversation_id(cv)
    .ordered(messages::field::created_at().asc())
```

implementation:

```txt
lookup conversation_id
fetch matching row pointers
read rows
sort in memory by created_at
take requested limit
```

for personal/couple-user databases, this is fine.

later, add compound lookups only if measurements show it is needed:

```rust
lookup conversation_id created_at
```

but do not add this in v1.

---

## 19. rough scaling expectations

for a personal/couple-user chat database:

```txt
10,000 conversations
40 messages each
400,000 messages
1 kb average row
about 400 mb raw jsonl before compaction/compression
```

with lookup-backed reads:

```txt
open one conversation:
  lookup conversation_id
  read maybe 40 row pointers
  seek/read 40 jsonl rows
  sort by created_at

list recent conversations:
  use conversations.updated_at lookup
  read recent conversation rows
```

without lookups:

```txt
open one conversation by scanning messages:
  scan hundreds of mb
```

the lookup is what matters.

one-file-per-conversation is not necessary for this scale. the indexed row model gives the important benefit without creating tens of thousands of tiny files.

---

## 20. parquet plan, later and hidden

parquet should not be a lookup table and should not be canonical storage.

use parquet later as a derived sidecar for broad scans and reports.

possible later layout:

```txt
data/projections/
  messages/
    000001.parquet
    000002.parquet

  tool_calls/
    000001.parquet
```

generated from compacted jsonl:

```txt
messages jsonl chunks -> messages parquet projection
tool_calls jsonl chunks -> tool_calls parquet projection
```

good parquet use cases:

```txt
count messages by day
token totals by model
tool failures by name
artifact sizes by mime
memory confidence distributions
bulk export
offline analysis
```

bad parquet use cases:

```txt
message id to row pointer
conversation id to messages
email to user
recent conversation lookup
```

parquet can be fast and light for scans, but it is the wrong hot point-lookup layer.

keep it behind a feature or separate crate:

```txt
sixpack-parquet
```

core v1 should not require arrow/parquet dependencies.

---

## 21. hidden binary lookup backend, later

once the text lookup design is proven, add a binary backend without changing public APIs.

same public schema:

```rust
lookup email unique
lookup conversation_id
```

same public read:

```rust
db.read(messages::by::conversation_id(cv))?
```

different hidden backend:

```txt
text lookup:
  snapshot + log

binary lookup:
  hash buckets + row pointers
  mmap-friendly
  rebuildable from jsonl
```

conceptual binary exact lookup:

```txt
hash(key) -> bucket -> key compare -> row_ptr
```

conceptual binary many lookup:

```txt
hash(key) -> bucket -> key compare -> row_ptr list
```

the important rule:

```txt
public lookup semantics do not change when backend changes
```

so benchmarks can improve later without making sdk users rewrite code.

---

## 22. small but important engine rules

add these to v1:

```txt
field order in stored json should follow schema order
engine fields come first: _tx, _op
user fields cannot start with _
id lookup is always unique
all lookup writes happen after row append
manifest tx moves only after row and lookup data are durable enough for selected mode
repair can rebuild lookup files entirely from jsonl
reads verify lookup pointers before returning rows
```

the stored row should be stable and boring:

```jsonl
{"_tx":1,"_op":"put","id":"msg_1","conversation_id":"cv_1","role":"user","body":"hello","created_at":1781481600000}
```

stable ordering makes diffs, tests, and repair easier.

---

## 23. suggested public vocabulary

use these words in docs and sdks:

```txt
table
field
lookup
row
read
change
save
remove
one
many
ordered
take
apply
compact
repair
```

avoid these in user-facing interfaces:

```txt
index
query language
planner
join
cursor
statement
```

implementation docs can mention internal lookup backends and row pointers. product docs should mostly say lookup.

---

## 24. revised first engineering milestone

build the same chat example, but prove the functional interface instead of method-style table commands.

schema:

```rust
conversations {
  id: id
  user_id: id
  title: text
  created_at: int
  updated_at: int
  archived: bool

  lookup user_id
  lookup updated_at
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

rust proof:

```rust
let cv = "cv_1".to_string();
let now = 1781481600000;

db.apply(
    change()
        .save(conversations::row(NewConversation {
            id: cv.clone(),
            user_id: "u_1".to_string(),
            title: "storage design".to_string(),
            created_at: now,
            updated_at: now,
            archived: false,
        }))
        .save(messages::row(NewMessage {
            id: "msg_1".to_string(),
            conversation_id: cv.clone(),
            role: "user".to_string(),
            body: "start here".to_string(),
            created_at: now,
        }))
)?;

let rows = db.read(
    messages::by::conversation_id(cv)
        .ordered(messages::field::created_at().asc())
        .take(200)
)?;
```

typescript proof:

```ts
await db.apply(
  change()
    .save(conversations.row({
      id: "cv_1",
      user_id: "u_1",
      title: "storage design",
      created_at: now,
      updated_at: now,
      archived: false,
    }))
    .save(messages.row({
      id: "msg_1",
      conversation_id: "cv_1",
      role: "user",
      body: "start here",
      created_at: now,
    }))
)

const rows = await db.read(
  messages.by.conversation_id("cv_1")
    .ordered(messages.field.created_at.asc())
    .take(200)
)
```

this proves:

```txt
schema.sixpack Rust macro surface
generated registry
functional generated api
jsonl append
lookup writes
lookup-backed read
in-memory sort
change plan application
repairable storage
```

---

## 25. final additive decisions

the base architecture remains right. these additions change the shape in four important ways:

```txt
1. generated public symbols should be lowercase function paths, not all-caps constants

2. public table access should be functional and declarative:
   db.read(plan)
   db.apply(change)

3. lookups should have separate unique and many cardinality internally:
   one lookup returns option row
   many lookup returns row list

4. chat stays row-based:
   conversations table
   messages table
   normal lookup on conversation_id
   extra chat complexity goes into separate primitive-field tables
```

that gives sixpack the intended feel:

```txt
not sql
not orm
not string query language
not imperative table command soup
small schema
generated typed paths
functional plans
jsonl truth
lookup speed
```
