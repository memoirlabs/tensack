Below is the more direct implementation version.

The smallest serious design is:

```txt id="sltwr4"
runtime binary:
  tensack-core
  tensack-engine
  generated/schema.rs

build-time only:
  tensack-schema macro input
  tensack-codegen

not in final binary:
  schema macro expansion
  CLI parser
  SDK generators
  serde_json checker
  repair tooling
```

The final app binary should **not** expand `schema.tensack` at runtime. It should use generated Rust code.

---

# 1. Exact v0 file layout

For this schema:

```rust id="vlpba4"
users {
  id: id
  email: text
  name: text
  age: int
  score: float
  active: bool

  lookup email unique
}
```

The runtime creates:

```txt id="g27tdb"
data/
  manifest.txt

  tables/
    users/
      current.jsonl

  lookups/
    users.id.lookup
    users.email.lookup
```

`current.jsonl`:

```jsonl id="at1fnj"
{"_tx":1,"_op":"put","id":"u_1","email":"a@test.com","name":"Alice","age":30,"score":98.5,"active":true}
{"_tx":2,"_op":"put","id":"u_2","email":"b@test.com","name":"Bob","age":28,"score":91.0,"active":true}
{"_tx":3,"_op":"delete","id":"u_2"}
```

`users.id.lookup`:

```txt id="8vzsmf"
u_1	current.jsonl	0	113	1	put
u_2	current.jsonl	114	110	2	put
u_2	-	0	0	3	delete
```

`users.email.lookup`:

```txt id="tbsr88"
a@test.com	current.jsonl	0	113	1	put
b@test.com	current.jsonl	114	110	2	put
b@test.com	-	0	0	3	delete
```

For v0, lookup files can be simple tab-separated text.

Later, replace them with binary lookup files without changing the public SDK surface.

---

# 2. Rust app surface

This is what usage should look like:

```rust id="7cwrm5"
use generated::schema::{open_tensack, users};

fn main() -> tensack_engine::Result<()> {
    let mut db = open_tensack("./data")?;

    db.users().insert(&users::Row {
        id: "u_1".to_string(),
        email: "a@test.com".to_string(),
        name: "Alice".to_string(),
        age: 30,
        score: 98.5,
        active: true,
    })?;

    let user = db.users().get("u_1")?;

    let by_email = db
        .users()
        .get_by::<users::Email>("a@test.com")?;

    db.users().put(&users::Row {
        id: "u_1".to_string(),
        email: "new@test.com".to_string(),
        name: "Alice Smith".to_string(),
        age: 31,
        score: 99.0,
        active: true,
    })?;

    db.users().delete("u_1")?;

    Ok(())
}
```

This should compile:

```rust id="dq3rtg"
db.users().get_by::<users::Email>("a@test.com")?;
```

This should **not** compile:

```rust id="3op55y"
db.messages().get_by::<users::Email>("a@test.com")?;
```

Because `users::Email` is only a lookup for the `users::Row` type.

---

# 3. Minimal `Cargo.toml`

For smallest binary, keep runtime dependencies at zero.

```toml id="ftgqet"
[package]
name = "tensack_app"
version = "0.1.0"
edition = "2021"

[dependencies]
tensack-engine = { path = "../crates/tensack-engine" }

[build-dependencies]
tensack-codegen = { path = "../crates/tensack-codegen" }

[profile.release]
opt-level = "z"
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
```

Important:

```txt id="w0ur0s"
tensack-codegen is build-time only.
tensack-schema is build-time only.
tensack-engine is runtime.
generated/schema.rs is runtime.
```

So the final binary does **not** include schema macro expansion.

---

# 4. Build script

`build.rs`:

```rust id="7g8ayk"
fn main() {
    println!("cargo:rerun-if-changed=schema.tensack");

    tensack_codegen::generate_rust_from_file(
        "schema.tensack",
        "src/generated/schema.rs",
    )
    .expect("failed to generate tensack schema");
}
```

The codegen step validates:

