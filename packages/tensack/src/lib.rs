//! Public Tensack database API.
//!
//! This crate composes the core data model, file format boundary, and local
//! storage engine. Apps should usually depend on this crate instead of wiring
//! lower-level packages together directly.

use std::path::{Path, PathBuf};

pub use tensack_core::{
    DatabaseSchema, FieldSpec, PrimitiveType, Record, SackValue, SchemaError, TableSchema,
    Workspace,
};
pub use tensack_format::Operation;
pub use tensack_store::{AppendResult, LocalStore};

#[macro_export]
macro_rules! __tensack_rust_type {
    (id) => {
        ::std::string::String
    };
    (text) => {
        ::std::string::String
    };
    (int) => {
        i64
    };
    (float) => {
        f64
    };
    (bool) => {
        bool
    };
}

#[macro_export]
macro_rules! __tensack_primitive_type {
    (id) => {
        $crate::PrimitiveType::Id
    };
    (text) => {
        $crate::PrimitiveType::Text
    };
    (int) => {
        $crate::PrimitiveType::Int
    };
    (float) => {
        $crate::PrimitiveType::Float
    };
    (bool) => {
        $crate::PrimitiveType::Bool
    };
}

#[macro_export]
macro_rules! __tensack_schema_items {
    ($table:ident;) => {};
    ($table:ident; lookup $field:ident unique $($rest:tt)*) => {
        $table.add_lookup(stringify!($field), true).unwrap();
        $crate::__tensack_schema_items!($table; $($rest)*);
    };
    ($table:ident; lookup $field:ident $($rest:tt)*) => {
        $table.add_lookup(stringify!($field), false).unwrap();
        $crate::__tensack_schema_items!($table; $($rest)*);
    };
    ($table:ident; $field:ident $field_ty:ident $($rest:tt)*) => {
        $table
            .add_field(
                stringify!($field),
                $crate::__tensack_primitive_type!($field_ty),
            )
            .unwrap();
        $crate::__tensack_schema_items!($table; $($rest)*);
    };
}

/// Declares a compact schema surface for a `schema.tensack` include file.
///
/// The intended authoring shape is:
///
/// ```rust
/// # use tensack::schema;
/// schema! {
///     users {
///         id id
///         email text
///
///         lookup email unique
///     }
/// }
/// ```
///
/// This emits one module per table with a `table_schema()` function and a
/// top-level `database_schema()` function that combines all declared tables.
#[macro_export]
macro_rules! schema {
    ($($table:ident { $($body:tt)* })*) => {
        $(
            pub mod $table {
                pub const NAME: &str = stringify!($table);

                pub fn table_schema() -> $crate::TableSchema {
                    let mut table = $crate::TableSchema::new(NAME);
                    $crate::__tensack_schema_items!(table; $($body)*);
                    table
                }
            }
        )*

        pub fn database_schema() -> $crate::DatabaseSchema {
            let mut db = $crate::DatabaseSchema::new();
            $(
                db.add_table($table::table_schema()).unwrap();
            )*
            db
        }
    };
}

/// Very small `table!` helper for a local schema-first path.
///
/// It emits:
/// - a module for the table name
/// - a typed `Row` struct using primitive Rust types
/// - a `table_schema()` function that builds a `TableSchema`
/// - a tiny `table_database()` function that wraps the schema as a 1-table DB
///
/// New schema files should prefer `schema!`; this macro remains useful for
/// narrow typed-row experiments.
///
/// This intentionally keeps syntax minimal and does not attempt full parse-time
/// validation beyond macro syntax and known primitive type names.
#[macro_export]
macro_rules! table {
    ($table:ident { $($field:ident : $field_ty:ident $(;)?)* }) => {
        pub mod $table {
            #[derive(Debug, Clone, PartialEq)]
            pub struct Row {
                $(
                    pub $field: $crate::__tensack_rust_type!($field_ty),
                )*
            }

            pub fn table_schema() -> $crate::TableSchema {
                let mut table = $crate::TableSchema::new(stringify!($table));
                $(
                    table
                        .add_field(
                            stringify!($field),
                            $crate::__tensack_primitive_type!($field_ty),
                        )
                        .unwrap();
                )*

                table
            }

            pub fn table_database() -> $crate::DatabaseSchema {
                let mut db = $crate::DatabaseSchema::new();
                db.add_table(table_schema()).unwrap();
                db
            }
        }
    };
}

