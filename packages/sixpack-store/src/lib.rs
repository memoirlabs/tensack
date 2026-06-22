//! Local storage engine boundary.
//!
//! The current store writes readable `.6` table row segments, keeps a small
//! `sixpack.toml` physical layout map, and rebuilds generated `.6b` lookup
//! caches from canonical `.6` data.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use sixpack_core::{DatabaseSchema, Record, TableSchema, Value};
use sixpack_format::{
    Operation, RowPointer, SIXB_BINARY_VERSION, SixOperationRecord, SixbCache, SixbLookupEntry,
    SixbRowEntry, decode_six_operation, decode_six_row, decode_sixb_cache, encode_six_header,
    encode_six_operation, encode_six_preamble, encode_sixb_cache, is_six_magic_line, source_hash,
};

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;
const CHUNK_CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
const CHUNK_BASE: usize = 36;
const CHUNK_WIDTH: usize = 3;
const MAX_CHUNKS: u64 = 36u64.pow(CHUNK_WIDTH as u32);
const MAX_SIX_CHUNK_BYTES: u64 = 1024 * 1024;

type TableName = String;
type RowId = String;
type ChunkName = String;
type CachedChunk = Arc<Vec<u8>>;

/// Local store handle.
#[derive(Clone)]
pub struct LocalStore {
    root: PathBuf,
    workspace: String,
    sixb_cache: Arc<RwLock<BTreeMap<TableName, Arc<SixbCache>>>>,
    runtime_sixb_cache: Arc<RwLock<BTreeMap<TableName, RuntimeSixb>>>,
    row_cache: Arc<RwLock<BTreeMap<TableName, BTreeMap<RowId, Record>>>>,
    chunk_cache: Arc<RwLock<BTreeMap<(TableName, ChunkName), CachedChunk>>>,
    chunk_len_cache: Arc<RwLock<BTreeMap<(TableName, ChunkName), u64>>>,
    next_tx_cache: Arc<RwLock<Option<u64>>>,
    next_chunk_cache: Arc<RwLock<BTreeMap<String, u64>>>,
    layout_cache: Arc<RwLock<BTreeSet<(String, String)>>>,
    table_writers: Arc<RwLock<BTreeMap<String, Arc<Mutex<()>>>>>,
}

impl std::fmt::Debug for LocalStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalStore")
            .field("root", &self.root)
            .field("workspace", &self.workspace)
            .finish_non_exhaustive()
    }
}

impl PartialEq for LocalStore {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root && self.workspace == other.workspace
    }
}

impl Eq for LocalStore {}

/// Result of appending one logical row entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppendResult {
    /// Assigned transaction id.
    pub tx_id: u64,
    /// Operation used.
    pub operation: Operation,
    /// Bytes written to the `.6` row segment (line + newline).
    pub bytes_written: u64,
}

/// One append-only `.6` operation for a batch write.
#[derive(Debug, Clone, PartialEq)]
pub struct AppendOperation {
    /// Operation to append.
    pub operation: Operation,
    /// Row-like record. Delete operations only require the `id` field.
    pub record: Record,
}

impl AppendOperation {
    /// Creates an append operation.
    pub fn new(operation: Operation, record: Record) -> Self {
        Self { operation, record }
    }

    /// Creates a put operation.
    pub fn put(record: Record) -> Self {
        Self::new(Operation::Put, record)
    }

    /// Creates a delete operation.
    pub fn delete(record: Record) -> Self {
        Self::new(Operation::Delete, record)
    }
}

/// Write-batch conflict mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteBatchMode {
    /// Every put must create a new live id.
    InsertOnly,
    /// Puts replace existing rows and deletes tombstone existing ids.
    Upsert,
}

/// Validated set of operations for one table.
#[derive(Debug, Clone, PartialEq)]
pub struct WriteBatch {
    table: String,
    mode: WriteBatchMode,
    operations: Vec<AppendOperation>,
}

impl WriteBatch {
    /// Creates an empty batch for one table.
    pub fn new(table: impl Into<String>, mode: WriteBatchMode) -> Self {
        Self {
            table: table.into(),
            mode,
            operations: Vec::new(),
        }
    }

    /// Creates an insert-only batch of put operations.
    pub fn insert_only(
        table: impl Into<String>,
        records: impl IntoIterator<Item = Record>,
    ) -> io::Result<Self> {
        let mut batch = Self::new(table, WriteBatchMode::InsertOnly);
        for record in records {
            batch.push(AppendOperation::put(record))?;
        }
        Ok(batch)
    }

    /// Creates an upsert batch from append operations.
    pub fn upsert(
        table: impl Into<String>,
        operations: impl IntoIterator<Item = AppendOperation>,
    ) -> io::Result<Self> {
        let mut batch = Self::new(table, WriteBatchMode::Upsert);
        for operation in operations {
            batch.push(operation)?;
        }
        Ok(batch)
    }

    /// Adds one operation, rejecting cross-table batches.
    pub fn push(&mut self, operation: AppendOperation) -> io::Result<()> {
        if operation.record.table() != self.table {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "write batch operations must all belong to the same table",
            ));
        }
        self.operations.push(operation);
        Ok(())
    }

    /// Returns the table name.
    pub fn table(&self) -> &str {
        &self.table
    }

    /// Returns the conflict mode.
    pub fn mode(&self) -> WriteBatchMode {
        self.mode
    }

    /// Returns the operations.
    pub fn operations(&self) -> &[AppendOperation] {
        &self.operations
    }

    /// Returns whether the batch is empty.
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }
}

impl LocalStore {
    /// Creates a store handle without touching the filesystem.
    pub fn new(root: impl Into<PathBuf>, workspace: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            workspace: workspace.into(),
            sixb_cache: Arc::new(RwLock::new(BTreeMap::new())),
            runtime_sixb_cache: Arc::new(RwLock::new(BTreeMap::new())),
            row_cache: Arc::new(RwLock::new(BTreeMap::new())),
            chunk_cache: Arc::new(RwLock::new(BTreeMap::new())),
            chunk_len_cache: Arc::new(RwLock::new(BTreeMap::new())),
            next_tx_cache: Arc::new(RwLock::new(None)),
            next_chunk_cache: Arc::new(RwLock::new(BTreeMap::new())),
            layout_cache: Arc::new(RwLock::new(BTreeSet::new())),
            table_writers: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    /// Returns the store root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the workspace name associated with this store.
    pub fn workspace(&self) -> &str {
        &self.workspace
    }

    /// Database directory for this workspace.
    pub fn database_dir(&self) -> PathBuf {
        self.root.join(&self.workspace)
    }

    /// Root `sixpack.toml` metadata path.
    pub fn metadata_path(&self) -> PathBuf {
        self.database_dir().join("sixpack.toml")
    }

    /// Table directory.
    pub fn table_dir(&self, table: &str) -> PathBuf {
        self.database_dir().join("tables").join(table)
    }

    /// Deterministic `.6` chunk path for a table counter.
    pub fn chunk_six_path(&self, table: &str, chunk_counter: u64) -> io::Result<PathBuf> {
        Ok(self.table_dir(table).join(chunk_path(chunk_counter)?))
    }

    /// Generated binary cache for table indexes/lookups.
    pub fn sixb_path(&self, table: &str) -> PathBuf {
        self.database_dir()
            .join("engine")
            .join(format!("{table}.6b"))
    }

    /// Optional generated full-text search index path.
    pub fn sixx_path(&self, table: &str) -> PathBuf {
        self.database_dir()
            .join("engine")
            .join(format!("{table}.6x"))
    }

    /// Appends a put event to the `.6` table segment.
    pub fn append_put(&self, schema: &DatabaseSchema, record: &Record) -> io::Result<AppendResult> {
        self.append(schema, Operation::Put, record)
    }

    /// Appends a put only when the id is not already live.
    pub fn append_insert(
        &self,
        schema: &DatabaseSchema,
        record: &Record,
    ) -> io::Result<AppendResult> {
        let batch = WriteBatch::insert_only(record.table(), [record.clone()])?;
        one_append_result(self.append_batch(schema, &batch)?)
    }

    /// Appends multiple puts to one table only when every id is new.
    pub fn append_insert_many(
        &self,
        schema: &DatabaseSchema,
        records: &[Record],
    ) -> io::Result<Vec<AppendResult>> {
        let Some(first) = records.first() else {
            return Ok(Vec::new());
        };
        let batch = WriteBatch::insert_only(first.table(), records.iter().cloned())?;
        self.append_batch(schema, &batch)
    }

    /// Appends multiple operations to one table in one `.6` chunk.
    pub fn append_many(
        &self,
        schema: &DatabaseSchema,
        operations: &[AppendOperation],
    ) -> io::Result<Vec<AppendResult>> {
        let Some(first) = operations.first() else {
            return Ok(Vec::new());
        };
        let batch = WriteBatch::upsert(first.record.table(), operations.iter().cloned())?;
        self.append_batch(schema, &batch)
    }

    /// Appends a prepared write batch to one `.6` chunk.
    pub fn append_batch(
        &self,
        schema: &DatabaseSchema,
        batch: &WriteBatch,
    ) -> io::Result<Vec<AppendResult>> {
        self.append_batch_inner(schema, batch)
    }

    /// Appends a delete event to the `.6` table segment.
    pub fn append_delete(
        &self,
        schema: &DatabaseSchema,
        record: &Record,
    ) -> io::Result<AppendResult> {
        self.append(schema, Operation::Delete, record)
    }

    /// Appends a delete tombstone by id.
    pub fn append_delete_id(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        id: &str,
    ) -> io::Result<AppendResult> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        let mut record = Record::new(table.name());
        record.insert_id(id.to_owned());
        self.append(schema, Operation::Delete, &record)
    }

