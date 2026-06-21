//! Local storage engine boundary.
//!
//! The current store writes readable `.ten` table row segments, keeps a small
//! `tensack.toml` physical layout map, and rebuilds generated `.tenb` lookup
//! caches from canonical `.ten` data.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use tensack_core::{DatabaseSchema, Record, TableSchema, Value};
use tensack_format::{
    Operation, RowPointer, TENB_BINARY_VERSION, TenOperationRecord, TenbCache, TenbLookupEntry,
    TenbRowEntry, decode_ten_operation, decode_ten_row, decode_tenb_cache, encode_ten_header,
    encode_ten_operation, encode_ten_preamble, encode_tenb_cache, is_ten_magic_line, source_hash,
};

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;
const CHUNK_CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
const CHUNK_BASE: usize = 36;
const CHUNK_WIDTH: usize = 3;
const MAX_CHUNKS: u64 = 36u64.pow(CHUNK_WIDTH as u32);
const MAX_TEN_CHUNK_BYTES: u64 = 1024 * 1024;

type TableName = String;
type RowId = String;
type ChunkName = String;
type CachedChunk = Arc<Vec<u8>>;

/// Local store handle.
#[derive(Clone)]
pub struct LocalStore {
    root: PathBuf,
    workspace: String,
    tenb_cache: Arc<RwLock<BTreeMap<TableName, Arc<TenbCache>>>>,
    row_cache: Arc<RwLock<BTreeMap<(TableName, RowId), Record>>>,
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
    /// Bytes written to the `.ten` row segment (line + newline).
    pub bytes_written: u64,
}

