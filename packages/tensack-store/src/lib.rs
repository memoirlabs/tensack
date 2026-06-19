//! Local storage engine boundary.
//!
//! The current store writes readable `.ten` table row segments, keeps a small
//! `tensack.toml` physical layout map, and rebuilds generated `.tenb` lookup
//! caches from canonical `.ten` data.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use tensack_core::{DatabaseSchema, Record, TableSchema, Value};
use tensack_format::{
    Operation, RowPointer, TENB_BINARY_VERSION, TenOperationRecord, TenbCache, TenbLookupEntry,
    TenbRowEntry, decode_ten_operation, decode_ten_row, decode_tenb_cache, encode_ten_header,
    encode_ten_operation, encode_ten_preamble, encode_tenb_cache, is_ten_magic_line, source_hash,
};

const CHUNK_CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
const CHUNK_BASE: usize = 36;
const GENERATION_WIDTH: usize = 2;
const CHUNK_WIDTH: usize = 3;
const CHUNKS_PER_GENERATION: u64 = 36u64.pow(CHUNK_WIDTH as u32);
const MAX_GENERATIONS: u64 = 36u64.pow(GENERATION_WIDTH as u32);
const MAX_CHUNKS: u64 = CHUNKS_PER_GENERATION * MAX_GENERATIONS;

/// Local store handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalStore {
    root: PathBuf,
    workspace: String,
}

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