    /// Appends an operation for a typed record.
    pub fn append(
        &self,
        schema: &DatabaseSchema,
        operation: Operation,
        record: &Record,
    ) -> io::Result<AppendResult> {
        let batch = WriteBatch::upsert(
            record.table(),
            [AppendOperation::new(operation, record.clone())],
        )?;
        one_append_result(self.append_batch(schema, &batch)?)
    }

    /// Creates DB directory layout if needed.
    pub fn ensure_workspace_layout(&self) -> io::Result<()> {
        fs::create_dir_all(self.database_dir().join("tables"))?;
        fs::create_dir_all(self.database_dir().join("engine"))
    }

    /// Initializes an empty database layout for every table in the schema.
    pub fn init(&self, schema: &DatabaseSchema) -> io::Result<()> {
        self.ensure_workspace_layout()?;
        let schema_hash = schema.schema_hash();
        for table in schema.tables().values() {
            self.ensure_table_layout(table, &schema_hash)?;
            self.rebuild_sixb(schema, table.name())?;
        }
        self.write_metadata(schema, self.next_tx_id()?)
    }

    /// Computes next transaction id from private engine metadata.
    pub fn next_tx_id(&self) -> io::Result<u64> {
        if let Some(value) = *self
            .next_tx_cache
            .read()
            .map_err(|_| io::Error::other("next tx cache lock poisoned"))?
        {
            return Ok(value);
        }
        let metadata = self.metadata_path();
        if !metadata.exists() {
            let recovered = self.discovered_next_tx_id()?;
            self.set_next_tx_id(recovered)?;
            return Ok(recovered);
        }
        let file = File::open(metadata)?;
        for line in BufReader::new(file).lines() {
            let line = line?;
            let Some(value) = line.strip_prefix("next_tx = ") else {
                continue;
            };
            let parsed = value.trim().parse::<u64>().map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidData, format!("bad next_tx: {error}"))
            })?;
            let recovered = parsed.max(self.discovered_next_tx_id()?);
            self.set_next_tx_id(recovered)?;
            return Ok(recovered);
        }
        let recovered = self.discovered_next_tx_id()?;
        self.set_next_tx_id(recovered)?;
        Ok(recovered)
    }

    /// Computes the next chunk counter for one table from metadata, falling back to files.
    pub fn next_chunk_counter(&self, table_name: &str) -> io::Result<u64> {
        if let Some(value) = self
            .next_chunk_cache
            .read()
            .map_err(|_| io::Error::other("next chunk cache lock poisoned"))?
            .get(table_name)
            .copied()
        {
            return Ok(value);
        }
        let discovered_next = six_files_in_read_order(&self.table_dir(table_name))?.len() as u64;
        let metadata = self.metadata_path();
        if metadata.exists() {
            let file = File::open(metadata)?;
            let mut in_table = false;
            for line in BufReader::new(file).lines() {
                let line = line?;
                if line.starts_with("[tables.") {
                    in_table = line == format!("[tables.{table_name}]");
                    continue;
                }
                if in_table {
                    let Some(value) = line.strip_prefix("next_chunk = ") else {
                        continue;
                    };
                    let parsed = value
                        .trim()
                        .parse::<u64>()
                        .map_err(|error| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!("bad next_chunk for `{table_name}`: {error}"),
                            )
                        })?
                        .max(discovered_next);
                    self.set_next_chunk_counter(table_name, parsed)?;
                    return Ok(parsed);
                }
            }
        }
        self.set_next_chunk_counter(table_name, discovered_next)?;
        Ok(discovered_next)
    }

    /// Reads all current live records from a table using the generated `.6b` cache.
    pub fn read_table(&self, schema: &DatabaseSchema, table_name: &str) -> io::Result<Vec<Record>> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        if let Some(entries) = self.runtime_row_entries(table_name)? {
            let mut rows = Vec::with_capacity(entries.len());
            for entry in &entries {
                rows.push(self.read_row_entry(table, entry)?);
            }
            return Ok(rows);
        }

        let cache = self.ensure_sixb_snapshot(schema, table_name)?;
        let mut rows = Vec::with_capacity(cache.rows.len());
        for entry in &cache.rows {
            rows.push(self.read_row_entry(table, entry)?);
        }
        Ok(rows)
    }

    /// Reads one row by implicit id lookup.
    pub fn get_by_id(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        id: &str,
    ) -> io::Result<Option<Record>> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        if let Some(entry) = self.runtime_row_entry(table_name, id)? {
            return self.read_row_entry(table, &entry).map(Some);
        }

        let cache = self.ensure_sixb_snapshot(schema, table_name)?;
        let Some(entry) = row_entry_by_id(&cache, id) else {
            return Ok(None);
        };
        self.read_row_entry(table, entry).map(Some)
    }

    /// Reads rows by a declared lookup field. Unique lookup callers should use the first item.
    pub fn get_by_lookup(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        field_name: &str,
        key: &str,
    ) -> io::Result<Vec<Record>> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        if field_name != "id" && table.lookup(field_name).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown lookup `{field_name}` for table `{table_name}`"),
            ));
        }
        if field_name == "id" {
            return self
                .get_by_id(schema, table_name, key)
                .map(|row| row.into_iter().collect());
        }
        if let Some(entries) = self.runtime_lookup_entries(table_name, field_name, key)? {
            let mut rows = Vec::with_capacity(entries.len());
            for lookup_entry in entries {
                if let Some(row_entry) = self.runtime_row_entry(table_name, &lookup_entry.id)? {
                    rows.push(self.read_row_entry(table, &row_entry)?);
                }
            }
            return Ok(rows);
        }

        let cache = self.ensure_sixb_snapshot(schema, table_name)?;
        let mut rows = Vec::new();
        for lookup_entry in lookup_entries_by_key(&cache, field_name, key) {
            if let Some(row_entry) = row_entry_by_id(&cache, &lookup_entry.id) {
                rows.push(self.read_row_entry(table, row_entry)?);
            }
        }
        Ok(rows)
    }

    /// Reads one row by a unique lookup field.
    pub fn get_unique_lookup(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        field_name: &str,
        key: &str,
    ) -> io::Result<Option<Record>> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        if field_name == "id" {
            return self.get_by_id(schema, table_name, key);
        }
        let lookup = table.lookup(field_name).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown lookup `{field_name}` for table `{table_name}`"),
            )
        })?;
        if !lookup.unique() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("lookup `{field_name}` for table `{table_name}` is not unique"),
            ));
        }
        let rows = self.get_by_lookup(schema, table_name, field_name, key)?;
        Ok(rows.into_iter().next())
    }

    /// Reads a page of live rows from a table.
    pub fn scan_table(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        limit: usize,
        offset: usize,
    ) -> io::Result<Vec<Record>> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        if let Some(entries) = self.runtime_row_entries(table_name)? {
            let mut rows = Vec::new();
            for entry in entries.iter().skip(offset).take(limit) {
                rows.push(self.read_row_entry(table, entry)?);
            }
            return Ok(rows);
        }

        let cache = self.ensure_sixb_snapshot(schema, table_name)?;
        let mut rows = Vec::new();
        for entry in cache.rows.iter().skip(offset).take(limit) {
            rows.push(self.read_row_entry(table, entry)?);
        }
        Ok(rows)
    }

    /// Counts current live rows in one table.
    pub fn count_table(&self, schema: &DatabaseSchema, table_name: &str) -> io::Result<usize> {
        schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        if let Some(count) = self.runtime_row_count(table_name)? {
            return Ok(count);
        }
        let cache = self.ensure_sixb_snapshot(schema, table_name)?;
        Ok(cache.rows.len())
    }

    /// Counts current live rows matching a lookup key.
    pub fn count_lookup(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        field_name: &str,
        key: &str,
    ) -> io::Result<usize> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        if field_name != "id" && table.lookup(field_name).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown lookup `{field_name}` for table `{table_name}`"),
            ));
        }
        if field_name == "id" {
            if let Some(entry) = self.runtime_row_entry(table_name, key)? {
                return Ok(usize::from(entry.id == key));
            }
            let cache = self.ensure_sixb_snapshot(schema, table_name)?;
            return Ok(usize::from(row_entry_by_id(&cache, key).is_some()));
        }
        if let Some(count) = self.runtime_lookup_count(table_name, field_name, key)? {
            return Ok(count);
        }
        let cache = self.ensure_sixb_snapshot(schema, table_name)?;
        Ok(lookup_entries_by_key(&cache, field_name, key).len())
    }

    /// Rebuilds the `.6b` cache from canonical `.6` files.
    pub fn rebuild_sixb(&self, schema: &DatabaseSchema, table_name: &str) -> io::Result<SixbCache> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        let scan = self.scan_table_files(table)?;
        let mut rows = Vec::new();
        let mut lookups = Vec::new();

        for (id, live) in &scan.live {
            rows.push(SixbRowEntry {
                id: id.clone(),
                ptr: live.ptr.clone(),
            });
            for lookup in table.lookup_specs_with_implicit_id() {
                let value = live
                    .record
                    .fields()
                    .get(lookup.field_name())
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("missing lookup field `{}`", lookup.field_name()),
                        )
                    })?;
                lookups.push(SixbLookupEntry {
                    field_name: lookup.field_name().to_owned(),
                    key: value_to_lookup_key(value),
                    id: id.clone(),
                });
            }
        }
        validate_unique_lookups(table, &lookups)?;
        sort_sixb_entries(&mut rows, &mut lookups);

        let cache = SixbCache {
            version: SIXB_BINARY_VERSION,
            table: table.name().to_owned(),
            schema_hash: schema.schema_hash(),
            source_hash: scan.source_hash.clone(),
            rows,
            lookups,
        };
        self.remember_table_records(table.name(), scan.live.iter())?;
        self.write_sixb_cache(table.name(), &cache)?;
        self.remember_runtime_sixb_cache(RuntimeSixb::from_cache(cache.clone()))?;
        Ok(cache)
    }

    /// Loads `.6b` if its header matches the current schema, otherwise rebuilds it from `.6`.
    pub fn ensure_sixb_current(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
    ) -> io::Result<SixbCache> {
        self.ensure_sixb_snapshot(schema, table_name)
            .map(|cache| cache.as_ref().clone())
    }

    fn ensure_sixb_snapshot(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
    ) -> io::Result<Arc<SixbCache>> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        let path = self.sixb_path(table.name());
        if let Some(cache) = self.runtime_sixb_to_cache(table.name(), &schema.schema_hash())? {
            return self.remember_sixb_cache(cache);
        }
        if let Some(cache) = self.cached_sixb(table.name(), &schema.schema_hash())? {
            return Ok(cache);
        }
        if path.exists() {
            let bytes = fs::read(&path)?;
            if let Ok(cache) = decode_sixb_cache(&bytes)
                && cache.version == SIXB_BINARY_VERSION
                && cache.table == table.name()
                && cache.schema_hash == schema.schema_hash()
                && cache.source_hash == self.scan_table_source_hash(table)?
            {
                return self.remember_sixb_cache(cache);
            }
        }
        self.rebuild_sixb(schema, table_name)
            .and_then(|cache| self.remember_sixb_cache(cache))
    }

    fn ensure_table_layout(&self, table: &TableSchema, _schema_hash: &str) -> io::Result<()> {
        let layout_key = (table.name().to_owned(), table.signature());
        if self
            .layout_cache
            .read()
            .map_err(|_| io::Error::other("layout cache lock poisoned"))?
            .contains(&layout_key)
        {
            return Ok(());
        }
        fs::create_dir_all(self.table_dir(table.name()))?;
        for path in six_files_in_read_order(&self.table_dir(table.name()))? {
            verify_header(table, &path)?;
        }
        self.layout_cache
            .write()
            .map_err(|_| io::Error::other("layout cache lock poisoned"))?
            .insert(layout_key);
        Ok(())
    }

    fn write_metadata(&self, schema: &DatabaseSchema, next_tx: u64) -> io::Result<()> {
        let tmp = self.metadata_path().with_extension("toml.tmp");
        let mut out = String::new();
        out.push_str("version = 1\n");
        out.push_str(&format!("schema_hash = \"{}\"\n", schema.schema_hash()));
        out.push_str(&format!("next_tx = {next_tx}\n\n"));

        for (index, table) in schema.tables().values().enumerate() {
            let table_id = index + 1;
            out.push_str(&format!("[tables.{}]\n", table.name()));
            out.push_str(&format!("id = {table_id}\n"));
            out.push_str(&format!("path = \"tables/{}\"\n", table.name()));
            out.push_str(&format!(
                "next_chunk = {}\n",
                self.next_chunk_counter(table.name())?
            ));
            out.push_str(&format!(
                "header = \"{}\"\n\n",
                escape_toml(&encode_six_header(table))
            ));
            out.push_str(&format!("[tables.{}.index]\n", table.name()));
            out.push_str("state = \"ready\"\n");
            out.push_str(&format!("file = \"engine/{}.6b\"\n", table.name()));
            if let Some(source_hash) = self.cached_source_hash(table.name())? {
                out.push_str(&format!(
                    "source_hash = \"{}\"\n",
                    escape_toml(&source_hash)
                ));
            }
            out.push('\n');
        }

        fs::write(&tmp, out)?;
        fs::rename(tmp, self.metadata_path())
    }

    fn scan_table_files(&self, table: &TableSchema) -> io::Result<TableScan> {
        let mut live = BTreeMap::new();
        let mut hash_bytes = Vec::new();
        for path in six_files_in_read_order(&self.table_dir(table.name()))? {
            let chunk_name = relative_chunk_name(&self.table_dir(table.name()), &path)?;
            let entries = scan_six_file(table, &path, &chunk_name)?;
            for entry in entries {
                hash_bytes.extend_from_slice(chunk_name.as_bytes());
                hash_bytes.push(0);
                hash_bytes.extend_from_slice(&entry.raw_line);
                match entry.operation {
                    SixOperationRecord::Put { tx_id: _, record } => {
                        let id = record_id(&record)?;
                        live.insert(
                            id,
                            LiveRow {
                                record,
                                ptr: entry.ptr,
                            },
                        );
                    }
                    SixOperationRecord::Delete { tx_id: _, id } => {
                        live.remove(&id);
                    }
                }
            }
        }
        Ok(TableScan {
            source_hash: source_hash(&hash_bytes),
            live,
        })
    }

    fn scan_table_source_hash(&self, table: &TableSchema) -> io::Result<String> {
        let mut hash_bytes = Vec::new();
        for path in six_files_in_read_order(&self.table_dir(table.name()))? {
            let chunk_name = relative_chunk_name(&self.table_dir(table.name()), &path)?;
            for line in raw_six_data_lines(table, &path)? {
                hash_bytes.extend_from_slice(chunk_name.as_bytes());
                hash_bytes.push(0);
                hash_bytes.extend_from_slice(&line);
            }
        }
        Ok(source_hash(&hash_bytes))
    }

    fn read_row_pointer(&self, table: &TableSchema, ptr: &RowPointer) -> io::Result<Record> {
        let chunk = self.read_chunk(table.name(), &ptr.chunk_name)?;
        let start = ptr.offset as usize;
        let end = start.checked_add(ptr.len as usize).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "row pointer offset overflow")
        })?;
        if end > chunk.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "row pointer extends past chunk",
            ));
        }
        let mut bytes = chunk[start..end].to_vec();
        if matches!(bytes.last(), Some(b'\n')) {
            bytes.pop();
        }
        let line = String::from_utf8(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        match decode_six_operation(table, &line).map_err(format_error_to_io)? {
            SixOperationRecord::Put { record, .. } => Ok(record),
            SixOperationRecord::Delete { .. } => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "row pointer referenced a delete tombstone",
            )),
        }
    }

    fn read_row_entry(&self, table: &TableSchema, entry: &SixbRowEntry) -> io::Result<Record> {
        if let Some(record) = self.cached_record(table.name(), &entry.id)? {
            return Ok(record);
        }
        let record = self.read_row_pointer(table, &entry.ptr)?;
        self.remember_record(&record)?;
        Ok(record)
    }

    fn append_batch_inner(
        &self,
        schema: &DatabaseSchema,
        batch: &WriteBatch,
    ) -> io::Result<Vec<AppendResult>> {
        if batch.is_empty() {
            return Ok(Vec::new());
        };
        self.ensure_workspace_layout()?;
        let table = schema
            .table(batch.table())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        let writer = self.table_writer(table.name())?;
        let _writer_guard = writer
            .lock()
            .map_err(|_| io::Error::other("table writer lock poisoned"))?;
        self.ensure_table_layout(table, &schema.schema_hash())?;

        let mut cache = self.take_runtime_sixb_for_write(schema, table.name())?;
        let tx_start = self.next_tx_id()?;
        let mut encoded = Vec::with_capacity(batch.operations().len());
        let mut batch_ids = BTreeSet::new();
        let mut batch_unique = BTreeMap::<(String, String), String>::new();

        for (index, append) in batch.operations().iter().enumerate() {
            let id = record_id(&append.record)?;
            if !batch_ids.insert(id.clone()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("write batch touches row `{id}` more than once"),
                ));
            }
            if batch.mode() == WriteBatchMode::InsertOnly && cache.has_row(&id) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("row `{}` already exists in `{}`", id, table.name()),
                ));
            }
            if append.operation == Operation::Put {
                validate_put_unique_lookup_conflicts(
                    table,
                    &cache,
                    &append.record,
                    &mut batch_unique,
                )?;
            }

            let tx_id = tx_start + index as u64;
            let line = encode_six_operation(table, append.operation, tx_id, &append.record)
                .map_err(format_error_to_io)?;
            let bytes_written = (line.len() + 1) as u64;
            encoded.push(EncodedAppend {
                operation: append.operation,
                record: append.record.clone(),
                tx_id,
                line,
                bytes_written,
            });
        }

        let schema_hash = schema.schema_hash();
        let preamble = encode_six_preamble(table, &schema_hash);
        let append_len = encoded
            .iter()
            .map(|append| append.bytes_written)
            .sum::<u64>();
        let target = self.append_target(table.name(), append_len, preamble.len() as u64)?;
        if let Some(parent) = target.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut append_bytes = Vec::new();
        if target.is_new {
            append_bytes.extend_from_slice(preamble.as_bytes());
        }
        let mut offset = target.row_offset;
        let mut results = Vec::with_capacity(encoded.len());

        for append in &encoded {
            let id = record_id(&append.record)?;
            let old_record = if cache.has_row(&id) {
                self.cached_record(table.name(), &id)?
            } else {
                None
            };
            let ptr = RowPointer {
                chunk_name: target.chunk_name.clone(),
                offset,
                len: append.bytes_written as u32,
                tx_id: append.tx_id,
            };
            append_bytes.extend_from_slice(append.line.as_bytes());
            append_bytes.push(b'\n');
            cache.apply_operation(
                table,
                append.operation,
                &append.record,
                ptr,
                old_record.as_ref(),
            )?;
            results.push(AppendResult {
                tx_id: append.tx_id,
                operation: append.operation,
                bytes_written: append.bytes_written,
            });
            offset = offset.saturating_add(append.bytes_written);
        }

        let mut hash_bytes = Vec::new();
        for append in &encoded {
            hash_bytes.extend_from_slice(target.chunk_name.as_bytes());
            hash_bytes.push(0);
            hash_bytes.extend_from_slice(append.line.as_bytes());
            hash_bytes.push(b'\n');
        }
        cache.source_hash = extend_source_hash(&cache.source_hash, &hash_bytes)?;
        cache.version = SIXB_BINARY_VERSION;
        cache.table = table.name().to_owned();
        cache.schema_hash = schema_hash;

        if target.is_new {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target.path)?;
            file.write_all(&append_bytes)?;
            self.remember_chunk(table.name(), &target.chunk_name, append_bytes)?;
        } else {
            let mut file = OpenOptions::new().append(true).open(&target.path)?;
            file.write_all(&append_bytes)?;
            self.set_chunk_len(
                table.name(),
                &target.chunk_name,
                target.row_offset + append_len,
            )?;
            self.forget_chunk(table.name(), &target.chunk_name)?;
        }
        self.forget_sixb_cache(table.name())?;
        self.remember_runtime_sixb_cache(cache)?;
        for append in &encoded {
            let id = record_id(&append.record)?;
            match append.operation {
                Operation::Put => self.remember_record(&append.record)?,
                Operation::Delete => self.forget_record(table.name(), &id)?,
            }
        }
        let next_tx = tx_start + encoded.len() as u64;
        self.set_next_tx_id(next_tx)?;
        self.set_next_chunk_counter(table.name(), target.next_chunk)?;
        if encoded.len() > 1 || !self.metadata_path().exists() {
            self.write_metadata(schema, next_tx)?;
        }

        Ok(results)
    }

    fn append_target(
        &self,
        table_name: &str,
        append_len: u64,
        preamble_len: u64,
    ) -> io::Result<AppendTarget> {
        let next_chunk = self.next_chunk_counter(table_name)?;
        if next_chunk > 0 {
            let chunk_counter = next_chunk - 1;
            let chunk_relative_path = chunk_path(chunk_counter)?;
            let chunk_name = chunk_path_to_name(&chunk_relative_path)?;
            let path = self.table_dir(table_name).join(&chunk_relative_path);
            if path.exists() {
                let current_len =
                    if let Some(len) = self.cached_chunk_len(table_name, &chunk_name)? {
                        len
                    } else {
                        let len = fs::metadata(&path)?.len();
                        self.set_chunk_len(table_name, &chunk_name, len)?;
                        len
                    };
                if current_len.saturating_add(append_len) <= MAX_SIX_CHUNK_BYTES {
                    return Ok(AppendTarget {
                        chunk_name,
                        path,
                        row_offset: current_len,
                        next_chunk,
                        is_new: false,
                    });
                }
            }
        }

        let chunk_counter = next_chunk;
        let chunk_relative_path = chunk_path(chunk_counter)?;
        let chunk_name = chunk_path_to_name(&chunk_relative_path)?;
        Ok(AppendTarget {
            chunk_name,
            path: self.table_dir(table_name).join(&chunk_relative_path),
            row_offset: preamble_len,
            next_chunk: chunk_counter.saturating_add(1),
            is_new: true,
        })
    }

    fn write_sixb_cache(&self, table_name: &str, cache: &SixbCache) -> io::Result<Arc<SixbCache>> {
        let path = self.sixb_path(table_name);
        let tmp = path.with_extension("sixb.tmp");
        fs::write(&tmp, encode_sixb_cache(cache))?;
        fs::rename(tmp, path)?;
        self.remember_sixb_cache(cache.clone())
    }

    fn read_chunk(&self, table_name: &str, chunk_name: &str) -> io::Result<Arc<Vec<u8>>> {
        let key = (table_name.to_owned(), chunk_name.to_owned());
        if let Some(chunk) = self
            .chunk_cache
            .read()
            .map_err(|_| io::Error::other("chunk cache lock poisoned"))?
            .get(&key)
            .cloned()
        {
            return Ok(chunk);
        }

        let path = self.table_dir(table_name).join(chunk_name);
        let bytes = Arc::new(fs::read(path)?);
        self.set_chunk_len(table_name, chunk_name, bytes.len() as u64)?;
        self.chunk_cache
            .write()
            .map_err(|_| io::Error::other("chunk cache lock poisoned"))?
            .insert(key, Arc::clone(&bytes));
        Ok(bytes)
    }

    fn remember_chunk(&self, table_name: &str, chunk_name: &str, bytes: Vec<u8>) -> io::Result<()> {
        let len = bytes.len() as u64;
        self.chunk_cache
            .write()
            .map_err(|_| io::Error::other("chunk cache lock poisoned"))?
            .insert(
                (table_name.to_owned(), chunk_name.to_owned()),
                Arc::new(bytes),
            );
        self.set_chunk_len(table_name, chunk_name, len)?;
        Ok(())
    }

    fn forget_chunk(&self, table_name: &str, chunk_name: &str) -> io::Result<()> {
        self.chunk_cache
            .write()
            .map_err(|_| io::Error::other("chunk cache lock poisoned"))?
            .remove(&(table_name.to_owned(), chunk_name.to_owned()));
        Ok(())
    }

    fn cached_chunk_len(&self, table_name: &str, chunk_name: &str) -> io::Result<Option<u64>> {
        Ok(self
            .chunk_len_cache
            .read()
            .map_err(|_| io::Error::other("chunk length cache lock poisoned"))?
            .get(&(table_name.to_owned(), chunk_name.to_owned()))
            .copied())
    }

    fn set_chunk_len(&self, table_name: &str, chunk_name: &str, len: u64) -> io::Result<()> {
        self.chunk_len_cache
            .write()
            .map_err(|_| io::Error::other("chunk length cache lock poisoned"))?
            .insert((table_name.to_owned(), chunk_name.to_owned()), len);
        Ok(())
    }

    fn cached_sixb(
        &self,
        table_name: &str,
        schema_hash: &str,
    ) -> io::Result<Option<Arc<SixbCache>>> {
        let guard = self
            .sixb_cache
            .read()
            .map_err(|_| io::Error::other("sixb cache lock poisoned"))?;
        Ok(guard.get(table_name).and_then(|cache| {
            (cache.version == SIXB_BINARY_VERSION
                && cache.table == table_name
                && cache.schema_hash == schema_hash)
                .then(|| Arc::clone(cache))
        }))
    }

    fn cached_source_hash(&self, table_name: &str) -> io::Result<Option<String>> {
        Ok(self
            .sixb_cache
            .read()
            .map_err(|_| io::Error::other("sixb cache lock poisoned"))?
            .get(table_name)
            .map(|cache| cache.source_hash.clone()))
    }

    fn discovered_next_tx_id(&self) -> io::Result<u64> {
        let tables_dir = self.database_dir().join("tables");
        if !tables_dir.exists() {
            return Ok(1);
        }

        let mut max_tx = 0u64;
        for table_entry in fs::read_dir(tables_dir)? {
            let table_entry = table_entry?;
            let table_dir = table_entry.path();
            if !table_dir.is_dir() {
                continue;
            }
            for path in six_files_in_read_order(&table_dir)? {
                max_tx = max_tx.max(max_tx_in_six_file(&path)?);
            }
        }
        Ok(max_tx.saturating_add(1).max(1))
    }

    fn remember_sixb_cache(&self, cache: SixbCache) -> io::Result<Arc<SixbCache>> {
        let cache = Arc::new(cache);
        let mut guard = self
            .sixb_cache
            .write()
            .map_err(|_| io::Error::other("sixb cache lock poisoned"))?;
        guard.insert(cache.table.clone(), Arc::clone(&cache));
        Ok(cache)
    }

    fn table_writer(&self, table_name: &str) -> io::Result<Arc<Mutex<()>>> {
        if let Some(writer) = self
            .table_writers
            .read()
            .map_err(|_| io::Error::other("table writers lock poisoned"))?
            .get(table_name)
            .cloned()
        {
            return Ok(writer);
        }

        let mut guard = self
            .table_writers
            .write()
            .map_err(|_| io::Error::other("table writers lock poisoned"))?;
        Ok(Arc::clone(
            guard
                .entry(table_name.to_owned())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        ))
    }

    fn set_next_tx_id(&self, next_tx: u64) -> io::Result<()> {
        *self
            .next_tx_cache
            .write()
            .map_err(|_| io::Error::other("next tx cache lock poisoned"))? = Some(next_tx);
        Ok(())
    }

    fn set_next_chunk_counter(&self, table_name: &str, next_chunk: u64) -> io::Result<()> {
        self.next_chunk_cache
            .write()
            .map_err(|_| io::Error::other("next chunk cache lock poisoned"))?
            .insert(table_name.to_owned(), next_chunk);
        Ok(())
    }

    fn cached_record(&self, table_name: &str, id: &str) -> io::Result<Option<Record>> {
        Ok(self
            .row_cache
            .read()
            .map_err(|_| io::Error::other("row cache lock poisoned"))?
            .get(table_name)
            .and_then(|table| table.get(id))
            .cloned())
    }

    fn runtime_sixb_to_cache(
        &self,
        table_name: &str,
        schema_hash: &str,
    ) -> io::Result<Option<SixbCache>> {
        Ok(self
            .runtime_sixb_cache
            .read()
            .map_err(|_| io::Error::other("runtime sixb cache lock poisoned"))?
            .get(table_name)
            .filter(|cache| cache.schema_hash == schema_hash)
            .map(RuntimeSixb::to_cache))
    }

    fn runtime_row_entries(&self, table_name: &str) -> io::Result<Option<Vec<SixbRowEntry>>> {
        Ok(self
            .runtime_sixb_cache
            .read()
            .map_err(|_| io::Error::other("runtime sixb cache lock poisoned"))?
            .get(table_name)
            .map(RuntimeSixb::row_entries))
    }

    fn runtime_row_entry(&self, table_name: &str, id: &str) -> io::Result<Option<SixbRowEntry>> {
        Ok(self
            .runtime_sixb_cache
            .read()
            .map_err(|_| io::Error::other("runtime sixb cache lock poisoned"))?
            .get(table_name)
            .and_then(|cache| cache.row_entry(id)))
    }

    fn runtime_lookup_entries(
        &self,
        table_name: &str,
        field_name: &str,
        key: &str,
    ) -> io::Result<Option<Vec<SixbLookupEntry>>> {
        Ok(self
            .runtime_sixb_cache
            .read()
            .map_err(|_| io::Error::other("runtime sixb cache lock poisoned"))?
            .get(table_name)
            .map(|cache| cache.lookup_entries(field_name, key)))
    }

    fn runtime_row_count(&self, table_name: &str) -> io::Result<Option<usize>> {
        Ok(self
            .runtime_sixb_cache
            .read()
            .map_err(|_| io::Error::other("runtime sixb cache lock poisoned"))?
            .get(table_name)
            .map(RuntimeSixb::row_count))
    }

    fn runtime_lookup_count(
        &self,
        table_name: &str,
        field_name: &str,
        key: &str,
    ) -> io::Result<Option<usize>> {
        Ok(self
            .runtime_sixb_cache
            .read()
            .map_err(|_| io::Error::other("runtime sixb cache lock poisoned"))?
            .get(table_name)
            .map(|cache| cache.lookup_count(field_name, key)))
    }

    fn forget_sixb_cache(&self, table_name: &str) -> io::Result<()> {
        self.sixb_cache
            .write()
            .map_err(|_| io::Error::other("sixb cache lock poisoned"))?
            .remove(table_name);
        Ok(())
    }

    fn take_sixb_cache_for_write(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
    ) -> io::Result<SixbCache> {
        self.ensure_sixb_snapshot(schema, table_name)?;
        let cache = self
            .sixb_cache
            .write()
            .map_err(|_| io::Error::other("sixb cache lock poisoned"))?
            .remove(table_name)
            .ok_or_else(|| io::Error::other("sixb cache missing after ensure"))?;
        Ok(match Arc::try_unwrap(cache) {
            Ok(cache) => cache,
            Err(cache) => cache.as_ref().clone(),
        })
    }

    fn take_runtime_sixb_for_write(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
    ) -> io::Result<RuntimeSixb> {
        if let Some(cache) = self
            .runtime_sixb_cache
            .write()
            .map_err(|_| io::Error::other("runtime sixb cache lock poisoned"))?
            .remove(table_name)
        {
            return Ok(cache);
        }

        self.take_sixb_cache_for_write(schema, table_name)
            .map(RuntimeSixb::from_cache)
    }

    fn remember_runtime_sixb_cache(&self, cache: RuntimeSixb) -> io::Result<()> {
        self.runtime_sixb_cache
            .write()
            .map_err(|_| io::Error::other("runtime sixb cache lock poisoned"))?
            .insert(cache.table.clone(), cache);
        Ok(())
    }

    fn remember_record(&self, record: &Record) -> io::Result<()> {
        let id = record_id(record)?;
        self.row_cache
            .write()
            .map_err(|_| io::Error::other("row cache lock poisoned"))?
            .entry(record.table().to_owned())
            .or_default()
            .insert(id, record.clone());
        Ok(())
    }

    fn forget_record(&self, table_name: &str, id: &str) -> io::Result<()> {
        if let Some(table) = self
            .row_cache
            .write()
            .map_err(|_| io::Error::other("row cache lock poisoned"))?
            .get_mut(table_name)
        {
            table.remove(id);
        }
        Ok(())
    }

    fn remember_table_records<'a>(
        &self,
        table_name: &str,
        records: impl IntoIterator<Item = (&'a String, &'a LiveRow)>,
    ) -> io::Result<()> {
        let mut guard = self
            .row_cache
            .write()
            .map_err(|_| io::Error::other("row cache lock poisoned"))?;
        let table = guard.entry(table_name.to_owned()).or_default();
        table.clear();
        for (id, live) in records {
            table.insert(id.clone(), live.record.clone());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct LiveRow {
    record: Record,
    ptr: RowPointer,
}

#[derive(Debug, Clone)]
struct TableScan {
    source_hash: String,
    live: BTreeMap<String, LiveRow>,
}

#[derive(Debug, Clone)]
struct ScannedSixEntry {
    operation: SixOperationRecord,
    ptr: RowPointer,
    raw_line: Vec<u8>,
}

#[derive(Debug, Clone)]
struct EncodedAppend {
    operation: Operation,
    record: Record,
    tx_id: u64,
    line: String,
    bytes_written: u64,
}

#[derive(Debug, Clone)]
struct AppendTarget {
    chunk_name: String,
    path: PathBuf,
    row_offset: u64,
    next_chunk: u64,
    is_new: bool,
}

#[derive(Debug, Clone)]
struct RuntimeSixb {
    version: u32,
    table: String,
    schema_hash: String,
    source_hash: String,
    rows_by_id: BTreeMap<RowId, RowPointer>,
    lookup_ids: BTreeMap<(String, String), BTreeSet<RowId>>,
    row_lookup_keys: BTreeMap<RowId, Vec<(String, String)>>,
}

impl RuntimeSixb {
    fn from_cache(cache: SixbCache) -> Self {
        let mut lookup_ids = BTreeMap::<(String, String), BTreeSet<RowId>>::new();
        let mut row_lookup_keys = BTreeMap::<RowId, Vec<(String, String)>>::new();
        for lookup in cache.lookups {
            let key = (lookup.field_name, lookup.key);
            lookup_ids
                .entry(key.clone())
                .or_default()
                .insert(lookup.id.clone());
            row_lookup_keys.entry(lookup.id).or_default().push(key);
        }

        Self {
            version: cache.version,
            table: cache.table,
            schema_hash: cache.schema_hash,
            source_hash: cache.source_hash,
            rows_by_id: cache
                .rows
                .into_iter()
                .map(|entry| (entry.id, entry.ptr))
                .collect(),
            lookup_ids,
            row_lookup_keys,
        }
    }

    fn to_cache(&self) -> SixbCache {
        let rows = self
            .rows_by_id
            .iter()
            .map(|(id, ptr)| SixbRowEntry {
                id: id.clone(),
                ptr: ptr.clone(),
            })
            .collect();
        let mut lookups = Vec::new();
        for ((field_name, key), ids) in &self.lookup_ids {
            for id in ids {
                lookups.push(SixbLookupEntry {
                    field_name: field_name.clone(),
                    key: key.clone(),
                    id: id.clone(),
                });
            }
        }
        SixbCache {
            version: self.version,
            table: self.table.clone(),
            schema_hash: self.schema_hash.clone(),
            source_hash: self.source_hash.clone(),
            rows,
            lookups,
        }
    }

    fn has_row(&self, id: &str) -> bool {
        self.rows_by_id.contains_key(id)
    }

    fn first_lookup_id(&self, field_name: &str, key: &str) -> Option<&str> {
        self.lookup_ids
            .get(&(field_name.to_owned(), key.to_owned()))
            .and_then(|ids| ids.first())
            .map(String::as_str)
    }

    fn row_entry(&self, id: &str) -> Option<SixbRowEntry> {
        self.rows_by_id.get(id).map(|ptr| SixbRowEntry {
            id: id.to_owned(),
            ptr: ptr.clone(),
        })
    }

    fn row_entries(&self) -> Vec<SixbRowEntry> {
        self.rows_by_id
            .iter()
            .map(|(id, ptr)| SixbRowEntry {
                id: id.clone(),
                ptr: ptr.clone(),
            })
            .collect()
    }

    fn lookup_entries(&self, field_name: &str, key: &str) -> Vec<SixbLookupEntry> {
        self.lookup_ids
            .get(&(field_name.to_owned(), key.to_owned()))
            .into_iter()
            .flat_map(|ids| ids.iter())
            .map(|id| SixbLookupEntry {
                field_name: field_name.to_owned(),
                key: key.to_owned(),
                id: id.clone(),
            })
            .collect()
    }

    fn lookup_count(&self, field_name: &str, key: &str) -> usize {
        self.lookup_ids
            .get(&(field_name.to_owned(), key.to_owned()))
            .map_or(0, BTreeSet::len)
    }

    fn row_count(&self) -> usize {
        self.rows_by_id.len()
    }

    fn apply_operation(
        &mut self,
        table: &TableSchema,
        operation: Operation,
        record: &Record,
        ptr: RowPointer,
        old_record: Option<&Record>,
    ) -> io::Result<()> {
        let id = record_id(record)?;
        match operation {
            Operation::Put => {
                if self.rows_by_id.contains_key(&id) {
                    self.remove_record_lookups(table, &id, old_record)?;
                }
                self.rows_by_id.insert(id.clone(), ptr);
                self.insert_record_lookups(table, &id, record)
            }
            Operation::Delete => {
                self.rows_by_id.remove(&id);
                self.remove_record_lookups(table, &id, old_record)
            }
        }
    }

    fn insert_record_lookups(
        &mut self,
        table: &TableSchema,
        id: &str,
        record: &Record,
    ) -> io::Result<()> {
        let mut keys = Vec::new();
        for lookup in table.lookup_specs_with_implicit_id() {
            let value = record.fields().get(lookup.field_name()).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("missing lookup field `{}`", lookup.field_name()),
                )
            })?;
            let key = (lookup.field_name().to_owned(), value_to_lookup_key(value));
            self.lookup_ids
                .entry(key.clone())
                .or_default()
                .insert(id.to_owned());
            keys.push(key);
        }
        self.row_lookup_keys.insert(id.to_owned(), keys);
        Ok(())
    }

    fn remove_record_lookups(
        &mut self,
        table: &TableSchema,
        id: &str,
        old_record: Option<&Record>,
    ) -> io::Result<()> {
        if let Some(old_record) = old_record {
            let mut keys = Vec::new();
            for lookup in table.lookup_specs_with_implicit_id() {
                let value = old_record
                    .fields()
                    .get(lookup.field_name())
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("missing lookup field `{}`", lookup.field_name()),
                        )
                    })?;
                keys.push((lookup.field_name().to_owned(), value_to_lookup_key(value)));
            }
            self.remove_lookup_keys(id, keys);
            return Ok(());
        }

        if let Some(keys) = self.row_lookup_keys.remove(id) {
            self.remove_lookup_keys(id, keys);
            return Ok(());
        }

        self.remove_lookup_id_slow(id);
        Ok(())
    }

    fn remove_lookup_keys(&mut self, id: &str, keys: Vec<(String, String)>) {
        for key in &keys {
            let remove_key = if let Some(ids) = self.lookup_ids.get_mut(key) {
                ids.remove(id);
                ids.is_empty()
            } else {
                false
            };
            if remove_key {
                self.lookup_ids.remove(key);
            }
        }
        self.row_lookup_keys.remove(id);
    }

    fn remove_lookup_id_slow(&mut self, id: &str) {
        let empty_keys = self
            .lookup_ids
            .iter_mut()
            .filter_map(|(key, ids)| {
                ids.remove(id);
                ids.is_empty().then(|| key.clone())
            })
            .collect::<Vec<_>>();
        for key in empty_keys {
            self.lookup_ids.remove(&key);
        }
        self.row_lookup_keys.remove(id);
    }
}

