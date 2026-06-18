//! Public Tensack database API.
//!
//! This crate composes the core data model, file format boundary, and local
//! storage engine. Apps should usually depend on this crate instead of wiring
//! lower-level packages together directly.

use std::path::{Path, PathBuf};

pub use tensack_core::{
    DatabaseSchema, FieldSpec, PrimitiveType, Record, SchemaError, TableSchema, Workspace,
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

    /// Writes a put operation to the database.
    pub fn put(&self, record: &Record) -> Result<AppendResult, TensackError> {
        self.schema.validate_record(record)?;
        self.store
            .append_put(&self.schema, record)
            .map_err(TensackError::from)
    }

    /// Writes a delete operation to the database.
    pub fn delete(&self, record: &Record) -> Result<AppendResult, TensackError> {
        self.schema.validate_record(record)?;
        self.store
            .append_delete(&self.schema, record)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tensack_core::PrimitiveType;

    fn temp_root() -> PathBuf {
        let mut dir = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        dir.push(format!("tensack-db-{stamp}"));
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
        assert!(db_dir.join("engine/messages.btf").exists());
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