impl LocalStore {
    /// Creates a store handle without touching the filesystem.
    pub fn new(root: impl Into<PathBuf>, workspace: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            workspace: workspace.into(),
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
        self.ensure_workspace_layout()?;
        let table = schema
            .table(record.table())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        self.ensure_table_layout(table, &schema.schema_hash())?;
        let id = record_id(record)?;
        if self.get_by_id(schema, table.name(), &id)?.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("row `{}` already exists in `{}`", id, table.name()),
            ));
        }
        self.append(schema, Operation::Put, record)
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
        self.ensure_workspace_layout()?;
        let table = schema
            .table(record.table())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        self.ensure_table_layout(table, &schema.schema_hash())?;
        if operation == Operation::Put {
            self.validate_unique_lookup_conflicts(schema, table, record)?;
        }

        let tx_id = self.next_tx_id()?;
        let line =
            encode_ten_operation(table, operation, tx_id, record).map_err(format_error_to_io)?;
        let bytes_written = (line.len() + 1) as u64;
        let chunk_counter = self.next_chunk_counter(table.name())?;
        let chunk_relative_path = chunk_path(chunk_counter)?;
        let chunk_path = self.table_dir(table.name()).join(&chunk_relative_path);
        if let Some(parent) = chunk_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&chunk_path)?;
        file.write_all(encode_ten_preamble(table, &schema.schema_hash()).as_bytes())?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;

        self.rebuild_tenb(schema, table.name())?;
        self.write_metadata(schema, tx_id.saturating_add(1))?;

        Ok(AppendResult {
            tx_id,
            operation,
            bytes_written,
        })
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
        let metadata = self.metadata_path();
        if !metadata.exists() {
            return Ok(1);
        }
        let file = File::open(metadata)?;
        for line in BufReader::new(file).lines() {
            let line = line?;
            let Some(value) = line.strip_prefix("next_tx = ") else {
                continue;
            };
            return value.trim().parse::<u64>().map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidData, format!("bad next_tx: {error}"))
            });
        }
        Ok(1)
    }

    /// Computes the next chunk counter for one table from metadata, falling back to files.
    pub fn next_chunk_counter(&self, table_name: &str) -> io::Result<u64> {
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
                    return value
                        .trim()
                        .parse::<u64>()
                        .map_err(|error| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!("bad next_chunk for `{table_name}`: {error}"),
                            )
                        })
                        .map(|stored_next| stored_next.max(discovered_next));
                }
            }
        }
        Ok(discovered_next)
    }

    /// Reads all current live records from a table using the generated `.tenb` cache.
    pub fn read_table(&self, schema: &DatabaseSchema, table_name: &str) -> io::Result<Vec<Record>> {
        let cache = self.ensure_tenb_current(schema, table_name)?;
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;

        let mut rows = Vec::with_capacity(cache.rows.len());
        for entry in cache.rows {
            rows.push(self.read_row_pointer(table, &entry.ptr)?);
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
        let cache = self.ensure_tenb_current(schema, table_name)?;
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        let Some(entry) = cache.rows.iter().find(|entry| entry.id == id) else {
            return Ok(None);
        };
        self.read_row_pointer(table, &entry.ptr).map(Some)
    }

    /// Reads rows by a declared lookup field. Unique lookup callers should use the first item.
    pub fn get_by_lookup(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        field_name: &str,
        key: &str,
    ) -> io::Result<Vec<Record>> {
        let cache = self.ensure_tenb_current(schema, table_name)?;
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        if field_name != "id" && table.lookup(field_name).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown lookup `{field_name}` for table `{table_name}`"),
            ));
        }
        let ids: BTreeSet<_> = cache
            .lookups
            .iter()
            .filter(|entry| entry.field_name == field_name && entry.key == key)
            .map(|entry| entry.id.as_str())
            .collect();
        let mut rows = Vec::new();
        for id in ids {
            if let Some(row_entry) = cache.rows.iter().find(|entry| entry.id == id) {
                rows.push(self.read_row_pointer(table, &row_entry.ptr)?);
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
        let cache = self.ensure_tenb_current(schema, table_name)?;
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        let mut rows = Vec::new();
        for entry in cache.rows.iter().skip(offset).take(limit) {
            rows.push(self.read_row_pointer(table, &entry.ptr)?);
        }
        Ok(rows)
    }

    /// Counts current live rows in one table.
    pub fn count_table(&self, schema: &DatabaseSchema, table_name: &str) -> io::Result<usize> {
        let cache = self.ensure_tenb_current(schema, table_name)?;
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
        let cache = self.ensure_tenb_current(schema, table_name)?;
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        if field_name != "id" && table.lookup(field_name).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown lookup `{field_name}` for table `{table_name}`"),
            ));
        }
        Ok(cache
            .lookups
            .iter()
            .filter(|entry| entry.field_name == field_name && entry.key == key)
            .count())
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

        let cache = TenbCache {
            version: TENB_BINARY_VERSION,
            table: table.name().to_owned(),
            schema_hash: schema.schema_hash(),
            source_hash: scan.source_hash,
            rows,
            lookups,
        };
        let path = self.tenb_path(table.name());
        let tmp = path.with_extension("tenb.tmp");
        fs::write(&tmp, encode_tenb_cache(&cache))?;
        fs::rename(tmp, path)?;
        Ok(cache)
    }

    /// Loads `.tenb` if valid, otherwise rebuilds it from `.ten`.
    pub fn ensure_tenb_current(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
    ) -> io::Result<TenbCache> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        let scan = self.scan_table_files(table)?;
        let path = self.tenb_path(table.name());
        if path.exists() {
            let bytes = fs::read(&path)?;
            if let Ok(cache) = decode_tenb_cache(&bytes)
                && cache.version == TENB_BINARY_VERSION
                && cache.table == table.name()
                && cache.schema_hash == schema.schema_hash()
                && cache.source_hash == scan.source_hash
            {
                return Ok(cache);
            }
        }
        self.rebuild_tenb(schema, table_name)
    }

    fn ensure_table_layout(&self, table: &TableSchema, _schema_hash: &str) -> io::Result<()> {
        fs::create_dir_all(self.table_dir(table.name()))?;
        for path in ten_files_in_read_order(&self.table_dir(table.name()))? {
            verify_header(table, &path)?;
        }
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
                "chunks = [{}]\n",
                chunk_segments_toml(&self.table_dir(table.name()))?
            ));
            out.push_str(&format!(
                "header = \"{}\"\n\n",
                escape_toml(&encode_ten_header(table))
            ));
            out.push_str(&format!("[tables.{}.index]\n", table.name()));
            out.push_str("state = \"ready\"\n");
            out.push_str(&format!("file = \"engine/{}.tenb\"\n\n", table.name()));
        }

        fs::write(&tmp, out)?;
        fs::rename(tmp, self.metadata_path())
    }

    fn scan_table_files(&self, table: &TableSchema) -> io::Result<TableScan> {
        let mut live = BTreeMap::new();
        let mut hash_bytes = Vec::new();
        for path in ten_files_in_read_order(&self.table_dir(table.name()))? {
            let chunk_name = relative_chunk_name(&self.table_dir(table.name()), &path)?;
            let bytes = fs::read(&path)?;
            hash_bytes.extend_from_slice(chunk_name.as_bytes());
            hash_bytes.push(0);
            hash_bytes.extend_from_slice(&bytes);
            let entries = scan_ten_file(table, &path, &chunk_name)?;
            for entry in entries {
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

    fn read_row_pointer(&self, table: &TableSchema, ptr: &RowPointer) -> io::Result<Record> {
        let path = self.table_dir(table.name()).join(&ptr.chunk_name);
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(ptr.offset))?;
        let mut bytes = vec![0; ptr.len as usize];
        file.read_exact(&mut bytes)?;
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

    fn validate_unique_lookup_conflicts(
        &self,
        schema: &DatabaseSchema,
        table: &TableSchema,
        record: &Record,
    ) -> io::Result<()> {
        let id = record_id(record)?;
        let cache = self.ensure_tenb_current(schema, table.name())?;
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
            if let Some(conflict) = cache
                .lookups
                .iter()
                .find(|entry| entry.field_name == lookup.field_name() && entry.key == key)
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
        });
    }
    Ok(out)
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

fn chunk_path(global_chunk_counter: u64) -> io::Result<PathBuf> {
    if global_chunk_counter >= MAX_CHUNKS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("chunk counter must be between 0 and {}", MAX_CHUNKS - 1),
        ));
    }

    let generation = global_chunk_counter / CHUNKS_PER_GENERATION;
    let local_chunk = global_chunk_counter % CHUNKS_PER_GENERATION;
    let folder = encode_reverse_base36(generation as usize, GENERATION_WIDTH)?;
    let file = encode_reverse_base36(local_chunk as usize, CHUNK_WIDTH)?;
    Ok(PathBuf::from(folder).join(format!("{file}.ten")))
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