```txt id="bpnxez"
table names
field names
primitive types
duplicate fields
duplicate tables
lookup fields
unique lookup rules
reserved fields
id field
```

If schema is broken, Rust compilation fails.

---

# 5. Generated schema code

This is the kind of file generated at:

```txt id="f6b2uz"
src/generated/schema.rs
```

For the `users` table:

```rust id="60mptb"
use tensack_engine::{
    Db, LookupEntry, LookupFor, Result, RowCodec, Table,
};

pub const SCHEMA_HASH: &str = "schema_v1_abc123";

pub fn open_tensack(path: impl AsRef<std::path::Path>) -> Result<TensackDb> {
    let inner = Db::open(path, SCHEMA_HASH)?;
    Ok(TensackDb { inner })
}

pub struct TensackDb {
    inner: Db,
}

impl TensackDb {
    pub fn users(&mut self) -> Table<'_, users::Row> {
        self.inner.table::<users::Row>()
    }
}

pub mod users {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    pub struct Row {
        pub id: String,
        pub email: String,
        pub name: String,
        pub age: i64,
        pub score: f64,
        pub active: bool,
    }

    pub struct Email;

    impl LookupFor<Row> for Email {
        const NAME: &'static str = "email";
        const UNIQUE: bool = true;
    }

    impl RowCodec for Row {
        const TABLE: &'static str = "users";

        fn id(&self) -> &str {
            &self.id
        }

        fn encode_put(&self, tx: u64, out: &mut String) {
            out.clear();

            out.push_str("{\"_tx\":");
            tensack_engine::push_u64(out, tx);

            out.push_str(",\"_op\":\"put\"");

            out.push_str(",\"id\":");
            tensack_engine::push_json_str(out, &self.id);

            out.push_str(",\"email\":");
            tensack_engine::push_json_str(out, &self.email);

            out.push_str(",\"name\":");
            tensack_engine::push_json_str(out, &self.name);

            out.push_str(",\"age\":");
            tensack_engine::push_i64(out, self.age);

            out.push_str(",\"score\":");
            tensack_engine::push_f64(out, self.score);

            out.push_str(",\"active\":");
            tensack_engine::push_bool(out, self.active);

            out.push('}');
        }

        fn decode_put_line(line: &str) -> Result<Self> {
            let op = tensack_engine::json_get_string(line, "_op")?;

            if op != "put" {
                return Err(tensack_engine::Error::BadRow);
            }

            Ok(Self {
                id: tensack_engine::json_get_string(line, "id")?,
                email: tensack_engine::json_get_string(line, "email")?,
                name: tensack_engine::json_get_string(line, "name")?,
                age: tensack_engine::json_get_i64(line, "age")?,
                score: tensack_engine::json_get_f64(line, "score")?,
                active: tensack_engine::json_get_bool(line, "active")?,
            })
        }

        fn lookup_values(&self, out: &mut Vec<LookupEntry>) {
            out.clear();

            out.push(LookupEntry {
                name: "email",
                key: self.email.clone(),
                unique: true,
            });
        }
    }
}
```

That generated code is what gives you type safety.

The runtime engine stays generic and tiny.

---

# 6. Minimal engine core

`crates/tensack-engine/src/lib.rs`:

```rust id="sf6oap"
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    BadRow,
    BadJson,
    MissingField,
    BadType,
    BadLookupKey,
    DuplicateId,
    DuplicateLookup,
    SchemaMismatch,
    NotFound,
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

#[derive(Debug, Clone)]
pub struct LookupEntry {
    pub name: &'static str,
    pub key: String,
    pub unique: bool,
}

#[derive(Debug, Clone)]
pub struct RowPtr {
    pub chunk: String,
    pub offset: u64,
    pub len: u32,
    pub tx: u64,
    pub deleted: bool,
}

pub trait RowCodec: Sized {
    const TABLE: &'static str;

    fn id(&self) -> &str;

    fn encode_put(&self, tx: u64, out: &mut String);

    fn decode_put_line(line: &str) -> Result<Self>;

    fn lookup_values(&self, out: &mut Vec<LookupEntry>);
}

pub trait LookupFor<R: RowCodec> {
    const NAME: &'static str;
    const UNIQUE: bool;
}

pub struct Db {
    root: PathBuf,
    schema_hash: &'static str,
    next_tx: u64,
    durable: bool,
}

pub struct Table<'a, R: RowCodec> {
    db: &'a mut Db,
    _row: PhantomData<R>,
}

impl Db {
    pub fn open(path: impl AsRef<Path>, schema_hash: &'static str) -> Result<Self> {
        let root = path.as_ref().to_path_buf();

        fs::create_dir_all(root.join("tables"))?;
        fs::create_dir_all(root.join("lookups"))?;

        let mut db = Self {
            root,
            schema_hash,
            next_tx: 1,
            durable: true,
        };

        db.load_or_create_manifest()?;

        Ok(db)
    }

    pub fn table<R: RowCodec>(&mut self) -> Table<'_, R> {
        Table {
            db: self,
            _row: PhantomData,
        }
    }

    pub fn set_durable(&mut self, durable: bool) {
        self.durable = durable;
    }

    fn alloc_tx(&mut self) -> Result<u64> {
        let tx = self.next_tx;
        self.next_tx += 1;
        self.write_manifest()?;
        Ok(tx)
    }

    fn load_or_create_manifest(&mut self) -> Result<()> {
        let path = self.root.join("manifest.txt");

        if !path.exists() {
            self.write_manifest()?;
            return Ok(());
        }

        let mut s = String::new();
        File::open(&path)?.read_to_string(&mut s)?;

        let mut found_hash = None;
        let mut found_next_tx = None;

        for line in s.lines() {
            if let Some(v) = line.strip_prefix("schema_hash=") {
                found_hash = Some(v.trim().to_string());
            }

            if let Some(v) = line.strip_prefix("next_tx=") {
                found_next_tx = v.trim().parse::<u64>().ok();
            }
        }

        if found_hash.as_deref() != Some(self.schema_hash) {
            return Err(Error::SchemaMismatch);
        }

        self.next_tx = found_next_tx.unwrap_or(1);

        Ok(())
    }

    fn write_manifest(&self) -> Result<()> {
        let tmp = self.root.join("manifest.tmp");
        let final_path = self.root.join("manifest.txt");

        let mut f = File::create(&tmp)?;

        writeln!(f, "schema_hash={}", self.schema_hash)?;
        writeln!(f, "next_tx={}", self.next_tx)?;

        if self.durable {
            f.sync_all()?;
        }

        fs::rename(tmp, final_path)?;

        Ok(())
    }

    fn table_dir(&self, table: &str) -> PathBuf {
        self.root.join("tables").join(table)
    }

    fn current_path(&self, table: &str) -> PathBuf {
        self.table_dir(table).join("current.jsonl")
    }

    fn lookup_path(&self, table: &str, lookup: &str) -> PathBuf {
        self.root
            .join("lookups")
            .join(format!("{}.{}.lookup", table, lookup))
    }

    fn append_row_line(&mut self, table: &str, line: &str, tx: u64) -> Result<RowPtr> {
        fs::create_dir_all(self.table_dir(table))?;

        let path = self.current_path(table);

        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;

        let offset = f.seek(SeekFrom::End(0))?;

        f.write_all(line.as_bytes())?;
        f.write_all(b"\n")?;

        if self.durable {
            f.sync_data()?;
        }

        let len = line.as_bytes().len() as u32;

        Ok(RowPtr {
            chunk: "current.jsonl".to_string(),
            offset,
            len,
            tx,
            deleted: false,
        })
    }

    fn append_lookup_put(
        &mut self,
        table: &str,
        lookup: &str,
        key: &str,
        ptr: &RowPtr,
    ) -> Result<()> {
        validate_lookup_key(key)?;

        let path = self.lookup_path(table, lookup);

        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;

        writeln!(
            f,
            "{}\t{}\t{}\t{}\t{}\tput",
            key, ptr.chunk, ptr.offset, ptr.len, ptr.tx
        )?;

        if self.durable {
            f.sync_data()?;
        }

        Ok(())
    }

    fn append_lookup_delete(
        &mut self,
        table: &str,
        lookup: &str,
        key: &str,
        tx: u64,
    ) -> Result<()> {
        validate_lookup_key(key)?;

        let path = self.lookup_path(table, lookup);

        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;

        writeln!(f, "{}\t-\t0\t0\t{}\tdelete", key, tx)?;

        if self.durable {
            f.sync_data()?;
        }

        Ok(())
    }

    fn lookup_latest(
        &self,
        table: &str,
        lookup: &str,
        key: &str,
    ) -> Result<Option<RowPtr>> {
        validate_lookup_key(key)?;

        let path = self.lookup_path(table, lookup);

        if !path.exists() {
            return Ok(None);
        }

        let f = File::open(path)?;
        let reader = BufReader::new(f);

        let mut latest: Option<RowPtr> = None;

        for line in reader.lines() {
            let line = line?;

            let mut parts = line.split('\t');

            let k = parts.next().unwrap_or("");
            let chunk = parts.next().unwrap_or("");
            let offset = parts.next().unwrap_or("0");
            let len = parts.next().unwrap_or("0");
            let tx = parts.next().unwrap_or("0");
            let op = parts.next().unwrap_or("");

            if k != key {
                continue;
            }

            let tx = tx.parse::<u64>().unwrap_or(0);

            if op == "delete" {
                latest = Some(RowPtr {
                    chunk: "-".to_string(),
                    offset: 0,
                    len: 0,
                    tx,
                    deleted: true,
                });

                continue;
            }

            latest = Some(RowPtr {
                chunk: chunk.to_string(),
                offset: offset.parse::<u64>().unwrap_or(0),
                len: len.parse::<u32>().unwrap_or(0),
                tx,
                deleted: false,
            });
        }

        if let Some(ptr) = latest {
            if ptr.deleted {
                Ok(None)
            } else {
                Ok(Some(ptr))
            }
        } else {
            Ok(None)
        }
    }

    fn read_ptr<R: RowCodec>(&self, ptr: &RowPtr) -> Result<R> {
        let path = self.table_dir(R::TABLE).join(&ptr.chunk);

        let mut f = File::open(path)?;
        f.seek(SeekFrom::Start(ptr.offset))?;

        let mut buf = vec![0u8; ptr.len as usize];
        f.read_exact(&mut buf)?;

        let line = std::str::from_utf8(&buf).map_err(|_| Error::BadRow)?;

        R::decode_put_line(line)
    }
}
```