/// One append-only `.ten` operation for a batch write.
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
            tenb_cache: Arc::new(RwLock::new(BTreeMap::new())),
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

    /// Root `tensack.toml` metadata path.
    pub fn metadata_path(&self) -> PathBuf {
        self.database_dir().join("tensack.toml")
    }

    /// Table directory.
    pub fn table_dir(&self, table: &str) -> PathBuf {
        self.database_dir().join("tables").join(table)
    }

    /// Deterministic `.ten` chunk path for a table counter.
    pub fn chunk_ten_path(&self, table: &str, chunk_counter: u64) -> io::Result<PathBuf> {
        Ok(self.table_dir(table).join(chunk_path(chunk_counter)?))
    }

    /// Generated binary cache for table indexes/lookups.
    pub fn tenb_path(&self, table: &str) -> PathBuf {
        self.database_dir()
            .join("engine")
            .join(format!("{table}.tenb"))
    }

    /// Optional generated full-text search index path.
    pub fn tenx_path(&self, table: &str) -> PathBuf {
        self.database_dir()
            .join("engine")
            .join(format!("{table}.tenx"))
    }

    /// Appends a put event to the `.ten` table segment.
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

    /// Appends multiple operations to one table in one `.ten` chunk.
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

    /// Appends a prepared write batch to one `.ten` chunk.
    pub fn append_batch(
        &self,
        schema: &DatabaseSchema,
        batch: &WriteBatch,
    ) -> io::Result<Vec<AppendResult>> {
        self.append_batch_inner(schema, batch)
    }

    /// Appends a delete event to the `.ten` table segment.
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
            self.rebuild_tenb(schema, table.name())?;
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
        let discovered_next = ten_files_in_read_order(&self.table_dir(table_name))?.len() as u64;
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

    /// Reads all current live records from a table using the generated `.tenb` cache.
    pub fn read_table(&self, schema: &DatabaseSchema, table_name: &str) -> io::Result<Vec<Record>> {
        let cache = self.ensure_tenb_snapshot(schema, table_name)?;
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;

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
        let cache = self.ensure_tenb_snapshot(schema, table_name)?;
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
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
        let cache = self.ensure_tenb_snapshot(schema, table_name)?;
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
        let cache = self.ensure_tenb_snapshot(schema, table_name)?;
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        let mut rows = Vec::new();
        for entry in cache.rows.iter().skip(offset).take(limit) {
            rows.push(self.read_row_entry(table, entry)?);
        }
        Ok(rows)
    }

    /// Counts current live rows in one table.
    pub fn count_table(&self, schema: &DatabaseSchema, table_name: &str) -> io::Result<usize> {
        let cache = self.ensure_tenb_snapshot(schema, table_name)?;
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
        let cache = self.ensure_tenb_snapshot(schema, table_name)?;
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
            return Ok(usize::from(row_entry_by_id(&cache, key).is_some()));
        }
        Ok(lookup_entries_by_key(&cache, field_name, key).len())
    }

    /// Rebuilds the `.tenb` cache from canonical `.ten` files.
    pub fn rebuild_tenb(&self, schema: &DatabaseSchema, table_name: &str) -> io::Result<TenbCache> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        let scan = self.scan_table_files(table)?;
        let mut rows = Vec::new();
        let mut lookups = Vec::new();

        for (id, live) in &scan.live {
            rows.push(TenbRowEntry {
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
                lookups.push(TenbLookupEntry {
                    field_name: lookup.field_name().to_owned(),
                    key: value_to_lookup_key(value),
                    id: id.clone(),
                });
            }
        }
        validate_unique_lookups(table, &lookups)?;
        sort_tenb_entries(&mut rows, &mut lookups);

        let cache = TenbCache {
            version: TENB_BINARY_VERSION,
            table: table.name().to_owned(),
            schema_hash: schema.schema_hash(),
            source_hash: scan.source_hash.clone(),
            rows,
            lookups,
        };
        self.remember_table_records(table.name(), scan.live.iter())?;
        self.write_tenb_cache(table.name(), &cache)?;
        Ok(cache)
    }

    /// Loads `.tenb` if its header matches the current schema, otherwise rebuilds it from `.ten`.
    pub fn ensure_tenb_current(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
    ) -> io::Result<TenbCache> {
        self.ensure_tenb_snapshot(schema, table_name)
            .map(|cache| cache.as_ref().clone())
    }

    fn ensure_tenb_snapshot(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
    ) -> io::Result<Arc<TenbCache>> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        let path = self.tenb_path(table.name());
        if let Some(cache) = self.cached_tenb(table.name(), &schema.schema_hash())? {
            return Ok(cache);
        }
        if path.exists() {
            let bytes = fs::read(&path)?;
            if let Ok(cache) = decode_tenb_cache(&bytes)
                && cache.version == TENB_BINARY_VERSION
                && cache.table == table.name()
                && cache.schema_hash == schema.schema_hash()
                && cache.source_hash == self.scan_table_source_hash(table)?
            {
                return self.remember_tenb_cache(cache);
            }
        }
        self.rebuild_tenb(schema, table_name)
            .and_then(|cache| self.remember_tenb_cache(cache))
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
        for path in ten_files_in_read_order(&self.table_dir(table.name()))? {
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
                escape_toml(&encode_ten_header(table))
            ));
            out.push_str(&format!("[tables.{}.index]\n", table.name()));
            out.push_str("state = \"ready\"\n");
            out.push_str(&format!("file = \"engine/{}.tenb\"\n", table.name()));
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
        for path in ten_files_in_read_order(&self.table_dir(table.name()))? {
            let chunk_name = relative_chunk_name(&self.table_dir(table.name()), &path)?;
            let entries = scan_ten_file(table, &path, &chunk_name)?;
            for entry in entries {
                hash_bytes.extend_from_slice(chunk_name.as_bytes());
                hash_bytes.push(0);
                hash_bytes.extend_from_slice(&entry.raw_line);
                match entry.operation {
                    TenOperationRecord::Put { tx_id: _, record } => {
                        let id = record_id(&record)?;
                        live.insert(
                            id,
                            LiveRow {
                                record,
                                ptr: entry.ptr,
                            },
                        );
                    }
                    TenOperationRecord::Delete { tx_id: _, id } => {
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
        for path in ten_files_in_read_order(&self.table_dir(table.name()))? {
            let chunk_name = relative_chunk_name(&self.table_dir(table.name()), &path)?;
            for line in raw_ten_data_lines(table, &path)? {
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
        match decode_ten_operation(table, &line).map_err(format_error_to_io)? {
            TenOperationRecord::Put { record, .. } => Ok(record),
            TenOperationRecord::Delete { .. } => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "row pointer referenced a delete tombstone",
            )),
        }
    }

    fn read_row_entry(&self, table: &TableSchema, entry: &TenbRowEntry) -> io::Result<Record> {
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

        let snapshot = self.ensure_tenb_snapshot(schema, table.name())?;
        let tx_start = self.next_tx_id()?;
        let mut cache = snapshot.as_ref().clone();
        let mut encoded = Vec::with_capacity(batch.operations().len());

        for (index, append) in batch.operations().iter().enumerate() {
            let id = record_id(&append.record)?;
            if batch.mode() == WriteBatchMode::InsertOnly && row_entry_by_id(&cache, &id).is_some()
            {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("row `{}` already exists in `{}`", id, table.name()),
                ));
            }
            if append.operation == Operation::Put {
                validate_put_unique_lookup_conflicts(table, &cache, &append.record)?;
            }

            let tx_id = tx_start + index as u64;
            let line = encode_ten_operation(table, append.operation, tx_id, &append.record)
                .map_err(format_error_to_io)?;
            let bytes_written = (line.len() + 1) as u64;
            apply_operation_to_cache(
                table,
                &mut cache,
                append.operation,
                &append.record,
                RowPointer {
                    chunk_name: String::new(),
                    offset: 0,
                    len: bytes_written as u32,
                    tx_id,
                },
            )?;
            encoded.push(EncodedAppend {
                operation: append.operation,
                record: append.record.clone(),
                tx_id,
                line,
                bytes_written,
            });
        }

        let schema_hash = schema.schema_hash();
        let preamble = encode_ten_preamble(table, &schema_hash);
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
            let ptr = RowPointer {
                chunk_name: target.chunk_name.clone(),
                offset,
                len: append.bytes_written as u32,
                tx_id: append.tx_id,
            };
            append_bytes.extend_from_slice(append.line.as_bytes());
            append_bytes.push(b'\n');
            apply_operation_to_cache(table, &mut cache, append.operation, &append.record, ptr)?;
            results.push(AppendResult {
                tx_id: append.tx_id,
                operation: append.operation,
                bytes_written: append.bytes_written,
            });
            offset = offset.saturating_add(append.bytes_written);
        }

        validate_unique_lookups(table, &cache.lookups)?;
        let mut hash_bytes = Vec::new();
        for append in &encoded {
            hash_bytes.extend_from_slice(target.chunk_name.as_bytes());
            hash_bytes.push(0);
            hash_bytes.extend_from_slice(append.line.as_bytes());
            hash_bytes.push(b'\n');
        }
        cache.source_hash = extend_source_hash(&cache.source_hash, &hash_bytes)?;
        cache.version = TENB_BINARY_VERSION;
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
        self.remember_tenb_cache(cache)?;
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
                if current_len.saturating_add(append_len) <= MAX_TEN_CHUNK_BYTES {
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

    fn write_tenb_cache(&self, table_name: &str, cache: &TenbCache) -> io::Result<Arc<TenbCache>> {
        let path = self.tenb_path(table_name);
        let tmp = path.with_extension("tenb.tmp");
        fs::write(&tmp, encode_tenb_cache(cache))?;
        fs::rename(tmp, path)?;
        self.remember_tenb_cache(cache.clone())
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

    fn cached_tenb(
        &self,
        table_name: &str,
        schema_hash: &str,
    ) -> io::Result<Option<Arc<TenbCache>>> {
        let guard = self
            .tenb_cache
            .read()
            .map_err(|_| io::Error::other("tenb cache lock poisoned"))?;
        Ok(guard.get(table_name).and_then(|cache| {
            (cache.version == TENB_BINARY_VERSION
                && cache.table == table_name
                && cache.schema_hash == schema_hash)
                .then(|| Arc::clone(cache))
        }))
    }

    fn cached_source_hash(&self, table_name: &str) -> io::Result<Option<String>> {
        Ok(self
            .tenb_cache
            .read()
            .map_err(|_| io::Error::other("tenb cache lock poisoned"))?
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
            for path in ten_files_in_read_order(&table_dir)? {
                max_tx = max_tx.max(max_tx_in_ten_file(&path)?);
            }
        }
        Ok(max_tx.saturating_add(1).max(1))
    }

    fn remember_tenb_cache(&self, cache: TenbCache) -> io::Result<Arc<TenbCache>> {
        let cache = Arc::new(cache);
        let mut guard = self
            .tenb_cache
            .write()
            .map_err(|_| io::Error::other("tenb cache lock poisoned"))?;
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
            .get(&(table_name.to_owned(), id.to_owned()))
            .cloned())
    }

    fn remember_record(&self, record: &Record) -> io::Result<()> {
        let id = record_id(record)?;
        self.row_cache
            .write()
            .map_err(|_| io::Error::other("row cache lock poisoned"))?
            .insert((record.table().to_owned(), id), record.clone());
        Ok(())
    }

    fn forget_record(&self, table_name: &str, id: &str) -> io::Result<()> {
        self.row_cache
            .write()
            .map_err(|_| io::Error::other("row cache lock poisoned"))?
            .remove(&(table_name.to_owned(), id.to_owned()));
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
        guard.retain(|(table, _), _| table != table_name);
        for (id, live) in records {
            guard.insert((table_name.to_owned(), id.clone()), live.record.clone());
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
struct ScannedTenEntry {
    operation: TenOperationRecord,
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

fn verify_header(table: &TableSchema, path: &Path) -> io::Result<()> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut header = String::new();
    reader.read_line(&mut header)?;
    let actual = header.trim_end_matches(['\r', '\n']);
    let expected = encode_ten_header(table);
    if actual == expected || is_ten_magic_line(actual) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected .ten header `{expected}`, found `{actual}`"),
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

fn scan_ten_file(
    table: &TableSchema,
    path: &Path,
    chunk_name: &str,
) -> io::Result<Vec<ScannedTenEntry>> {
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
        if is_ten_magic_line(trimmed) || trimmed.starts_with('@') {
            continue;
        }

        let operation = if trimmed.starts_with("R\t") || trimmed.starts_with("D\t") {
            decode_ten_operation(table, trimmed).map_err(format_error_to_io)?
        } else if trimmed == encode_ten_header(table) {
            continue;
        } else {
            TenOperationRecord::Put {
                tx_id: 0,
                record: decode_ten_row(table, trimmed).map_err(format_error_to_io)?,
            }
        };
        let tx_id = operation.tx_id();
        out.push(ScannedTenEntry {
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

fn raw_ten_data_lines(table: &TableSchema, path: &Path) -> io::Result<Vec<Vec<u8>>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    let header = encode_ten_header(table);

    for line in reader.lines() {
        let mut line = line?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty()
            || is_ten_magic_line(trimmed)
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

fn max_tx_in_ten_file(path: &Path) -> io::Result<u64> {
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

fn validate_unique_lookups(table: &TableSchema, lookups: &[TenbLookupEntry]) -> io::Result<()> {
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

fn apply_put_to_cache(
    table: &TableSchema,
    cache: &mut TenbCache,
    record: &Record,
    ptr: RowPointer,
) -> io::Result<()> {
    let id = record_id(record)?;
    if let Ok(index) = cache
        .rows
        .binary_search_by(|entry| entry.id.as_str().cmp(id.as_str()))
    {
        cache.rows.remove(index);
    }
    cache.lookups.retain(|entry| entry.id != id);

    let row = TenbRowEntry {
        id: id.clone(),
        ptr,
    };
    let row_index = cache
        .rows
        .partition_point(|entry| entry.id.as_str() < row.id.as_str());
    cache.rows.insert(row_index, row);

    for lookup in table.lookup_specs_with_implicit_id() {
        let value = record.fields().get(lookup.field_name()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("missing lookup field `{}`", lookup.field_name()),
            )
        })?;
        let entry = TenbLookupEntry {
            field_name: lookup.field_name().to_owned(),
            key: value_to_lookup_key(value),
            id: id.clone(),
        };
        let lookup_index = cache.lookups.partition_point(|existing| {
            (
                existing.field_name.as_str(),
                existing.key.as_str(),
                existing.id.as_str(),
            ) < (
                entry.field_name.as_str(),
                entry.key.as_str(),
                entry.id.as_str(),
            )
        });
        cache.lookups.insert(lookup_index, entry);
    }
    Ok(())
}

fn apply_operation_to_cache(
    table: &TableSchema,
    cache: &mut TenbCache,
    operation: Operation,
    record: &Record,
    ptr: RowPointer,
) -> io::Result<()> {
    let id = record_id(record)?;
    match operation {
        Operation::Put => apply_put_to_cache(table, cache, record, ptr),
        Operation::Delete => {
            if let Ok(index) = cache
                .rows
                .binary_search_by(|entry| entry.id.as_str().cmp(id.as_str()))
            {
                cache.rows.remove(index);
            }
            cache.lookups.retain(|entry| entry.id != id);
            Ok(())
        }
    }
}

fn validate_put_unique_lookup_conflicts(
    table: &TableSchema,
    cache: &TenbCache,
    record: &Record,
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
        if let Some(conflict) = lookup_entries_by_key(cache, lookup.field_name(), &key).first()
            && conflict.id != id
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "unique lookup `{}` key `{}` is already used by row `{}`",
                    lookup.field_name(),
                    key,
                    conflict.id
                ),
            ));
        }
    }
    Ok(())
}

fn sort_tenb_entries(rows: &mut [TenbRowEntry], lookups: &mut [TenbLookupEntry]) {
    rows.sort_by(|left, right| left.id.cmp(&right.id));
    lookups.sort_by(|left, right| {
        left.field_name
            .cmp(&right.field_name)
            .then_with(|| left.key.cmp(&right.key))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn row_entry_by_id<'a>(cache: &'a TenbCache, id: &str) -> Option<&'a TenbRowEntry> {
    cache
        .rows
        .binary_search_by(|entry| entry.id.as_str().cmp(id))
        .ok()
        .map(|index| &cache.rows[index])
}

fn lookup_entries_by_key<'a>(
    cache: &'a TenbCache,
    field_name: &str,
    key: &str,
) -> &'a [TenbLookupEntry] {
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
    Ok(PathBuf::from(format!("{file}.ten")))
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

fn ten_files_in_read_order(table_dir: &Path) -> io::Result<Vec<PathBuf>> {
    if !table_dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_ten_files(table_dir, &mut files)?;
    files.sort();
    files.reverse();
    Ok(files)
}

fn collect_ten_files(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_ten_files(&path, files)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("ten") {
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

fn format_error_to_io(error: tensack_format::FormatError) -> io::Error {
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
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tensack_core::{DatabaseSchema, PrimitiveType};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_root(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        dir.push(format!(
            "tensack-store-{name}-{}-{stamp}-{counter}",
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
    fn append_writes_ten_layout_and_metadata() {
        let root = temp_root("ten");
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

        let chunk = fs::read_to_string(store.chunk_ten_path("messages", 0).unwrap()).unwrap();
        assert!(!store.chunk_ten_path("messages", 1).unwrap().exists());
        assert!(chunk.starts_with("TEN\t1\ttable\tmessages\t"));
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
        assert!(metadata.contains("file = \"engine/messages.tenb\""));
        assert!(store.tenb_path("messages").exists());

        let rows = store.read_table(&schema, "messages").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].fields().get("id"), first.fields().get("id"));
        assert_eq!(rows[1].fields().get("id"), second.fields().get("id"));

        let incremental_cache = store.ensure_tenb_current(&schema, "messages").unwrap();
        let rebuilt_cache = store.rebuild_tenb(&schema, "messages").unwrap();
        assert_eq!(incremental_cache, rebuilt_cache);

        let recovered = LocalStore::new(&root, "db");
        assert_eq!(recovered.next_tx_id().unwrap(), 3);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn chunk_paths_reverse_sort_from_counter() {
        assert_eq!(chunk_path(0).unwrap(), PathBuf::from("zzz.ten"));
        assert_eq!(chunk_path(1).unwrap(), PathBuf::from("zzy.ten"));
        assert_eq!(chunk_path(2).unwrap(), PathBuf::from("zzx.ten"));
        assert_eq!(chunk_path(46_655).unwrap(), PathBuf::from("000.ten"));
        assert!(chunk_path(46_656).is_err());
    }
}