fn verify_header(table: &TableSchema, path: &Path) -> io::Result<()> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut header = String::new();
    reader.read_line(&mut header)?;
    let actual = header.trim_end_matches(['\r', '\n']);
    let expected = encode_six_header(table);
    if actual == expected || is_six_magic_line(actual) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected .6 header `{expected}`, found `{actual}`"),
        ))
    }
}

fn one_append_result(mut results: Vec<AppendResult>) -> io::Result<AppendResult> {
    results.pop().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "single-operation batch produced no append result",
        )
    })
}

fn scan_six_file(
    table: &TableSchema,
    path: &Path,
    chunk_name: &str,
) -> io::Result<Vec<ScannedSixEntry>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut out = Vec::new();
    let mut offset = 0u64;

    loop {
        let line_offset = offset;
        let mut line = String::new();
        let len = reader.read_line(&mut line)?;
        if len == 0 {
            break;
        }
        offset += len as u64;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            continue;
        }
        if is_six_magic_line(trimmed) || trimmed.starts_with('@') {
            continue;
        }

        let operation = if trimmed.starts_with("R\t") || trimmed.starts_with("D\t") {
            decode_six_operation(table, trimmed).map_err(format_error_to_io)?
        } else if trimmed == encode_six_header(table) {
            continue;
        } else {
            SixOperationRecord::Put {
                tx_id: 0,
                record: decode_six_row(table, trimmed).map_err(format_error_to_io)?,
            }
        };
        let tx_id = operation.tx_id();
        out.push(ScannedSixEntry {
            operation,
            ptr: RowPointer {
                chunk_name: chunk_name.to_owned(),
                offset: line_offset,
                len: len as u32,
                tx_id,
            },
            raw_line: line.into_bytes(),
        });
    }
    Ok(out)
}

