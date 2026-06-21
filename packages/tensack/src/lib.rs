//! Public Tensack database API.
//!
//! This crate composes the core data model, file format boundary, and local
//! storage engine. Apps should usually depend on this crate instead of wiring
//! lower-level packages together directly.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub use tensack_core::{
    DatabaseSchema, FieldSpec, PrimitiveType, Record, SchemaError, TableSchema, Value, Workspace,
};
pub use tensack_format::Operation;
pub use tensack_store::{AppendOperation, AppendResult, LocalStore};

const DEFAULT_PLAN_LIMIT: usize = 100;
const MAX_PLAN_LIMIT: usize = 1_000;

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
    /// Internal plan validation or execution failure.
    Plan(PlanError),
}

impl std::fmt::Display for TensackError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Schema(error) => write!(formatter, "{error}"),
            Self::Plan(error) => write!(formatter, "{error}"),
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

impl From<PlanError> for TensackError {
    fn from(error: PlanError) -> Self {
        Self::Plan(error)
    }
}

/// Internal plan operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanOp {
    Insert,
    Upsert,
    Patch,
    Remove,
    Get,
    Find,
    Scan,
    Count,
}

/// Internal operation envelope shared by generated APIs and runtime entrypoints.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanEnvelope {
    pub op: PlanOp,
    pub table: String,
    pub lookup: Option<String>,
    pub key: BTreeMap<String, Value>,
    pub value: BTreeMap<String, Value>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

impl PlanEnvelope {
    pub fn new(op: PlanOp, table: impl Into<String>) -> Self {
        Self {
            op,
            table: table.into(),
            lookup: None,
            key: BTreeMap::new(),
            value: BTreeMap::new(),
            limit: None,
            cursor: None,
        }
    }

    pub fn with_lookup(mut self, lookup: impl Into<String>) -> Self {
        self.lookup = Some(lookup.into());
        self
    }

    pub fn with_key(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.key.insert(name.into(), value.into());
        self
    }

    pub fn with_value(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.value.insert(name.into(), value.into());
        self
    }

    pub fn with_record_value(mut self, record: Record) -> Self {
        self.value = record.fields().clone();
        self
    }

    pub fn with_values(mut self, values: BTreeMap<String, Value>) -> Self {
        self.value = values;
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn with_cursor(mut self, cursor: impl Into<String>) -> Self {
        self.cursor = Some(cursor.into());
        self
    }
}

/// Paged row result for plan reads.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanPage {
    pub rows: Vec<Record>,
    pub next_cursor: Option<String>,
}

/// Result of executing one internal plan.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanOutcome {
    Append(AppendResult),
    Row(Option<Record>),
    Rows(PlanPage),
    Count(usize),
}

/// Declarative request for current state.
pub trait GetRequest {
    type Output;

    fn into_plan(self) -> Result<PlanEnvelope, TensackError>;

    fn from_outcome(outcome: PlanOutcome) -> Result<Self::Output, TensackError>;
}

/// Declarative request for a state change.
pub trait WriteRequest {
    type Output;

    fn into_plan(self) -> Result<PlanEnvelope, TensackError>;

    fn from_outcome(outcome: PlanOutcome) -> Result<Self::Output, TensackError>;
}

/// Selector for one row through a unique lookup.
#[derive(Debug, Clone, PartialEq)]
pub struct GetOne {
    plan: PlanEnvelope,
}

impl GetOne {
    pub fn new(
        table: impl Into<String>,
        lookup: impl Into<String>,
        value: impl Into<Value>,
    ) -> Self {
        let lookup = lookup.into();
        Self {
            plan: PlanEnvelope::new(PlanOp::Get, table)
                .with_lookup(lookup.clone())
                .with_key(lookup, value),
        }
    }

    pub fn into_plan(self) -> PlanEnvelope {
        self.plan
    }

    pub fn from_outcome(outcome: PlanOutcome) -> Result<Option<Record>, TensackError> {
        match outcome {
            PlanOutcome::Row(row) => Ok(row),
            _ => Err(PlanError::Invalid("get selector returned non-row outcome".to_owned()).into()),
        }
    }
}

impl GetRequest for GetOne {
    type Output = Option<Record>;

    fn into_plan(self) -> Result<PlanEnvelope, TensackError> {
        Ok(self.into_plan())
    }

    fn from_outcome(outcome: PlanOutcome) -> Result<Self::Output, TensackError> {
        Self::from_outcome(outcome)
    }
}

/// Selector for many rows through a declared lookup.
#[derive(Debug, Clone, PartialEq)]
pub struct GetMany {
    plan: PlanEnvelope,
}

impl GetMany {
    pub fn new(
        table: impl Into<String>,
        lookup: impl Into<String>,
        value: impl Into<Value>,
    ) -> Self {
        let lookup = lookup.into();
        Self {
            plan: PlanEnvelope::new(PlanOp::Find, table)
                .with_lookup(lookup.clone())
                .with_key(lookup, value)
                .with_limit(MAX_PLAN_LIMIT),
        }
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.plan.limit = Some(limit);
        self
    }

    pub fn cursor(mut self, cursor: impl Into<String>) -> Self {
        self.plan.cursor = Some(cursor.into());
        self
    }