---

# 7. Table methods

Add this to the same engine file:

```rust id="q5cfc5"
impl<'a, R: RowCodec> Table<'a, R> {
    pub fn insert(&mut self, row: &R) -> Result<()> {
        if self.get(row.id())?.is_some() {
            return Err(Error::DuplicateId);
        }

        let mut lookups = Vec::new();
        row.lookup_values(&mut lookups);

        for item in &lookups {
            if item.unique {
                if self
                    .db
                    .lookup_latest(R::TABLE, item.name, &item.key)?
                    .is_some()
                {
                    return Err(Error::DuplicateLookup);
                }
            }
        }

        self.put(row)
    }

    pub fn put(&mut self, row: &R) -> Result<()> {
        validate_lookup_key(row.id())?;

        let tx = self.db.alloc_tx()?;

        let mut line = String::with_capacity(256);
        row.encode_put(tx, &mut line);

        let ptr = self.db.append_row_line(R::TABLE, &line, tx)?;

        self.db
            .append_lookup_put(R::TABLE, "id", row.id(), &ptr)?;

        let mut lookups = Vec::new();
        row.lookup_values(&mut lookups);

        for item in lookups {
            self.db
                .append_lookup_put(R::TABLE, item.name, &item.key, &ptr)?;
        }

        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<R>> {
        let Some(ptr) = self.db.lookup_latest(R::TABLE, "id", id)? else {
            return Ok(None);
        };

        let row = self.db.read_ptr::<R>(&ptr)?;

        Ok(Some(row))
    }

    pub fn get_by<L>(&self, key: &str) -> Result<Option<R>>
    where
        L: LookupFor<R>,
    {
        let Some(ptr) = self.db.lookup_latest(R::TABLE, L::NAME, key)? else {
            return Ok(None);
        };

        let row = self.db.read_ptr::<R>(&ptr)?;

        Ok(Some(row))
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        let Some(row) = self.get(id)? else {
            return Ok(());
        };

        let tx = self.db.alloc_tx()?;

        let mut line = String::new();

        line.push_str("{\"_tx\":");
        push_u64(&mut line, tx);
        line.push_str(",\"_op\":\"delete\",\"id\":");
        push_json_str(&mut line, id);
        line.push('}');

        self.db.append_row_line(R::TABLE, &line, tx)?;

        self.db.append_lookup_delete(R::TABLE, "id", id, tx)?;

        let mut lookups = Vec::new();
        row.lookup_values(&mut lookups);

        for item in lookups {
            self.db
                .append_lookup_delete(R::TABLE, item.name, &item.key, tx)?;
        }

        Ok(())
    }
}
```