fn raw_six_data_lines(table: &TableSchema, path: &Path) -> io::Result<Vec<Vec<u8>>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    let header = encode_six_header(table);

    for line in reader.lines() {
        let mut line = line?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty()
            || is_six_magic_line(trimmed)
            || trimmed.starts_with('@')
            || trimmed == header
        {
            continue;
        }
        line.push('\n');
        out.push(line.into_bytes());
    }
    Ok(out)
}

fn max_tx_in_six_file(path: &Path) -> io::Result<u64> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut max_tx = 0u64;

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if !(trimmed.starts_with("R\t") || trimmed.starts_with("D\t")) {
            continue;
        }
        let mut parts = trimmed.splitn(3, '\t');
        let _tag = parts.next();
        let Some(tx) = parts.next() else {
            continue;
        };
        if let Ok(tx) = tx.parse::<u64>() {
            max_tx = max_tx.max(tx);
        }
    }
    Ok(max_tx)
}

fn record_id(record: &Record) -> io::Result<String> {
    let value = record
        .fields()
        .get("id")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "record missing id"))?;
    Ok(value_to_lookup_key(value))
}

fn value_to_lookup_key(value: &Value) -> String {
    match value {
        Value::Id(value) | Value::Text(value) => value.clone(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
    }
}

fn validate_unique_lookups(table: &TableSchema, lookups: &[SixbLookupEntry]) -> io::Result<()> {
    for lookup in table.lookup_specs_with_implicit_id() {
        if !lookup.unique() {
            continue;
        }
        let mut seen = BTreeMap::<(&str, &str), &str>::new();
        for entry in lookups
            .iter()
            .filter(|entry| entry.field_name == lookup.field_name())
        {
            let key = (entry.field_name.as_str(), entry.key.as_str());
            if let Some(existing_id) = seen.insert(key, entry.id.as_str())
                && existing_id != entry.id
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "unique lookup `{}` has duplicate key `{}`",
                        lookup.field_name(),
                        entry.key
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn validate_put_unique_lookup_conflicts(
    table: &TableSchema,
    cache: &RuntimeSixb,
    record: &Record,
    batch_unique: &mut BTreeMap<(String, String), String>,
) -> io::Result<()> {
    let id = record_id(record)?;
    for lookup in table
        .lookup_specs_with_implicit_id()
        .into_iter()
        .filter(|lookup| lookup.unique())
    {
        let value = record.fields().get(lookup.field_name()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("missing lookup field `{}`", lookup.field_name()),
            )
        })?;
        let key = value_to_lookup_key(value);
        if let Some(conflict_id) = cache.first_lookup_id(lookup.field_name(), &key)
            && conflict_id != id
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "unique lookup `{}` key `{}` is already used by row `{}`",
                    lookup.field_name(),
                    key,
                    conflict_id
                ),
            ));
        }
        let unique_key = (lookup.field_name().to_owned(), key);
        if let Some(existing_id) = batch_unique.insert(unique_key, id.clone())
            && existing_id != id
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "unique lookup `{}` key is used by multiple rows in one batch",
                    lookup.field_name()
                ),
            ));
        }
    }
    Ok(())
}