    pub fn into_plan(self) -> PlanEnvelope {
        self.plan
    }

    pub fn from_outcome(outcome: PlanOutcome) -> Result<Vec<Record>, TensackError> {
        match outcome {
            PlanOutcome::Rows(page) => Ok(page.rows),
            _ => {
                Err(PlanError::Invalid("get selector returned non-rows outcome".to_owned()).into())
            }
        }
    }
}

impl GetRequest for GetMany {
    type Output = Vec<Record>;

    fn into_plan(self) -> Result<PlanEnvelope, TensackError> {
        Ok(self.into_plan())
    }

    fn from_outcome(outcome: PlanOutcome) -> Result<Self::Output, TensackError> {
        Self::from_outcome(outcome)
    }
}

/// Selector for a page of table rows.
#[derive(Debug, Clone, PartialEq)]
pub struct GetPage {
    plan: PlanEnvelope,
}

impl GetPage {
    pub fn new(table: impl Into<String>) -> Self {
        Self {
            plan: PlanEnvelope::new(PlanOp::Scan, table),
        }
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.plan.limit = Some(limit);
        self
    }

    pub fn cursor(mut self, cursor: impl Into<String>) -> Self {
        self.plan.cursor = Some(cursor.into());
        self
    }

    pub fn into_plan(self) -> PlanEnvelope {
        self.plan
    }

    pub fn from_outcome(outcome: PlanOutcome) -> Result<PlanPage, TensackError> {
        match outcome {
            PlanOutcome::Rows(page) => Ok(page),
            _ => {
                Err(PlanError::Invalid("get selector returned non-page outcome".to_owned()).into())
            }
        }
    }
}

impl GetRequest for GetPage {
    type Output = PlanPage;

    fn into_plan(self) -> Result<PlanEnvelope, TensackError> {
        Ok(self.into_plan())
    }

    fn from_outcome(outcome: PlanOutcome) -> Result<Self::Output, TensackError> {
        Self::from_outcome(outcome)
    }
}

/// Selector for a table or lookup count.
#[derive(Debug, Clone, PartialEq)]
pub struct GetCount {
    plan: PlanEnvelope,
}

impl GetCount {
    pub fn table(table: impl Into<String>) -> Self {
        Self {
            plan: PlanEnvelope::new(PlanOp::Count, table),
        }
    }

    pub fn lookup(
        table: impl Into<String>,
        lookup: impl Into<String>,
        value: impl Into<Value>,
    ) -> Self {
        let lookup = lookup.into();
        Self {
            plan: PlanEnvelope::new(PlanOp::Count, table)
                .with_lookup(lookup.clone())
                .with_key(lookup, value),
        }
    }

    pub fn into_plan(self) -> PlanEnvelope {
        self.plan
    }

    pub fn from_outcome(outcome: PlanOutcome) -> Result<usize, TensackError> {
        match outcome {
            PlanOutcome::Count(count) => Ok(count),
            _ => {
                Err(PlanError::Invalid("get selector returned non-count outcome".to_owned()).into())
            }
        }
    }
}

impl GetRequest for GetCount {
    type Output = usize;

    fn into_plan(self) -> Result<PlanEnvelope, TensackError> {
        Ok(self.into_plan())
    }

    fn from_outcome(outcome: PlanOutcome) -> Result<Self::Output, TensackError> {
        Self::from_outcome(outcome)
    }
}

/// Declarative state change.
#[derive(Debug, Clone, PartialEq)]
pub struct WriteChange {
    plan: PlanEnvelope,
}

impl WriteChange {
    pub fn add_record(record: Record) -> Self {
        Self {
            plan: PlanEnvelope::new(PlanOp::Insert, record.table()).with_record_value(record),
        }
    }

    pub fn set_record(record: Record) -> Self {
        Self {
            plan: PlanEnvelope::new(PlanOp::Upsert, record.table()).with_record_value(record),
        }
    }

    pub fn edit(
        table: impl Into<String>,
        lookup: impl Into<String>,
        key: impl Into<Value>,
        value: BTreeMap<String, Value>,
    ) -> Self {
        let lookup = lookup.into();
        Self {
            plan: PlanEnvelope::new(PlanOp::Patch, table)
                .with_lookup(lookup.clone())
                .with_key(lookup, key)
                .with_values(value),
        }
    }

    pub fn remove(
        table: impl Into<String>,
        lookup: impl Into<String>,
        key: impl Into<Value>,
    ) -> Self {
        let lookup = lookup.into();
        Self {
            plan: PlanEnvelope::new(PlanOp::Remove, table)
                .with_lookup(lookup.clone())
                .with_key(lookup, key),
        }
    }

    pub fn into_plan(self) -> PlanEnvelope {
        self.plan
    }

    pub fn from_outcome(outcome: PlanOutcome) -> Result<AppendResult, TensackError> {
        match outcome {
            PlanOutcome::Append(result) => Ok(result),
            _ => Err(
                PlanError::Invalid("write change returned non-append outcome".to_owned()).into(),
            ),
        }
    }
}

impl WriteRequest for WriteChange {
    type Output = AppendResult;

    fn into_plan(self) -> Result<PlanEnvelope, TensackError> {
        Ok(self.into_plan())
    }