fn chunk_segments_toml(table_dir: &Path) -> io::Result<String> {
    if !table_dir.exists() {
        return Ok(String::new());
    }
    let mut segments = Vec::new();
    for path in ten_files_in_read_order(table_dir)? {
        segments.push(relative_chunk_name(table_dir, &path)?);
    }
    Ok(segments
        .into_iter()
        .map(|segment| format!("\"{}\"", escape_toml(&segment)))
        .collect::<Vec<_>>()
        .join(", "))
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

        let first_chunk = fs::read_to_string(store.chunk_ten_path("messages", 0).unwrap()).unwrap();
        let second_chunk =
            fs::read_to_string(store.chunk_ten_path("messages", 1).unwrap()).unwrap();
        assert!(first_chunk.starts_with("TEN\t1\ttable\tmessages\t"));
        assert!(first_chunk.contains("@field\tid\tid\n"));
        assert!(first_chunk.contains("@field\tbody\ttext\n"));
        assert!(first_chunk.contains("@lookup\tid\tunique\n"));
        assert!(first_chunk.contains("@lookup\tcreated_at\tmany\n"));
        assert!(first_chunk.contains("@data\n"));
        assert!(first_chunk.contains("R\t1\tm1\thello\\tworld\t1\n"));
        assert!(second_chunk.contains("R\t2\tm2\tline\\nbreak\t2\n"));
        assert!(!first_chunk.contains("\tput\t"));

        let metadata = fs::read_to_string(store.metadata_path()).unwrap();
        assert!(metadata.contains("[tables.messages]"));
        assert!(metadata.contains("next_tx = 3"));
        assert!(metadata.contains("next_chunk = 2"));
        assert!(metadata.contains("chunks = [\"zz/zzz.ten\", \"zz/zzy.ten\"]"));
        assert!(metadata.contains("file = \"engine/messages.tenb\""));
        assert!(store.tenb_path("messages").exists());

        let rows = store.read_table(&schema, "messages").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].fields().get("id"), first.fields().get("id"));
        assert_eq!(rows[1].fields().get("id"), second.fields().get("id"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn chunk_paths_reverse_sort_from_counter() {
        assert_eq!(chunk_path(0).unwrap(), PathBuf::from("zz").join("zzz.ten"));
        assert_eq!(chunk_path(1).unwrap(), PathBuf::from("zz").join("zzy.ten"));
        assert_eq!(
            chunk_path(46_655).unwrap(),
            PathBuf::from("zz").join("000.ten")
        );
        assert_eq!(
            chunk_path(46_656).unwrap(),
            PathBuf::from("zy").join("zzz.ten")
        );
        assert_eq!(
            chunk_path(93_312).unwrap(),
            PathBuf::from("zx").join("zzz.ten")
        );
    }
}