This is enough for:

```txt id="uyymxy"
insert
put
get by id
get by declared lookup
delete
```

No SQL.

No query planner.

No runtime schema interpreter.

No serde.

---

# 8. Tiny JSON writer helpers

```rust id="qrf9qk"
pub fn push_json_str(out: &mut String, s: &str) {
    out.push('"');

    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                use std::fmt::Write;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }

    out.push('"');
}

pub fn push_u64(out: &mut String, n: u64) {
    use std::fmt::Write;
    let _ = write!(out, "{}", n);
}

pub fn push_i64(out: &mut String, n: i64) {
    use std::fmt::Write;
    let _ = write!(out, "{}", n);
}

pub fn push_f64(out: &mut String, n: f64) {
    use std::fmt::Write;

    if n.is_finite() {
        let _ = write!(out, "{}", n);
    } else {
        out.push_str("0.0");
    }
}

pub fn push_bool(out: &mut String, b: bool) {
    if b {
        out.push_str("true");
    } else {
        out.push_str("false");
    }
}

pub fn validate_lookup_key(key: &str) -> Result<()> {
    if key.is_empty() || key.contains('\t') || key.contains('\n') || key.contains('\r') {
        return Err(Error::BadLookupKey);
    }

    Ok(())
}
```

For v0, lookup keys cannot contain tabs/newlines.

That is fine for:

```txt id="w0q6tp"
id
email
created_at as canonical text
conversation_id
user_id
```

---

# 9. Tiny JSON reader helpers

This is not a general JSON parser.

It is a **canonical Tensack JSONL parser** for objects the engine writes itself.

```rust id="zjj8pc"
pub fn json_get_string(line: &str, key: &str) -> Result<String> {
    let pattern = format!("\"{}\":\"", key);

    let start = line.find(&pattern).ok_or(Error::MissingField)? + pattern.len();

    let mut out = String::new();
    let mut escaped = false;

    for c in line[start..].chars() {
        if escaped {
            match c {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                other => out.push(other),
            }

            escaped = false;
            continue;
        }

        if c == '\\' {
            escaped = true;
            continue;
        }

        if c == '"' {
            return Ok(out);
        }

        out.push(c);
    }

    Err(Error::BadJson)
}

pub fn json_get_i64(line: &str, key: &str) -> Result<i64> {
    let raw = json_get_raw_number(line, key)?;
    raw.parse::<i64>().map_err(|_| Error::BadType)
}

pub fn json_get_f64(line: &str, key: &str) -> Result<f64> {
    let raw = json_get_raw_number(line, key)?;
    let n = raw.parse::<f64>().map_err(|_| Error::BadType)?;

    if n.is_finite() {
        Ok(n)
    } else {
        Err(Error::BadType)
    }
}

pub fn json_get_bool(line: &str, key: &str) -> Result<bool> {
    let pattern = format!("\"{}\":", key);

    let start = line.find(&pattern).ok_or(Error::MissingField)? + pattern.len();
    let rest = &line[start..];

    if rest.starts_with("true") {
        Ok(true)
    } else if rest.starts_with("false") {
        Ok(false)
    } else {
        Err(Error::BadType)
    }
}

fn json_get_raw_number<'a>(line: &'a str, key: &str) -> Result<&'a str> {
    let pattern = format!("\"{}\":", key);

    let start = line.find(&pattern).ok_or(Error::MissingField)? + pattern.len();
    let rest = &line[start..];

    let mut end = 0;

    for b in rest.as_bytes() {
        let c = *b as char;

        if c.is_ascii_digit()
            || c == '-'
            || c == '+'
            || c == '.'
            || c == 'e'
            || c == 'E'
        {
            end += 1;
        } else {
            break;
        }
    }

    if end == 0 {
        return Err(Error::BadType);
    }

    Ok(&rest[..end])
}
```