    fn from_outcome(outcome: PlanOutcome) -> Result<Self::Output, TensackError> {
        Self::from_outcome(outcome)
    }
}

pub mod selector {
    use super::{GetCount, GetMany, GetOne, GetPage, Value};

    pub fn id(table: impl Into<String>, id: impl Into<String>) -> GetOne {
        GetOne::new(table, "id", Value::Id(id.into()))
    }

    pub fn one(
        table: impl Into<String>,
        lookup: impl Into<String>,
        key: impl Into<String>,
    ) -> GetOne {
        GetOne::new(table, lookup, Value::Text(key.into()))
    }

    pub fn many(
        table: impl Into<String>,
        lookup: impl Into<String>,
        key: impl Into<String>,
    ) -> GetMany {
        GetMany::new(table, lookup, Value::Text(key.into()))
    }

    pub fn all(table: impl Into<String>) -> GetPage {
        GetPage::new(table)
    }

    pub fn count(table: impl Into<String>) -> GetCount {
        GetCount::table(table)
    }
}

pub mod change {
    use std::collections::BTreeMap;

    use super::{Record, Value, WriteChange};

    pub fn add(record: Record) -> WriteChange {
        WriteChange::add_record(record)
    }

    pub fn set(record: Record) -> WriteChange {
        WriteChange::set_record(record)
    }

    pub fn edit_id(
        table: impl Into<String>,
        id: impl Into<String>,
        value: BTreeMap<String, Value>,
    ) -> WriteChange {
        WriteChange::edit(table, "id", Value::Id(id.into()), value)
    }

    pub fn remove_id(table: impl Into<String>, id: impl Into<String>) -> WriteChange {
        WriteChange::remove(table, "id", Value::Id(id.into()))
    }
}

