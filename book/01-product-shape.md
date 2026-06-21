# Product Shape

Tensack is a local-first state layer for small apps and tools.

The unit of data is a database directory. The unit of logic is a schema. The
normal user-facing API should be tiny and declarative:

```txt
db.get(selector)
db.watch(selector)
db.write(change)
```

## Mental Model

```txt
schema.tensack
  -> schema compiler
  -> generated selectors and changes
  -> user calls db.get(...) or db.write(...)
  -> runtime builds and validates a plan
  -> store reads/writes local files
```

`get` means "give me the current value once."

`watch` means "keep this selector updated." It is the future subscription
surface and should not be claimed as implemented until it is actually live.

`write` means "apply this declared state change."

## Current Runtime Surface

Current usable Rust runtime API now includes:

```rust
db.init()?;
db.get(selector::id("messages", "m1"))?;
db.get(selector::many("messages", "conversation_id", "cv1"))?;
db.get(selector::all("messages").limit(100))?;
db.get(selector::count("messages"))?;
db.write(change::add(record))?;
db.write(change::set(record))?;
db.write(change::edit_id("messages", "m1", patch))?;
db.write(change::remove_id("messages", "m1"))?;
db.execute_plan(plan)?;
```

Lower-level runtime helpers are implementation details, not product vocabulary.

## Target Generated Surface

Generated schema modules should make selectors and changes read like the user's
own data model:

```rust
db.get(messages::by::id("m1"))?;
db.get(messages::by::conversation_id("cv1"))?;
db.get(messages::all().limit(100))?;
db.get(messages::count())?;

db.write(messages::add(row))?;
db.write(messages::set(row))?;
db.write(messages::edit(messages::key::id("m1"), patch))?;
db.write(messages::remove(messages::key::id("m1")))?;
```

The product decision is not table-handle command soup as the user-facing shape.
The public shape is:

```txt
get current state
watch current state live
write a change
```
