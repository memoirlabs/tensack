//! Local storage engine boundary.
//!
//! The current store writes readable `.ten` table row segments, keeps a small
//! `tensack.toml` physical layout map, and creates rebuildable `.btf` index
//! placeholders for declared lookup/index state.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use tensack_core::{DatabaseSchema, Record, TableSchema};
use tensack_format::{Operation, decode_ten_row, encode_ten_header, encode_ten_row};

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

    /// Active `.ten` row segment for a table.
    pub fn active_ten_path(&self, table: &str) -> PathBuf {
        self.table_dir(table).join("active.ten")
    }

    /// Binary Tensack file for table indexes/lookups.
    pub fn btf_path(&self, table: &str) -> PathBuf {
        self.database_dir()
            .join("engine")
            .join(format!("{table}.btf"))
    }

    /// Appends a put event to the `.ten` table segment.
    pub fn append_put(&self, schema: &DatabaseSchema, record: &Record) -> io::Result<AppendResult> {
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
        self.ensure_table_layout(table)?;

        let tx_id = self.next_tx_id()?;
        let bytes_written = if operation == Operation::Put {
            let line = encode_ten_row(table, record).map_err(format_error_to_io)?;
            let bytes_written = (line.len() + 1) as u64;
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.active_ten_path(table.name()))?;
            file.write_all(line.as_bytes())?;
            file.write_all(b"\n")?;
            bytes_written
        } else {
            0
        };

        self.write_btf_placeholder(table)?;
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

    /// Reads all typed records from a table's `.ten` files in filename order.
    pub fn read_table(&self, schema: &DatabaseSchema, table_name: &str) -> io::Result<Vec<Record>> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        let mut files = ten_files_in_read_order(&self.table_dir(table.name()))?;
        let active = self.active_ten_path(table.name());
        files.retain(|path| path != &active);
        if active.exists() {
            files.push(active);
        }

        let mut rows = Vec::new();
        for path in files {
            rows.extend(read_ten_file(table, &path)?);
        }
        Ok(rows)
    }

    fn ensure_table_layout(&self, table: &TableSchema) -> io::Result<()> {
        fs::create_dir_all(self.table_dir(table.name()))?;
        let active = self.active_ten_path(table.name());
        if !active.exists() {
            let mut file = File::create(&active)?;
            file.write_all(encode_ten_header(table).as_bytes())?;
            file.write_all(b"\n")?;
        } else {
            verify_header(table, &active)?;
        }
        Ok(())
    }

    fn write_btf_placeholder(&self, table: &TableSchema) -> io::Result<()> {
        let path = self.btf_path(table.name());
        if path.exists() {
            return Ok(());
        }
        let mut file = File::create(path)?;
        file.write_all(b"BTF0\n")?;
        file.write_all(format!("table={}\n", table.name()).as_bytes())?;
        file.write_all(b"state=placeholder\n")
    }

    fn write_metadata(&self, schema: &DatabaseSchema, next_tx: u64) -> io::Result<()> {
        let tmp = self.metadata_path().with_extension("toml.tmp");
        let mut out = String::new();
        out.push_str("version = 1\n");
        out.push_str("schema_hash = \"dev\"\n");
        out.push_str(&format!("next_tx = {next_tx}\n\n"));

        for (index, table) in schema.tables().values().enumerate() {
            let table_id = index + 1;
            out.push_str(&format!("[tables.{}]\n", table.name()));
            out.push_str(&format!("id = {table_id}\n"));
            out.push_str(&format!("path = \"tables/{}\"\n", table.name()));
            out.push_str("active = \"active.ten\"\n");
            out.push_str(&format!(
                "segments = [{}]\n",
                sealed_segments_toml(&self.table_dir(table.name()))?
            ));
            out.push_str(&format!(
                "header = \"{}\"\n\n",
                escape_toml(&encode_ten_header(table))
            ));
            out.push_str(&format!("[tables.{}.index]\n", table.name()));
            out.push_str("state = \"placeholder\"\n");
            out.push_str(&format!("file = \"engine/{}.btf\"\n\n", table.name()));
        }

        fs::write(&tmp, out)?;
        fs::rename(tmp, self.metadata_path())
    }
}

fn read_ten_file(table: &TableSchema, path: &Path) -> io::Result<Vec<Record>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut header = String::new();
    reader.read_line(&mut header)?;
    if header.trim_end_matches(['\r', '\n']) != encode_ten_header(table) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad .ten header in {}", path.display()),
        ));
    }

    let mut out = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        out.push(decode_ten_row(table, &line).map_err(format_error_to_io)?);
    }
    Ok(out)
}

fn verify_header(table: &TableSchema, path: &Path) -> io::Result<()> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut header = String::new();
    reader.read_line(&mut header)?;
    let actual = header.trim_end_matches(['\r', '\n']);
    let expected = encode_ten_header(table);
    if actual == expected {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected .ten header `{expected}`, found `{actual}`"),
        ))
    }
}

fn ten_files_in_read_order(table_dir: &Path) -> io::Result<Vec<PathBuf>> {
    if !table_dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(table_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("ten") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn sealed_segments_toml(table_dir: &Path) -> io::Result<String> {
    if !table_dir.exists() {
        return Ok(String::new());
    }
    let mut segments = Vec::new();
    for entry in fs::read_dir(table_dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name != "active.ten" && name.ends_with(".ten") {
            segments.push(name.to_owned());
        }
    }
    segments.sort();
    Ok(segments
        .into_iter()
        .map(|segment| format!("\"{}\"", escape_toml(&segment)))
        .collect::<Vec<_>>()
        .join(", "))
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
    use std::time::{SystemTime, UNIX_EPOCH};
    use tensack_core::{DatabaseSchema, PrimitiveType};

    fn temp_root(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        dir.push(format!("tensack-store-{name}-{stamp}"));
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

        let ten = fs::read_to_string(store.active_ten_path("messages")).unwrap();
        assert!(ten.starts_with("id\tbody\tcreated_at\n"));
        assert!(ten.contains("m1\thello\\tworld\t1\n"));
        assert!(ten.contains("m2\tline\\nbreak\t2\n"));
        assert!(!ten.contains("\tput\t"));

        let metadata = fs::read_to_string(store.metadata_path()).unwrap();
        assert!(metadata.contains("[tables.messages]"));
        assert!(metadata.contains("next_tx = 3"));
        assert!(metadata.contains("active = \"active.ten\""));
        assert!(metadata.contains("file = \"engine/messages.btf\""));
        assert!(store.btf_path("messages").exists());

        let rows = store.read_table(&schema, "messages").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].fields().get("id"), first.fields().get("id"));
        assert_eq!(rows[1].fields().get("id"), second.fields().get("id"));
        let _ = fs::remove_dir_all(root);
    }
}