fn sort_sixb_entries(rows: &mut [SixbRowEntry], lookups: &mut [SixbLookupEntry]) {
    rows.sort_by(|left, right| left.id.cmp(&right.id));
    lookups.sort_by(|left, right| {
        left.field_name
            .cmp(&right.field_name)
            .then_with(|| left.key.cmp(&right.key))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn row_entry_by_id<'a>(cache: &'a SixbCache, id: &str) -> Option<&'a SixbRowEntry> {
    cache
        .rows
        .binary_search_by(|entry| entry.id.as_str().cmp(id))
        .ok()
        .map(|index| &cache.rows[index])
}

fn lookup_entries_by_key<'a>(
    cache: &'a SixbCache,
    field_name: &str,
    key: &str,
) -> &'a [SixbLookupEntry] {
    let start = cache.lookups.partition_point(|entry| {
        (entry.field_name.as_str(), entry.key.as_str()) < (field_name, key)
    });
    let len = cache.lookups[start..]
        .partition_point(|entry| entry.field_name == field_name && entry.key == key);
    &cache.lookups[start..start + len]
}

fn chunk_path(global_chunk_counter: u64) -> io::Result<PathBuf> {
    if global_chunk_counter >= MAX_CHUNKS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("chunk counter must be between 0 and {}", MAX_CHUNKS - 1),
        ));
    }

    let file = encode_reverse_base36(global_chunk_counter as usize, CHUNK_WIDTH)?;
    Ok(PathBuf::from(format!("{file}.6")))
}

