# Product Shape

Tensack is a local-first data layer for small apps and tools.

The unit of data is a database directory. The unit of logic is a schema. The
normal user-facing API is generated from that schema.

## Mental Model

```txt
schema.tensack
  -> schema compiler
  -> generated table API
  -> user calls db.<table>.<operation>()
  -> generated API builds a plan
  -> runtime validates and executes the plan
  -> store reads/writes local files
```

## Current Product Surface

Current usable Rust runtime API:

```rust
db.init()?;
db.insert(&record)?;
db.upsert(&record)?;
db.put(&record)?;
db.patch_by_id("messages", "m1", patch)?;
db.delete_by_id("messages", "m1")?;
db.get("messages", "m1")?;
db.get_by("users", "email", "a@test.com")?;
db.get_many_by("messages", "conversation_id", "cv1")?;
db.scan("messages", Some(100), None)?;
db.count("messages")?;
db.execute_plan(plan)?;
```

This is compatibility/runtime surface, not the final product feel.

## Target Product Surface

The generated API should feel like:

```txt
db.messages.insert(row)
db.messages.upsert(row)
db.messages.patch({ id: "m1" }, { body: "updated" })
db.messages.remove({ id: "m1" })

db.messages.get.id("m1")
db.messages.find.conversation_id("cv1")
db.messages.scan({ limit: 100 })
db.messages.count()
```

Rust may require method-style syntax:

```rust
db.messages().insert(row)?;
db.messages().upsert(row)?;
db.messages().patch(messages::key::id("m1"), patch)?;
db.messages().remove(messages::key::id("m1"))?;

db.messages().get().id("m1")?;
db.messages().find().conversation_id("cv1")?;
db.messages().scan().limit(100).run()?;
db.messages().count()?;
```

The exact Rust syntax can evolve. The product decision is table-first and
lookup-name-first.