/// Public database errors.
#[derive(Debug)]
pub enum TensackError {
    /// Filesystem-level append/parsing error from the local store.
    Io(std::io::Error),
    /// Schema/validation failure.
    Schema(SchemaError),
}

impl std::fmt::Display for TensackError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Schema(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for TensackError {}

impl From<std::io::Error> for TensackError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<SchemaError> for TensackError {
    fn from(error: SchemaError) -> Self {
        Self::Schema(error)
    }
}

/// A local Tensack database handle.
#[derive(Debug, Clone, PartialEq)]
pub struct TensackDatabase {
    workspace: Workspace,
    store: LocalStore,
    schema: DatabaseSchema,
}

impl TensackDatabase {
    /// Opens a local database handle with an empty schema.
    pub fn open_local(root: impl Into<PathBuf>, workspace_name: impl Into<String>) -> Self {
        Self::open_local_with_schema(root, workspace_name, DatabaseSchema::new())
    }

    /// Opens a local database handle bound to a schema.
    pub fn open_local_with_schema(
        root: impl Into<PathBuf>,
        workspace_name: impl Into<String>,
        schema: DatabaseSchema,
    ) -> Self {
        let workspace_name = workspace_name.into();
        let workspace = Workspace::new(&workspace_name);
        let store = LocalStore::new(root, workspace_name);

        Self {
            workspace,
            store,
            schema,
        }
    }

    /// Returns the workspace.
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Returns the local store root.
    pub fn store_root(&self) -> &Path {
        self.store.root()
    }

    /// Returns configured schema.
    pub fn schema(&self) -> &DatabaseSchema {
        &self.schema
    }

    /// Replaces schema for this database handle.
    pub fn with_schema(mut self, schema: DatabaseSchema) -> Self {
        self.schema = schema;
        self
    }

    /// Creates the empty database layout for all tables in the current schema.
    pub fn init(&self) -> Result<(), TensackError> {
        self.store.init(&self.schema).map_err(TensackError::from)
    }

    /// Inserts a new row. Fails if the id already exists.
    pub fn insert(&self, record: &Record) -> Result<AppendResult, TensackError> {
        self.schema.validate_record(record)?;
        self.store
            .append_insert(&self.schema, record)
            .map_err(TensackError::from)
    }

    /// Writes a replacement row, or inserts it if it does not exist.
    pub fn put(&self, record: &Record) -> Result<AppendResult, TensackError> {
        self.schema.validate_record(record)?;
        self.store
            .append_put(&self.schema, record)
            .map_err(TensackError::from)
    }

    /// Writes a delete operation using the id from a row-like record.
    pub fn delete(&self, record: &Record) -> Result<AppendResult, TensackError> {
        let id = record_id(record)?;
        self.delete_by_id(record.table(), &id)
    }

    /// Deletes a row by id. This does not require the rest of the row fields.
    pub fn delete_by_id(&self, table_name: &str, id: &str) -> Result<AppendResult, TensackError> {
        self.store
            .append_delete_id(&self.schema, table_name, id)
            .map_err(TensackError::from)
    }

    /// Writes a row with a specific operation (advanced path).
    pub fn apply(
        &self,
        operation: Operation,
        record: &Record,
    ) -> Result<AppendResult, TensackError> {
        self.schema.validate_record(record)?;
        self.store
            .append(&self.schema, operation, record)
            .map_err(TensackError::from)
    }

    /// Reads the current live row for a table id.
    pub fn get(&self, table_name: &str, id: &str) -> Result<Option<Record>, TensackError> {
        self.store
            .get_by_id(&self.schema, table_name, id)
            .map_err(TensackError::from)
    }

    /// Reads the first current live row matching a lookup key.
    pub fn get_by(
        &self,
        table_name: &str,
        lookup_field: &str,
        key: &str,
    ) -> Result<Option<Record>, TensackError> {
        Ok(self
            .store
            .get_by_lookup(&self.schema, table_name, lookup_field, key)
            .map_err(TensackError::from)?
            .into_iter()
            .next())
    }

    /// Reads all current live rows matching a lookup key.
    pub fn get_many_by(
        &self,
        table_name: &str,
        lookup_field: &str,
        key: &str,
    ) -> Result<Vec<Record>, TensackError> {
        self.store
            .get_by_lookup(&self.schema, table_name, lookup_field, key)
            .map_err(TensackError::from)
    }

    /// Rebuilds the generated `.tenb` cache for one table from canonical `.ten`.
    pub fn rebuild_cache(&self, table_name: &str) -> Result<(), TensackError> {
        self.store
            .rebuild_tenb(&self.schema, table_name)
            .map(|_| ())
            .map_err(TensackError::from)
    }
}

fn record_id(record: &Record) -> Result<String, TensackError> {
    match record.fields().get("id") {
        Some(SackValue::Id(value)) | Some(SackValue::Text(value)) => Ok(value.clone()),
        Some(value) => Err(TensackError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("record id must be id/text, got {}", value.value_type()),
        ))),
        None => Err(TensackError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "record missing id",
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tensack_core::PrimitiveType;

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_root() -> PathBuf {
        let mut dir = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        dir.push(format!(
            "tensack-db-{}-{stamp}-{counter}",
            std::process::id()
        ));
        dir
    }

    fn schema() -> DatabaseSchema {
        let mut db = DatabaseSchema::new();
        let mut messages = TableSchema::new("messages");
        messages.add_field("id", PrimitiveType::Id).unwrap();
        messages.add_field("body", PrimitiveType::Text).unwrap();
        db.add_table(messages).unwrap();
        db
    }

    fn note_schema() -> DatabaseSchema {
        let mut db = DatabaseSchema::new();
        let mut notebooks = TableSchema::new("notebooks");
        notebooks.add_field("id", PrimitiveType::Id).unwrap();
        notebooks.add_field("title", PrimitiveType::Text).unwrap();
        notebooks
            .add_field("created_at", PrimitiveType::Int)
            .unwrap();
        notebooks.add_lookup("title", false).unwrap();
        db.add_table(notebooks).unwrap();

        let mut notes = TableSchema::new("notes");
        notes.add_field("id", PrimitiveType::Id).unwrap();
        notes.add_field("notebook_id", PrimitiveType::Id).unwrap();
        notes.add_field("title", PrimitiveType::Text).unwrap();
        notes.add_field("body", PrimitiveType::Text).unwrap();
        notes.add_field("updated_at", PrimitiveType::Int).unwrap();
        notes.add_lookup("notebook_id", false).unwrap();
        notes.add_lookup("updated_at", false).unwrap();
        db.add_table(notes).unwrap();
        db
    }

    #[test]
    fn init_creates_empty_note_database_layout() {
        let root = temp_root();
        let db = TensackDatabase::open_local_with_schema(root.clone(), "notes-db", note_schema());

        db.init().unwrap();
        let db_dir = root.join("notes-db");
        assert!(db_dir.join("tensack.toml").exists());
        assert!(db_dir.join("tables/notebooks/active.ten").exists());
        assert!(db_dir.join("tables/notes/active.ten").exists());
        assert!(db_dir.join("engine/notebooks.tenb").exists());
        assert!(db_dir.join("engine/notes.tenb").exists());

        let notebooks = fs::read_to_string(db_dir.join("tables/notebooks/active.ten")).unwrap();
        assert!(notebooks.contains("TEN\t1\ttable\tnotebooks\t"));
        assert!(notebooks.contains("@field\ttitle\ttext\n"));
        assert!(notebooks.contains("@lookup\tid\tunique\n"));
        assert!(notebooks.contains("@lookup\ttitle\tmany\n"));
        assert!(notebooks.ends_with("@data\n"));

        let notes = fs::read_to_string(db_dir.join("tables/notes/active.ten")).unwrap();
        assert!(notes.contains("@field\tnotebook_id\tid\n"));
        assert!(notes.contains("@lookup\tnotebook_id\tmany\n"));
        assert!(notes.ends_with("@data\n"));

        let metadata = fs::read_to_string(db_dir.join("tensack.toml")).unwrap();
        assert!(metadata.contains("[tables.notebooks]"));
        assert!(metadata.contains("[tables.notes]"));
        assert!(metadata.contains("file = \"engine/notebooks.tenb\""));
        assert!(metadata.contains("file = \"engine/notes.tenb\""));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn put_validates_and_appends() {
        let root = temp_root();
        let db = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema());

        let first = Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", "hello")
            .unwrap();
        let second = Record::new("messages")
            .with_id("m2")
            .unwrap()
            .with_field("body", "world")
            .unwrap();

        let one = db.put(&first).unwrap();
        let two = db.put(&second).unwrap();
        assert_eq!(one.tx_id, 1);
        assert_eq!(two.tx_id, 2);
        let db_dir = root.join("chat");
        assert!(db_dir.join("tables/messages/active.ten").exists());
        assert!(db_dir.join("tensack.toml").exists());
        assert!(db_dir.join("engine/messages.tenb").exists());
        let active = fs::read_to_string(db_dir.join("tables/messages/active.ten")).unwrap();
        assert!(active.starts_with("TEN\t1\ttable\tmessages\t"));
        assert!(active.contains("R\t1\tm1\thello\n"));
        assert!(active.contains("R\t2\tm2\tworld\n"));
        assert_eq!(
            db.get("messages", "m1")
                .unwrap()
                .unwrap()
                .fields()
                .get("body"),
            first.fields().get("body")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn insert_fails_when_id_already_exists() {
        let root = temp_root();
        let db = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema());
        let row = Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", "hello")
            .unwrap();

        db.insert(&row).unwrap();
        assert!(db.insert(&row).is_err());
        let rows = db
            .store
            .read_table(db.schema(), "messages")
            .expect("read table");
        assert_eq!(rows.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn put_replaces_existing_row() {
        let root = temp_root();
        let db = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema());
        let first = Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", "hello")
            .unwrap();
        let second = Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", "updated")
            .unwrap();

        db.insert(&first).unwrap();
        db.put(&second).unwrap();
        assert_eq!(
            db.get("messages", "m1")
                .unwrap()
                .unwrap()
                .fields()
                .get("body"),
            second.fields().get("body")
        );
        let rows = db
            .store
            .read_table(db.schema(), "messages")
            .expect("read table");
        assert_eq!(rows.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn get_by_lookup_uses_generated_tenb_cache() {
        let root = temp_root();
        let mut schema = DatabaseSchema::new();
        let mut messages = TableSchema::new("messages");
        messages.add_field("id", PrimitiveType::Id).unwrap();
        messages
            .add_field("conversation_id", PrimitiveType::Id)
            .unwrap();
        messages.add_field("body", PrimitiveType::Text).unwrap();
        messages.add_lookup("conversation_id", false).unwrap();
        schema.add_table(messages).unwrap();
        let db = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema);

        let first = Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field(
                "conversation_id",
                tensack_core::SackValue::Id("cv1".to_owned()),
            )
            .unwrap()
            .with_field("body", "hello")
            .unwrap();
        let second = Record::new("messages")
            .with_id("m2")
            .unwrap()
            .with_field(
                "conversation_id",
                tensack_core::SackValue::Id("cv1".to_owned()),
            )
            .unwrap()
            .with_field("body", "world")
            .unwrap();

        db.put(&first).unwrap();
        db.put(&second).unwrap();
        let rows = db
            .get_many_by("messages", "conversation_id", "cv1")
            .unwrap();
        assert_eq!(rows.len(), 2);

        fs::remove_file(root.join("chat/engine/messages.tenb")).unwrap();
        assert_eq!(
            db.get("messages", "m2")
                .unwrap()
                .unwrap()
                .fields()
                .get("body"),
            second.fields().get("body")
        );
        assert!(root.join("chat/engine/messages.tenb").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn delete_removes_live_row_from_tenb_cache() {
        let root = temp_root();
        let db = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema());
        let row = Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", "hello")
            .unwrap();

        db.put(&row).unwrap();
        assert!(db.get("messages", "m1").unwrap().is_some());
        db.delete_by_id("messages", "m1").unwrap();
        assert!(db.get("messages", "m1").unwrap().is_none());

        let active = fs::read_to_string(root.join("chat/tables/messages/active.ten")).unwrap();
        assert!(active.contains("D\t2\tm1\n"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unique_lookup_conflicts_fail_before_append() {
        let root = temp_root();
        let mut schema = DatabaseSchema::new();
        let mut users = TableSchema::new("users");
        users.add_field("id", PrimitiveType::Id).unwrap();
        users.add_field("email", PrimitiveType::Text).unwrap();
        users.add_field("name", PrimitiveType::Text).unwrap();
        users.add_lookup("email", true).unwrap();
        schema.add_table(users).unwrap();
        let db = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema);

        let first = Record::new("users")
            .with_id("u1")
            .unwrap()
            .with_field("email", "same@test.com")
            .unwrap()
            .with_field("name", "Ada")
            .unwrap();
        let second = Record::new("users")
            .with_id("u2")
            .unwrap()
            .with_field("email", "same@test.com")
            .unwrap()
            .with_field("name", "Ben")
            .unwrap();

        db.insert(&first).unwrap();
        assert!(db.insert(&second).is_err());
        let active = fs::read_to_string(root.join("chat/tables/users/active.ten")).unwrap();
        assert!(!active.contains("u2\tsame@test.com"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn put_fails_schema_mismatch() {
        let root = temp_root();
        let db = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema());
        let bad = Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", 42i64)
            .unwrap();

        assert!(db.put(&bad).is_err());
        let _ = fs::remove_dir_all(root);
    }

    table!(chat_schema_example {
        id: id;
        username: text;
        score: float;
        has_premium: bool;
    });

    #[test]
    fn macro_generates_table_schema() {
        let table = chat_schema_example::table_schema();
        let db_schema = chat_schema_example::table_database();
        assert_eq!(table.name(), "chat_schema_example");
        assert!(db_schema.table("chat_schema_example").is_some());
        assert_eq!(table.field("id").map(|f| f.kind()), Some(PrimitiveType::Id));
    }

    #[test]
    fn macro_row_types_are_concrete() {
        let row = chat_schema_example::Row {
            id: "u1".to_string(),
            username: "mira".to_string(),
            score: 12.5,
            has_premium: false,
        };
        assert_eq!(row.id, "u1");
    }

    schema! {
        chat {
            id id
            owner_id id
            title text
            created_at int

            lookup owner_id
            lookup created_at
        }

        schema_users {
            id id
            email text
            name text

            lookup email unique
        }
    }

    #[test]
    fn schema_macro_generates_database_schema() {
        let db = database_schema();
        let users = db.table("schema_users").unwrap();
        let chat = db.table("chat").unwrap();
        assert_eq!(users.field("email").unwrap().kind(), PrimitiveType::Text);
        assert!(users.lookup("email").unwrap().unique());
        assert!(!chat.lookup("owner_id").unwrap().unique());
    }
}