fn chunk_path_to_name(path: &Path) -> io::Result<String> {
    let value = path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad chunk path `{}`", path.display()),
        )
    })?;
    Ok(value.replace('\\', "/"))
}

fn encode_reverse_base36(n: usize, width: usize) -> io::Result<String> {
    let max = CHUNK_BASE.pow(width as u32);
    if n >= max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("value must be between 0 and {}", max - 1),
        ));
    }
    encode_fixed_base36(max - 1 - n, width)
}

fn encode_fixed_base36(mut n: usize, width: usize) -> io::Result<String> {
    let max = CHUNK_BASE.pow(width as u32);
    if n >= max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("value must be between 0 and {}", max - 1),
        ));
    }

    let mut out = vec![b'0'; width];
    for i in (0..width).rev() {
        let digit = n % CHUNK_BASE;
        out[i] = CHUNK_CHARS[digit];
        n /= CHUNK_BASE;
    }
    String::from_utf8(out)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
}

fn six_files_in_read_order(table_dir: &Path) -> io::Result<Vec<PathBuf>> {
    if !table_dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_six_files(table_dir, &mut files)?;
    files.sort();
    files.reverse();
    Ok(files)
}

fn collect_six_files(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_six_files(&path, files)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("6") {
            files.push(path);
        }
    }
    Ok(())
}