/// Plan-level validation and execution errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    Invalid(String),
    NotFound(String),
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => write!(formatter, "{message}"),
            Self::NotFound(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for PlanError {}

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

    /// Gets the current state for one declarative selector.
    pub fn get<R: GetRequest>(&self, request: R) -> Result<R::Output, TensackError> {
        let outcome = self.execute_plan(request.into_plan()?)?;
        R::from_outcome(outcome)
    }

    /// Applies one declarative state change.
    pub fn write<W: WriteRequest>(&self, request: W) -> Result<W::Output, TensackError> {
        let outcome = self.execute_plan(request.into_plan()?)?;
        W::from_outcome(outcome)
    }

    /// Applies multiple state changes for one table in one storage batch.
    pub fn write_many(&self, changes: &[WriteChange]) -> Result<Vec<AppendResult>, TensackError> {
        let plans = changes
            .iter()
            .cloned()
            .map(WriteChange::into_plan)
            .collect::<Vec<_>>();
        let Some(first) = plans.first() else {
            return Ok(Vec::new());
        };
        if plans.iter().any(|plan| plan.table != first.table) {
            return Err(PlanError::Invalid(
                "write_many changes must all belong to the same table".to_owned(),
            )
            .into());
        }
        if plans.iter().all(|plan| plan.op == PlanOp::Insert) {
            let records = plans
                .iter()
                .map(|plan| {
                    let table = self.schema.table(&plan.table).ok_or_else(|| {
                        PlanError::Invalid(format!("unknown table `{}`", plan.table))
                    })?;
                    let record = self.record_from_plan_value(table.name(), &plan.value)?;
                    self.schema.validate_record(&record)?;
                    Ok(record)
                })
                .collect::<Result<Vec<_>, TensackError>>()?;
            return self.insert_many(&records);
        }
        if plans.iter().any(|plan| plan.op == PlanOp::Insert) {
            return Err(PlanError::Invalid(
                "write_many insert changes cannot be mixed with other changes; use insert_many"
                    .to_owned(),
            )
            .into());
        }

        let mut operations = Vec::with_capacity(plans.len());
        let mut touched_ids = BTreeSet::new();
        for plan in plans {
            let table = self
                .schema
                .table(&plan.table)
                .ok_or_else(|| PlanError::Invalid(format!("unknown table `{}`", plan.table)))?;
            match plan.op {
                PlanOp::Upsert => {
                    let record = self.record_from_plan_value(table.name(), &plan.value)?;
                    self.schema.validate_record(&record)?;
                    let id = record_id(&record)?;
                    if !touched_ids.insert(id.clone()) {
                        return Err(PlanError::Invalid(format!(
                            "write_many touches row `{id}` more than once"
                        ))
                        .into());
                    }
                    operations.push(AppendOperation::put(record));
                }
                PlanOp::Patch => {
                    validate_patch_plan(table, &plan)?;
                    let mut row = self.require_unique_row(&plan)?;
                    let id = record_id(&row)?;
                    if !touched_ids.insert(id.clone()) {
                        return Err(PlanError::Invalid(format!(
                            "write_many touches row `{id}` more than once"
                        ))
                        .into());
                    }
                    for (name, value) in plan.value {
                        row.insert_field(name, value)?;
                    }
                    self.schema.validate_record(&row)?;
                    operations.push(AppendOperation::put(row));
                }
                PlanOp::Remove => {
                    let row = self.require_unique_row(&plan)?;
                    let id = record_id(&row)?;
                    if !touched_ids.insert(id.clone()) {
                        return Err(PlanError::Invalid(format!(
                            "write_many touches row `{id}` more than once"
                        ))
                        .into());
                    }
                    let mut record = Record::new(table.name());
                    record.insert_id(id);
                    operations.push(AppendOperation::delete(record));
                }
                PlanOp::Get | PlanOp::Find | PlanOp::Scan | PlanOp::Count | PlanOp::Insert => {
                    return Err(PlanError::Invalid(
                        "write_many only accepts state changes".to_owned(),
                    )
                    .into());
                }
            }
        }

        self.store
            .append_many(&self.schema, &operations)
            .map_err(TensackError::from)
    }

    /// Inserts a new row. Fails if the id already exists.
    pub fn insert(&self, record: &Record) -> Result<AppendResult, TensackError> {
        self.write(WriteChange::add_record(record.clone()))
    }

    /// Inserts multiple new rows for one table in one storage batch.
    pub fn insert_many(&self, records: &[Record]) -> Result<Vec<AppendResult>, TensackError> {
        let Some(first) = records.first() else {
            return Ok(Vec::new());
        };
        for record in records {
            if record.table() != first.table() {
                return Err(PlanError::Invalid(
                    "insert_many records must all belong to the same table".to_owned(),
                )
                .into());
            }
            self.schema.validate_record(record)?;
        }
        self.store
            .append_insert_many(&self.schema, records)
            .map_err(TensackError::from)
    }

    /// Writes a replacement row, or inserts it if it does not exist.
    pub fn put(&self, record: &Record) -> Result<AppendResult, TensackError> {
        self.upsert(record)
    }

    /// Writes a replacement row, or inserts it if it does not exist.
    pub fn upsert(&self, record: &Record) -> Result<AppendResult, TensackError> {
        self.write(WriteChange::set_record(record.clone()))
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

    /// Compatibility path for reading the current live row by table id.
    pub fn get_by_id(&self, table_name: &str, id: &str) -> Result<Option<Record>, TensackError> {
        self.get(selector::id(table_name, id))
    }

    /// Reads the first current live row matching a lookup key.
    pub fn get_by(
        &self,
        table_name: &str,
        lookup_field: &str,
        key: &str,
    ) -> Result<Option<Record>, TensackError> {
        match self.execute_plan(
            PlanEnvelope::new(PlanOp::Get, table_name)
                .with_lookup(lookup_field)
                .with_key(lookup_field, Value::Text(key.to_owned())),
        )? {
            PlanOutcome::Row(row) => Ok(row),
            _ => unreachable!("get plans return row results"),
        }
    }

    /// Reads all current live rows matching a lookup key.
    pub fn get_many_by(
        &self,
        table_name: &str,
        lookup_field: &str,
        key: &str,
    ) -> Result<Vec<Record>, TensackError> {
        match self.execute_plan(
            PlanEnvelope::new(PlanOp::Find, table_name)
                .with_lookup(lookup_field)
                .with_key(lookup_field, Value::Text(key.to_owned()))
                .with_limit(MAX_PLAN_LIMIT),
        )? {
            PlanOutcome::Rows(page) => Ok(page.rows),
            _ => unreachable!("find plans return row pages"),
        }
    }

    /// Applies a partial update to one row addressed by a unique lookup.
    pub fn patch_by_id(
        &self,
        table_name: &str,
        id: &str,
        patch: BTreeMap<String, Value>,
    ) -> Result<AppendResult, TensackError> {
        match self.execute_plan(PlanEnvelope {
            op: PlanOp::Patch,
            table: table_name.to_owned(),
            lookup: Some("id".to_owned()),
            key: BTreeMap::from([("id".to_owned(), Value::Id(id.to_owned()))]),
            value: patch,
            limit: None,
            cursor: None,
        })? {
            PlanOutcome::Append(result) => Ok(result),
            _ => unreachable!("patch plans return append results"),
        }
    }

    /// Reads live rows from a table without a lookup.
    pub fn scan(
        &self,
        table_name: &str,
        limit: Option<usize>,
        cursor: Option<&str>,
    ) -> Result<PlanPage, TensackError> {
        let mut plan = PlanEnvelope::new(PlanOp::Scan, table_name);
        plan.limit = limit;
        plan.cursor = cursor.map(str::to_owned);
        match self.execute_plan(plan)? {
            PlanOutcome::Rows(page) => Ok(page),
            _ => unreachable!("scan plans return row pages"),
        }
    }

    /// Counts current live rows in a table.
    pub fn count(&self, table_name: &str) -> Result<usize, TensackError> {
        match self.execute_plan(PlanEnvelope::new(PlanOp::Count, table_name))? {
            PlanOutcome::Count(count) => Ok(count),
            _ => unreachable!("count plans return counts"),
        }
    }

    /// Executes one validated internal plan envelope.
    pub fn execute_plan(&self, plan: PlanEnvelope) -> Result<PlanOutcome, TensackError> {
        let table = self
            .schema
            .table(&plan.table)
            .ok_or_else(|| PlanError::Invalid(format!("unknown table `{}`", plan.table)))?;

        match plan.op {
            PlanOp::Insert => {
                let record = self.record_from_plan_value(table.name(), &plan.value)?;
                self.schema.validate_record(&record)?;
                self.store
                    .append_insert(&self.schema, &record)
                    .map(PlanOutcome::Append)
                    .map_err(TensackError::from)
            }
            PlanOp::Upsert => {
                let record = self.record_from_plan_value(table.name(), &plan.value)?;
                self.schema.validate_record(&record)?;
                self.store
                    .append_put(&self.schema, &record)
                    .map(PlanOutcome::Append)
                    .map_err(TensackError::from)
            }
            PlanOp::Patch => {
                validate_patch_plan(table, &plan)?;
                let mut row = self.require_unique_row(&plan)?;
                for (name, value) in plan.value {
                    row.insert_field(name, value)?;
                }
                self.schema.validate_record(&row)?;
                self.store
                    .append_put(&self.schema, &row)
                    .map(PlanOutcome::Append)
                    .map_err(TensackError::from)
            }
            PlanOp::Remove => {
                let row = self.require_unique_row(&plan)?;
                let id = record_id(&row)?;
                self.store
                    .append_delete_id(&self.schema, table.name(), &id)
                    .map(PlanOutcome::Append)
                    .map_err(TensackError::from)
            }
            PlanOp::Get => self.optional_unique_row(&plan).map(PlanOutcome::Row),
            PlanOp::Find => {
                let lookup = self.require_lookup_name(&plan)?;
                self.require_declared_lookup(table, &lookup)?;
                let key = self.require_lookup_key(&plan, &lookup)?;
                let limit = checked_limit(plan.limit)?;
                let cursor = checked_cursor(plan.cursor.as_deref())?;
                let count = self
                    .store
                    .count_lookup(&self.schema, table.name(), &lookup, &key)?;
                let mut rows =
                    self.store
                        .get_by_lookup(&self.schema, table.name(), &lookup, &key)?;
                rows = rows.into_iter().skip(cursor).take(limit).collect();
                let next_cursor = next_cursor(cursor, limit, count);
                Ok(PlanOutcome::Rows(PlanPage { rows, next_cursor }))
            }
            PlanOp::Scan => {
                let limit = checked_limit(plan.limit)?;
                let cursor = checked_cursor(plan.cursor.as_deref())?;
                let count = self.store.count_table(&self.schema, table.name())?;
                let rows = self
                    .store
                    .scan_table(&self.schema, table.name(), limit, cursor)?;
                let next_cursor = next_cursor(cursor, limit, count);
                Ok(PlanOutcome::Rows(PlanPage { rows, next_cursor }))
            }
            PlanOp::Count => {
                if let Some(lookup) = plan.lookup.as_deref() {
                    self.require_declared_lookup(table, lookup)?;
                    let key = self.require_lookup_key(&plan, lookup)?;
                    self.store
                        .count_lookup(&self.schema, table.name(), lookup, &key)
                        .map(PlanOutcome::Count)
                        .map_err(TensackError::from)
                } else {
                    self.store
                        .count_table(&self.schema, table.name())
                        .map(PlanOutcome::Count)
                        .map_err(TensackError::from)
                }
            }
        }
    }

    /// Rebuilds the generated `.tenb` cache for one table from canonical `.ten`.
    pub fn rebuild_cache(&self, table_name: &str) -> Result<(), TensackError> {
        self.store
            .rebuild_tenb(&self.schema, table_name)
            .map(|_| ())
            .map_err(TensackError::from)
    }

    fn record_from_plan_value(
        &self,
        table_name: &str,
        value: &BTreeMap<String, Value>,
    ) -> Result<Record, TensackError> {
        let mut record = Record::new(table_name);
        for (name, value) in value {
            record.insert_field(name, value.clone())?;
        }
        Ok(record)
    }

    fn optional_unique_row(&self, plan: &PlanEnvelope) -> Result<Option<Record>, TensackError> {
        let table = self
            .schema
            .table(&plan.table)
            .ok_or_else(|| PlanError::Invalid(format!("unknown table `{}`", plan.table)))?;
        let lookup = self.require_lookup_name(plan)?;
        self.require_unique_lookup(table, &lookup)?;
        let key = self.require_lookup_key(plan, &lookup)?;
        self.store
            .get_unique_lookup(&self.schema, table.name(), &lookup, &key)
            .map_err(TensackError::from)
    }

    fn require_unique_row(&self, plan: &PlanEnvelope) -> Result<Record, TensackError> {
        self.optional_unique_row(plan)?.ok_or_else(|| {
            PlanError::NotFound(format!(
                "row not found in `{}` for unique lookup `{}`",
                plan.table,
                plan.lookup.as_deref().unwrap_or("<missing>")
            ))
            .into()
        })
    }

    fn require_lookup_name(&self, plan: &PlanEnvelope) -> Result<String, PlanError> {
        plan.lookup
            .clone()
            .ok_or_else(|| PlanError::Invalid("plan missing lookup".to_owned()))
    }

    fn require_lookup_key(&self, plan: &PlanEnvelope, lookup: &str) -> Result<String, PlanError> {
        let value = plan
            .key
            .get(lookup)
            .ok_or_else(|| PlanError::Invalid(format!("plan missing key `{lookup}`")))?;
        Ok(value_to_lookup_key(value))
    }

    fn require_unique_lookup(&self, table: &TableSchema, lookup: &str) -> Result<(), PlanError> {
        if lookup == "id" {
            return Ok(());
        }
        let spec = table.lookup(lookup).ok_or_else(|| {
            PlanError::Invalid(format!(
                "unknown lookup `{lookup}` for table `{}`",
                table.name()
            ))
        })?;
        if !spec.unique() {
            return Err(PlanError::Invalid(format!(
                "lookup `{lookup}` for table `{}` is not unique",
                table.name()
            )));
        }
        Ok(())
    }

    fn require_declared_lookup(&self, table: &TableSchema, lookup: &str) -> Result<(), PlanError> {
        if lookup == "id" || table.lookup(lookup).is_some() {
            return Ok(());
        }
        Err(PlanError::Invalid(format!(
            "unknown lookup `{lookup}` for table `{}`",
            table.name()
        )))
    }
}

fn checked_limit(limit: Option<usize>) -> Result<usize, PlanError> {
    let limit = limit.unwrap_or(DEFAULT_PLAN_LIMIT);
    if limit == 0 || limit > MAX_PLAN_LIMIT {
        return Err(PlanError::Invalid(format!(
            "limit must be between 1 and {MAX_PLAN_LIMIT}"
        )));
    }
    Ok(limit)
}

fn checked_cursor(cursor: Option<&str>) -> Result<usize, PlanError> {
    match cursor {
        Some(value) if !value.is_empty() => value
            .parse::<usize>()
            .map_err(|error| PlanError::Invalid(format!("invalid cursor: {error}"))),
        _ => Ok(0),
    }
}

fn next_cursor(offset: usize, limit: usize, total: usize) -> Option<String> {
    let next = offset.saturating_add(limit);
    (next < total).then(|| next.to_string())
}

fn validate_patch_plan(table: &TableSchema, plan: &PlanEnvelope) -> Result<(), PlanError> {
    if plan.value.is_empty() {
        return Err(PlanError::Invalid("patch value cannot be empty".to_owned()));
    }
    if plan.value.contains_key("id") {
        return Err(PlanError::Invalid("patch cannot change id".to_owned()));
    }
    for field in plan.value.keys() {
        if table.field(field).is_none() {
            return Err(PlanError::Invalid(format!(
                "unknown field `{field}` for table `{}`",
                table.name()
            )));
        }
    }
    Ok(())
}

fn value_to_lookup_key(value: &Value) -> String {
    match value {
        Value::Id(value) | Value::Text(value) => value.clone(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
    }
}

fn record_id(record: &Record) -> Result<String, TensackError> {
    match record.fields().get("id") {
        Some(Value::Id(value)) | Some(Value::Text(value)) => Ok(value.clone()),
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
        assert!(db_dir.join("tables/notebooks").exists());
        assert!(db_dir.join("tables/notes").exists());
        assert!(db_dir.join("engine/notebooks.tenb").exists());
        assert!(db_dir.join("engine/notes.tenb").exists());

        let metadata = fs::read_to_string(db_dir.join("tensack.toml")).unwrap();
        assert!(metadata.contains("[tables.notebooks]"));
        assert!(metadata.contains("[tables.notes]"));
        assert!(metadata.contains("next_chunk = 0"));
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
        assert!(db_dir.join("tables/messages/zz/zzz.ten").exists());
        assert!(db_dir.join("tables/messages/zz/zzy.ten").exists());
        assert!(db_dir.join("tensack.toml").exists());
        assert!(db_dir.join("engine/messages.tenb").exists());
        let first_chunk = fs::read_to_string(db_dir.join("tables/messages/zz/zzz.ten")).unwrap();
        let second_chunk = fs::read_to_string(db_dir.join("tables/messages/zz/zzy.ten")).unwrap();
        assert!(first_chunk.starts_with("TEN\t1\ttable\tmessages\t"));
        assert!(first_chunk.contains("R\t1\tm1\thello\n"));
        assert!(second_chunk.contains("R\t2\tm2\tworld\n"));
        assert_eq!(
            db.get(selector::id("messages", "m1"))
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
    fn get_and_write_accept_declarative_requests() {
        let root = temp_root();
        let db = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema());
        let row = Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", "hello")
            .unwrap();

        db.write(change::add(row)).unwrap();
        let found = db.get(selector::id("messages", "m1")).unwrap().unwrap();
        assert_eq!(
            found.fields().get("body"),
            Some(&Value::Text("hello".to_owned()))
        );

        db.write(change::edit_id(
            "messages",
            "m1",
            BTreeMap::from([("body".to_owned(), Value::Text("updated".to_owned()))]),
        ))
        .unwrap();
        let updated = db.get(selector::id("messages", "m1")).unwrap().unwrap();
        assert_eq!(
            updated.fields().get("body"),
            Some(&Value::Text("updated".to_owned()))
        );

        db.write(change::remove_id("messages", "m1")).unwrap();
        assert!(db.get(selector::id("messages", "m1")).unwrap().is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn insert_many_batches_rows_into_one_chunk() {
        let root = temp_root();
        let db = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema());
        let rows = vec![
            Record::new("messages")
                .with_id("m1")
                .unwrap()
                .with_field("body", "hello")
                .unwrap(),
            Record::new("messages")
                .with_id("m2")
                .unwrap()
                .with_field("body", "world")
                .unwrap(),
        ];

        let results = db.insert_many(&rows).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].tx_id, 1);
        assert_eq!(results[1].tx_id, 2);
        assert!(root.join("chat/tables/messages/zz/zzz.ten").exists());
        assert!(!root.join("chat/tables/messages/zz/zzy.ten").exists());
        let chunk = fs::read_to_string(root.join("chat/tables/messages/zz/zzz.ten")).unwrap();
        assert!(chunk.contains("R\t1\tm1\thello\n"));
        assert!(chunk.contains("R\t2\tm2\tworld\n"));
        assert_eq!(db.count("messages").unwrap(), 2);
        assert_eq!(
            db.get(selector::id("messages", "m2"))
                .unwrap()
                .unwrap()
                .fields()
                .get("body"),
            Some(&Value::Text("world".to_owned()))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn insert_many_rejects_duplicate_ids_before_append() {
        let root = temp_root();
        let db = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema());
        let rows = vec![
            Record::new("messages")
                .with_id("m1")
                .unwrap()
                .with_field("body", "hello")
                .unwrap(),
            Record::new("messages")
                .with_id("m1")
                .unwrap()
                .with_field("body", "world")
                .unwrap(),
        ];

        assert!(db.insert_many(&rows).is_err());
        assert!(!root.join("chat/tables/messages/zz/zzz.ten").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn insert_many_rejects_unique_lookup_duplicates_before_append() {
        let root = temp_root();
        let mut schema = DatabaseSchema::new();
        let mut users = TableSchema::new("users");
        users.add_field("id", PrimitiveType::Id).unwrap();
        users.add_field("email", PrimitiveType::Text).unwrap();
        users.add_field("name", PrimitiveType::Text).unwrap();
        users.add_lookup("email", true).unwrap();
        schema.add_table(users).unwrap();
        let db = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema);
        let rows = vec![
            Record::new("users")
                .with_id("u1")
                .unwrap()
                .with_field("email", "same@test.com")
                .unwrap()
                .with_field("name", "Ada")
                .unwrap(),
            Record::new("users")
                .with_id("u2")
                .unwrap()
                .with_field("email", "same@test.com")
                .unwrap()
                .with_field("name", "Ben")
                .unwrap(),
        ];

        assert!(db.insert_many(&rows).is_err());
        assert!(!root.join("chat/tables/users/zz/zzz.ten").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn write_many_batches_patches_into_one_chunk() {
        let root = temp_root();
        let db = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema());
        let rows = vec![
            Record::new("messages")
                .with_id("m1")
                .unwrap()
                .with_field("body", "hello")
                .unwrap(),
            Record::new("messages")
                .with_id("m2")
                .unwrap()
                .with_field("body", "world")
                .unwrap(),
        ];
        db.insert_many(&rows).unwrap();

        let results = db
            .write_many(&[
                change::edit_id(
                    "messages",
                    "m1",
                    BTreeMap::from([("body".to_owned(), Value::Text("first".to_owned()))]),
                ),
                change::edit_id(
                    "messages",
                    "m2",
                    BTreeMap::from([("body".to_owned(), Value::Text("second".to_owned()))]),
                ),
            ])
            .unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].tx_id, 3);
        assert_eq!(results[1].tx_id, 4);
        assert!(root.join("chat/tables/messages/zz/zzy.ten").exists());
        assert!(!root.join("chat/tables/messages/zz/zzx.ten").exists());
        let chunk = fs::read_to_string(root.join("chat/tables/messages/zz/zzy.ten")).unwrap();
        assert!(chunk.contains("R\t3\tm1\tfirst\n"));
        assert!(chunk.contains("R\t4\tm2\tsecond\n"));
        assert_eq!(
            db.get(selector::id("messages", "m2"))
                .unwrap()
                .unwrap()
                .fields()
                .get("body"),
            Some(&Value::Text("second".to_owned()))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn write_many_batches_removes_into_one_chunk() {
        let root = temp_root();
        let db = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema());
        let rows = vec![
            Record::new("messages")
                .with_id("m1")
                .unwrap()
                .with_field("body", "hello")
                .unwrap(),
            Record::new("messages")
                .with_id("m2")
                .unwrap()
                .with_field("body", "world")
                .unwrap(),
        ];
        db.insert_many(&rows).unwrap();

        let results = db
            .write_many(&[
                change::remove_id("messages", "m1"),
                change::remove_id("messages", "m2"),
            ])
            .unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].tx_id, 3);
        assert_eq!(results[1].tx_id, 4);
        assert_eq!(db.count("messages").unwrap(), 0);
        assert!(root.join("chat/tables/messages/zz/zzy.ten").exists());
        assert!(!root.join("chat/tables/messages/zz/zzx.ten").exists());
        let chunk = fs::read_to_string(root.join("chat/tables/messages/zz/zzy.ten")).unwrap();
        assert!(chunk.contains("D\t3\tm1\n"));
        assert!(chunk.contains("D\t4\tm2\n"));
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
            db.get(selector::id("messages", "m1"))
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
            .with_field("conversation_id", tensack_core::Value::Id("cv1".to_owned()))
            .unwrap()
            .with_field("body", "hello")
            .unwrap();
        let second = Record::new("messages")
            .with_id("m2")
            .unwrap()
            .with_field("conversation_id", tensack_core::Value::Id("cv1".to_owned()))
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
            db.get(selector::id("messages", "m2"))
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
    fn fresh_handle_uses_and_rebuilds_generated_cache() {
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

        let db = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema.clone());
        let row = Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("conversation_id", Value::Id("cv1".to_owned()))
            .unwrap()
            .with_field("body", "hello")
            .unwrap();
        db.insert(&row).unwrap();

        let reopened =
            TensackDatabase::open_local_with_schema(root.clone(), "chat", schema.clone());
        let rows = reopened
            .get_many_by("messages", "conversation_id", "cv1")
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].fields().get("body"),
            Some(&Value::Text("hello".to_owned()))
        );

        fs::remove_file(root.join("chat/engine/messages.tenb")).unwrap();
        let cold = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema);
        assert_eq!(cold.count("messages").unwrap(), 1);
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
        assert!(db.get(selector::id("messages", "m1")).unwrap().is_some());
        db.delete_by_id("messages", "m1").unwrap();
        assert!(db.get(selector::id("messages", "m1")).unwrap().is_none());

        let delete_chunk =
            fs::read_to_string(root.join("chat/tables/messages/zz/zzy.ten")).unwrap();
        assert!(delete_chunk.contains("D\t2\tm1\n"));
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
        let first_chunk = fs::read_to_string(root.join("chat/tables/users/zz/zzz.ten")).unwrap();
        assert!(!first_chunk.contains("u2\tsame@test.com"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn plan_executor_patches_rows_and_preserves_fields() {
        let root = temp_root();
        let db = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema());
        let row = Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", "hello")
            .unwrap();

        db.insert(&row).unwrap();
        let result = db
            .patch_by_id(
                "messages",
                "m1",
                BTreeMap::from([("body".to_owned(), Value::Text("updated".to_owned()))]),
            )
            .unwrap();

        assert_eq!(result.tx_id, 2);
        let updated = db.get(selector::id("messages", "m1")).unwrap().unwrap();
        assert_eq!(
            updated.fields().get("body"),
            Some(&Value::Text("updated".to_owned()))
        );
        assert_eq!(
            updated.fields().get("id"),
            Some(&Value::Id("m1".to_owned()))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn plan_executor_removes_by_unique_lookup() {
        let root = temp_root();
        let mut schema = DatabaseSchema::new();
        let mut users = TableSchema::new("users");
        users.add_field("id", PrimitiveType::Id).unwrap();
        users.add_field("email", PrimitiveType::Text).unwrap();
        users.add_field("name", PrimitiveType::Text).unwrap();
        users.add_lookup("email", true).unwrap();
        schema.add_table(users).unwrap();
        let db = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema);
        let row = Record::new("users")
            .with_id("u1")
            .unwrap()
            .with_field("email", "a@test.com")
            .unwrap()
            .with_field("name", "Ada")
            .unwrap();

        db.insert(&row).unwrap();
        let plan = PlanEnvelope::new(PlanOp::Remove, "users")
            .with_lookup("email")
            .with_key("email", Value::Text("a@test.com".to_owned()));
        db.execute_plan(plan).unwrap();

        assert!(db.get(selector::id("users", "u1")).unwrap().is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn plan_executor_scans_and_counts_live_rows() {
        let root = temp_root();
        let db = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema());
        for id in ["m1", "m2", "m3"] {
            let row = Record::new("messages")
                .with_id(id)
                .unwrap()
                .with_field("body", format!("body-{id}"))
                .unwrap();
            db.insert(&row).unwrap();
        }
        db.delete_by_id("messages", "m2").unwrap();

        assert_eq!(db.count("messages").unwrap(), 2);
        let first = db.scan("messages", Some(1), None).unwrap();
        assert_eq!(first.rows.len(), 1);
        assert_eq!(first.next_cursor, Some("1".to_owned()));
        let second = db
            .scan("messages", Some(1), first.next_cursor.as_deref())
            .unwrap();
        assert_eq!(second.rows.len(), 1);
        assert_eq!(second.next_cursor, None);
        let ids: Vec<_> = [first.rows, second.rows]
            .concat()
            .into_iter()
            .map(|row| record_id(&row).unwrap())
            .collect();
        assert_eq!(ids, vec!["m1".to_owned(), "m3".to_owned()]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn plan_executor_rejects_bad_patch() {
        let root = temp_root();
        let db = TensackDatabase::open_local_with_schema(root.clone(), "chat", schema());
        let row = Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", "hello")
            .unwrap();
        db.insert(&row).unwrap();

        let err = db
            .patch_by_id(
                "messages",
                "m1",
                BTreeMap::from([("id".to_owned(), Value::Id("m2".to_owned()))]),
            )
            .unwrap_err();

        assert!(err.to_string().contains("patch cannot change id"));
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