For the repair/check CLI, you can use `serde_json`.

For the tiny embedded runtime, use this canonical parser.

That keeps the hot binary smaller.

---

# 10. Full example app

`src/main.rs`:

```rust id="9hmwdn"
mod generated {
    pub mod schema;
}

use generated::schema::{open_tensack, users};

fn main() -> tensack_engine::Result<()> {
    let mut db = open_tensack("./data")?;

    db.users().insert(&users::Row {
        id: "u_1".to_string(),
        email: "a@test.com".to_string(),
        name: "Alice".to_string(),
        age: 30,
        score: 98.5,
        active: true,
    })?;

    db.users().insert(&users::Row {
        id: "u_2".to_string(),
        email: "b@test.com".to_string(),
        name: "Bob".to_string(),
        age: 28,
        score: 91.0,
        active: true,
    })?;

    let user = db.users().get("u_1")?;

    println!("by id: {:?}", user);

    let user = db.users().get_by::<users::Email>("a@test.com")?;

    println!("by email: {:?}", user);

    db.users().put(&users::Row {
        id: "u_1".to_string(),
        email: "new@test.com".to_string(),
        name: "Alice Smith".to_string(),
        age: 31,
        score: 99.0,
        active: true,
    })?;

    db.users().delete("u_2")?;

    Ok(())
}
```

After running, files look like:

```txt id="gvz0il"
data/
  manifest.txt
  tables/
    users/
      current.jsonl
  lookups/
    users.id.lookup
    users.email.lookup
```

---

# 11. Tiny CLI shape

Do **not** use `clap` if binary size matters.

Use manual argument parsing:

```rust id="e788lt"
fn main() {
    if let Err(e) = real_main() {
        eprintln!("{:?}", e);
        std::process::exit(1);
    }
}

fn real_main() -> tensack_engine::Result<()> {
    let mut args = std::env::args().skip(1);

    let cmd = args.next().ok_or(tensack_engine::Error::BadRow)?;

    match cmd.as_str() {
        "put" => {
            let root = args.next().ok_or(tensack_engine::Error::BadRow)?;
            let table = args.next().ok_or(tensack_engine::Error::BadRow)?;
            let json = args.next().ok_or(tensack_engine::Error::BadRow)?;

            // In real generated CLI, dispatch table name to generated parser.
            generated::schema::cli_put(&root, &table, &json)
        }

        "get" => {
            let root = args.next().ok_or(tensack_engine::Error::BadRow)?;
            let table = args.next().ok_or(tensack_engine::Error::BadRow)?;
            let id = args.next().ok_or(tensack_engine::Error::BadRow)?;

            generated::schema::cli_get(&root, &table, &id)
        }

        "get-by" => {
            let root = args.next().ok_or(tensack_engine::Error::BadRow)?;
            let table = args.next().ok_or(tensack_engine::Error::BadRow)?;
            let lookup = args.next().ok_or(tensack_engine::Error::BadRow)?;
            let key = args.next().ok_or(tensack_engine::Error::BadRow)?;

            generated::schema::cli_get_by(&root, &table, &lookup, &key)
        }

        "delete" => {
            let root = args.next().ok_or(tensack_engine::Error::BadRow)?;
            let table = args.next().ok_or(tensack_engine::Error::BadRow)?;
            let id = args.next().ok_or(tensack_engine::Error::BadRow)?;

            generated::schema::cli_delete(&root, &table, &id)
        }

        _ => Err(tensack_engine::Error::BadRow),
    }
}
```