fn relative_chunk_name(table_dir: &Path, path: &Path) -> io::Result<String> {
    let relative = path.strip_prefix(table_dir).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad chunk path `{}`: {error}", path.display()),
        )
    })?;
    let value = relative.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad chunk path `{}`", relative.display()),
        )
    })?;
    Ok(value.replace('\\', "/"))
}

fn escape_toml(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn format_error_to_io(error: sixpack_format::FormatError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn extend_source_hash(current: &str, bytes: &[u8]) -> io::Result<String> {
    let mut hash = if current.is_empty() {
        FNV_OFFSET_BASIS
    } else {
        u64::from_str_radix(current, 16).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("bad source hash `{current}`: {error}"),
            )
        })?
    };
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    Ok(format!("{hash:016x}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sixpack_core::{DatabaseSchema, PrimitiveType};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_root(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        dir.push(format!(
            "sixpack-store-{name}-{}-{stamp}-{counter}",
            std::process::id()
        ));
        dir
    }

    fn schema() -> DatabaseSchema {
        let mut schema = DatabaseSchema::new();
        let mut messages = TableSchema::new("messages");
        messages.add_field("id", PrimitiveType::Id).unwrap();
        messages.add_field("body", PrimitiveType::Text).unwrap();
        messages
            .add_field("created_at", PrimitiveType::Int)
            .unwrap();
        messages.add_lookup("created_at", false).unwrap();
        schema.add_table(messages).unwrap();
        schema
    }

    #[test]
    fn append_writes_six_layout_and_metadata() {
        let root = temp_root("six");
        let schema = schema();
        let store = LocalStore::new(&root, "db");
        let first = Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", "hello\tworld")
            .unwrap()
            .with_field("created_at", 1i64)
            .unwrap();

        let second = Record::new("messages")
            .with_id("m2")
            .unwrap()
            .with_field("body", "line\nbreak")
            .unwrap()
            .with_field("created_at", 2i64)
            .unwrap();

        let one = store.append_put(&schema, &first).unwrap();
        let two = store.append_put(&schema, &second).unwrap();
        assert_eq!(one.tx_id, 1);
        assert_eq!(two.tx_id, 2);
        assert_eq!(one.operation, Operation::Put);
        assert!(one.bytes_written > 0);

        let chunk = fs::read_to_string(store.chunk_six_path("messages", 0).unwrap()).unwrap();
        assert!(!store.chunk_six_path("messages", 1).unwrap().exists());
        assert!(chunk.starts_with("SIX\t1\ttable\tmessages\t"));
        assert!(chunk.contains("@field\tid\tid\n"));
        assert!(chunk.contains("@field\tbody\ttext\n"));
        assert!(chunk.contains("@lookup\tid\tunique\n"));
        assert!(chunk.contains("@lookup\tcreated_at\tmany\n"));
        assert!(chunk.contains("@data\n"));
        assert!(chunk.contains("R\t1\tm1\thello\\tworld\t1\n"));
        assert!(chunk.contains("R\t2\tm2\tline\\nbreak\t2\n"));
        assert!(!chunk.contains("\tput\t"));

        let metadata = fs::read_to_string(store.metadata_path()).unwrap();
        assert!(metadata.contains("[tables.messages]"));
        assert!(metadata.contains("next_tx = 2"));
        assert!(metadata.contains("next_chunk = 1"));
        assert!(!metadata.contains("chunks = ["));
        assert!(metadata.contains("file = \"engine/messages.6b\""));
        assert!(store.sixb_path("messages").exists());

        let rows = store.read_table(&schema, "messages").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].fields().get("id"), first.fields().get("id"));
        assert_eq!(rows[1].fields().get("id"), second.fields().get("id"));

        let incremental_cache = store.ensure_sixb_current(&schema, "messages").unwrap();
        let rebuilt_cache = store.rebuild_sixb(&schema, "messages").unwrap();
        assert_eq!(incremental_cache, rebuilt_cache);

        let recovered = LocalStore::new(&root, "db");
        assert_eq!(recovered.next_tx_id().unwrap(), 3);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn chunk_paths_reverse_sort_from_counter() {
        assert_eq!(chunk_path(0).unwrap(), PathBuf::from("zzz.6"));
        assert_eq!(chunk_path(1).unwrap(), PathBuf::from("zzy.6"));
        assert_eq!(chunk_path(2).unwrap(), PathBuf::from("zzx.6"));
        assert_eq!(chunk_path(46_655).unwrap(), PathBuf::from("000.6"));
        assert!(chunk_path(46_656).is_err());
    }
}