The generated schema handles table dispatch.

---

# 14. Generated CLI dispatch example

```rust id="d1mz0e"
pub fn cli_put(root: &str, table: &str, json: &str) -> tensack_engine::Result<()> {
    let mut db = open_tensack(root)?;

    match table {
        "users" => {
            let row = users::Row::decode_external_json(json)?;
            db.users().put(&row)
        }
        _ => Err(tensack_engine::Error::BadRow),
    }
}

pub fn cli_get(root: &str, table: &str, id: &str) -> tensack_engine::Result<()> {
    let mut db = open_tensack(root)?;

    match table {
        "users" => {
            if let Some(row) = db.users().get(id)? {
                let mut out = String::new();
                row.encode_public_json(&mut out);
                println!("{}", out);
            }

            Ok(())
        }
        _ => Err(tensack_engine::Error::BadRow),
    }
}

pub fn cli_get_by(
    root: &str,
    table: &str,
    lookup: &str,
    key: &str,
) -> tensack_engine::Result<()> {
    let mut db = open_tensack(root)?;

    match (table, lookup) {
        ("users", "email") => {
            if let Some(row) = db.users().get_by::<users::Email>(key)? {
                let mut out = String::new();
                row.encode_public_json(&mut out);
                println!("{}", out);
            }

            Ok(())
        }
        _ => Err(tensack_engine::Error::BadRow),
    }
}

pub fn cli_delete(root: &str, table: &str, id: &str) -> tensack_engine::Result<()> {
    let mut db = open_tensack(root)?;

    match table {
        "users" => db.users().delete(id),
        _ => Err(tensack_engine::Error::BadRow),
    }
}
```

Again: this generated CLI dispatch means no runtime schema macro expansion.

---

# 15. Generated public JSON encoding

Internal row line includes `_tx` and `_op`.

Public JSON should not.

```rust id="cs7v98"
impl users::Row {
    pub fn encode_public_json(&self, out: &mut String) {
        out.clear();

        out.push('{');

        out.push_str("\"id\":");
        tensack_engine::push_json_str(out, &self.id);

        out.push_str(",\"email\":");
        tensack_engine::push_json_str(out, &self.email);

        out.push_str(",\"name\":");
        tensack_engine::push_json_str(out, &self.name);

        out.push_str(",\"age\":");
        tensack_engine::push_i64(out, self.age);

        out.push_str(",\"score\":");
        tensack_engine::push_f64(out, self.score);

        out.push_str(",\"active\":");
        tensack_engine::push_bool(out, self.active);

        out.push('}');
    }

    pub fn decode_external_json(line: &str) -> tensack_engine::Result<Self> {
        Ok(Self {
            id: tensack_engine::json_get_string(line, "id")?,
            email: tensack_engine::json_get_string(line, "email")?,
            name: tensack_engine::json_get_string(line, "name")?,
            age: tensack_engine::json_get_i64(line, "age")?,
            score: tensack_engine::json_get_f64(line, "score")?,
            active: tensack_engine::json_get_bool(line, "active")?,
        })
    }
}
```

This keeps the CLI small too.

No `serde_json`.

---

# 16. Why this stays small

The final runtime contains:

```txt id="4nwvne"
std fs/io
generated row structs
generated row encoders
generated row decoders
append JSONL writer
lookup file scanner
manifest text reader/writer
```

The final runtime does **not** contain:

```txt id="sm5wtn"
SQL parser
query planner
schema macro expansion
serde
serde_json
clap
tokio
async runtime
HTTP server
regex
rocksdb
sqlite
lmdb
parquet
arrow
```

That is the size win.

---

# 17. Performance upgrade path without changing surface

The v0 lookup implementation scans `.lookup` files.

That is tiny.

When you need speed, keep the same API and change internals.

## v0

```txt id="6i9o6m"
lookup file:
  text append log

read:
  scan lookup file
  find latest matching key
```

Smallest code. Slower for huge lookup files.

## v1

```txt id="7nvukb"
on open:
  load lookup files into HashMap<String, RowPtr>

read:
  O(1) memory lookup
  seek to row
```

Still simple.

## v2

```txt id="pdjzwo"
lookup file:
  binary hash table

read:
  mmap lookup file
  direct seek
```

Fast and compact.

The SDK does not change:

```rust id="b0wmyy"
db.users().get("u_1")?;
db.users().get_by::<users::Email>("a@test.com")?;
```

---

# 18. Compaction surface

API:

```rust id="j8qpcm"
db.users().compact()?;
```

Internal behavior:

```txt id="kvytjd"
1. Read all rows in table chunks.
2. Keep latest put per id.
3. Remove deleted ids.
4. Write compacted tmp chunk.
5. Replace old chunk set.
6. Rebuild lookups.
```

Do not build this first.

Build first:

```txt id="9dj38v"
insert
put
get
get_by
delete
```

Then compaction.

---

# 19. Final minimal backend structure

```txt id="giisuy"
tensack/
  crates/
    tensack-engine/
      Cargo.toml
      src/
        lib.rs

    tensack-schema/
      Cargo.toml
      src/
        macro_input.rs
        validate.rs
        ir.rs

    tensack-codegen/
      Cargo.toml
      src/
        lib.rs
        rust.rs
        ts.rs

  examples/
    chat/
      Cargo.toml
      build.rs
      schema.tensack
      src/
        main.rs
        generated/
          schema.rs
```

Runtime dependency graph:

```txt id="oznz0u"
chat binary
  ├── tensack-engine
  └── generated/schema.rs
```

Build-time dependency graph:

```txt id="t6gwc3"
build.rs
  └── tensack-codegen
        └── tensack-schema
```

That is the important architecture.

---

# 20. The right v0 target

Build this exact surface first:

```rust id="jeol4p"
db.users().insert(&row)?;
db.users().put(&row)?;
db.users().get("u_1")?;
db.users().get_by::<users::Email>("a@test.com")?;
db.users().delete("u_1")?;
```

With these files:

```txt id="ajbnjw"
current.jsonl
users.id.lookup
users.email.lookup
manifest.txt
```

With these primitive types:

```txt id="3q6pt6"
id      String
text    String
int     i64
float   f64
bool    bool
```

With this binary philosophy:

```txt id="wrgtp2"
No runtime schema macro expansion.
No serde in hot binary.
No SQL.
No query planner.
No async.
No external database.
Generated code does the type work.
JSONL is the readable truth.
Lookup files are rebuildable acceleration.
```

---

# 21. What still has to be added

This starter zip includes the minimal engine shell, a generated `users` table example, a tiny codegen placeholder, and a schema placeholder.

The missing production pieces are:

```txt
1. Real schema.tensack macro input handling in crates/tensack-schema.
2. Real schema validator for names, duplicate fields, primitive types, id field, and lookup declarations.
3. Real Rust code generator for arbitrary tables instead of the hardcoded users example.
4. Repair and inspect commands.
6. Check/repair CLI using a full JSON parser such as serde_json outside the tiny runtime.
7. Compaction implementation.
8. Lookup rebuild implementation.
9. Optional in-memory lookup cache for faster reads.
10. Optional binary lookup file format for v2.
11. Multi-process locking.
12. Crash recovery that scans current.jsonl and manifest to repair next_tx.
```

The current folder is intentionally small. The hot runtime path is already represented by `tensack-engine/src/lib.rs` and `examples/chat/src/generated/schema.rs`.
