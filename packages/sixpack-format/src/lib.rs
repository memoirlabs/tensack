//! sixpack file format primitives.
//!
//! The current durable row segment format is `.6`: tab-separated, one row per
//! line, with explicit escaping for tabs/newlines inside values. Legacy JSONL
//! helpers remain here while older tests and prototypes still use them.

use std::collections::BTreeMap;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use sixpack_core::{PrimitiveType, Record, TableSchema, Value};

/// File format version recognized by this shell.
pub const FORMAT_VERSION: u32 = 1;
pub const SIX_MAGIC: &str = "SIX";
pub const SIX_PROFILE_TABLE: &str = "table";
pub const SIXB_MAGIC: &str = "SIXB";
pub const SIXB_BINARY_VERSION: u32 = 2;
pub const SIXX_MAGIC: &str = "SIXX";

/// Internal operation type in the JSONL append log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    /// Adds/replaces row data.
    Put,
    /// Marks a row as deleted.
    Delete,
}

impl fmt::Display for Operation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Put => write!(formatter, "put"),
            Self::Delete => write!(formatter, "delete"),
        }
    }
}

/// Internal representation of one durable append entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogRecord {
    #[serde(rename = "_v")]
    pub version: u32,
    #[serde(rename = "_tx")]
    pub tx_id: u64,
    #[serde(rename = "_op")]
    pub operation: Operation,
    #[serde(rename = "_ts")]
    pub timestamp_ms: u64,
    pub table: String,
    pub data: BTreeMap<String, JsonValue>,
}

/// Error during format serialization/parsing.
#[derive(Debug)]
pub enum FormatError {
    /// Serialization failed.
    Encode(serde_json::Error),
    /// Deserialization failed.
    Decode(serde_json::Error),
    /// The log record header/version is not supported.
    UnsupportedVersion { expected: u32, found: u32 },
    /// A `.6` row has the wrong column count.
    BadSixColumnCount { expected: usize, found: usize },
    /// A `.6` file has an invalid magic/header line.
    BadSixMagic(String),
    /// A `.6` row has an invalid transaction id.
    BadSixTx(std::num::ParseIntError),
    /// A `.6` row has an invalid operation.
    BadSixOperation(String),
    /// A `.6` field value cannot be parsed as the schema type.
    BadSixValue {
        field: String,
        kind: PrimitiveType,
        value: String,
    },
    /// A `.6` value has an invalid escape sequence.
    BadSixEscape(String),
    /// A `.6` record could not be built.
    BadSixRecord(String),
    /// A `.6b` cache cannot be decoded.
    BadSixb(String),
}

impl fmt::Display for FormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(error) => write!(formatter, "encode error: {error}"),
            Self::Decode(error) => write!(formatter, "decode error: {error}"),
            Self::UnsupportedVersion { expected, found } => write!(
                formatter,
                "unsupported format version: expected {expected}, found {found}"
            ),
            Self::BadSixColumnCount { expected, found } => {
                write!(
                    formatter,
                    ".6 row column count mismatch: expected {expected}, found {found}"
                )
            }
            Self::BadSixMagic(line) => write!(formatter, ".6 file has bad magic: {line}"),
            Self::BadSixTx(error) => write!(formatter, ".6 row has invalid tx id: {error}"),
            Self::BadSixOperation(operation) => {
                write!(formatter, ".6 row has invalid operation: {operation}")
            }
            Self::BadSixValue { field, kind, value } => {
                write!(
                    formatter,
                    ".6 field `{field}` expected {kind}, got `{value}`"
                )
            }
            Self::BadSixEscape(value) => write!(formatter, ".6 value has bad escape: {value}"),
            Self::BadSixRecord(error) => write!(formatter, ".6 record error: {error}"),
            Self::BadSixb(error) => write!(formatter, ".6b cache error: {error}"),
        }
    }
}

impl std::error::Error for FormatError {}

impl LogRecord {
    /// Builds a put record from typed data.
    pub fn put(tx_id: u64, record: &Record, timestamp_ms: u64) -> Self {
        Self::new(tx_id, Operation::Put, record, timestamp_ms)
    }

    /// Builds a delete record from typed data.
    pub fn delete(tx_id: u64, record: &Record, timestamp_ms: u64) -> Self {
        Self::new(tx_id, Operation::Delete, record, timestamp_ms)
    }

    /// Creates an append entry from a typed row.
    pub fn new(tx_id: u64, operation: Operation, record: &Record, timestamp_ms: u64) -> Self {
        let data = record
            .fields()
            .iter()
            .map(|(name, value)| (name.clone(), value_to_json(value)))
            .collect();

        Self {
            version: FORMAT_VERSION,
            tx_id,
            operation,
            timestamp_ms,
            table: record.table().to_owned(),
            data,
        }
    }
}

/// Encodes a log record as one JSONL line.
pub fn encode_log_record(record: &LogRecord) -> Result<String, FormatError> {
    serde_json::to_string(record).map_err(FormatError::Encode)
}

/// Parses one JSONL line into a log record.
pub fn decode_log_record(line: &str) -> Result<LogRecord, FormatError> {
    let parsed: LogRecord = serde_json::from_str(line).map_err(FormatError::Decode)?;
    if parsed.version != FORMAT_VERSION {
        return Err(FormatError::UnsupportedVersion {
            expected: FORMAT_VERSION,
            found: parsed.version,
        });
    }
    Ok(parsed)
}

/// Returns the exact `.6` header for a table.
pub fn encode_six_header(table: &TableSchema) -> String {
    table.field_order().join("\t")
}

/// Encodes the self-describing `.6` preamble for one logical table segment.
pub fn encode_six_preamble(table: &TableSchema, schema_hash: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{SIX_MAGIC}\t{FORMAT_VERSION}\t{SIX_PROFILE_TABLE}\t{}\t{schema_hash}\n",
        escape_six_value(table.name())
    ));
    for field_name in table.field_order() {
        let field = table
            .field(field_name)
            .expect("field order only contains declared fields");
        out.push_str(&format!(
            "@field\t{}\t{}\n",
            escape_six_value(field.name()),
            <&'static str>::from(field.kind())
        ));
    }
    for lookup in table.lookup_specs_with_implicit_id() {
        out.push_str(&format!(
            "@lookup\t{}\t{}\n",
            escape_six_value(lookup.field_name()),
            if lookup.unique() { "unique" } else { "many" }
        ));
    }
    out.push_str("@data\n");
    out
}

/// Returns true when a line is the magic line for the current `.6` table profile.
pub fn is_six_magic_line(line: &str) -> bool {
    line.starts_with("SIX\t")
}

/// Encodes one `.6` data row in schema field order.
pub fn encode_six_row(table: &TableSchema, record: &Record) -> Result<String, FormatError> {
    let mut columns = Vec::with_capacity(table.field_order().len());
    for field_name in table.field_order() {
        let value = record
            .fields()
            .get(field_name)
            .ok_or_else(|| FormatError::BadSixRecord(format!("missing field `{field_name}`")))?;
        columns.push(escape_six_value(&value_to_string(value)));
    }
    Ok(columns.join("\t"))
}

/// Encodes one `.6` operation line.
pub fn encode_six_operation(
    table: &TableSchema,
    operation: Operation,
    tx_id: u64,
    record: &Record,
) -> Result<String, FormatError> {
    match operation {
        Operation::Put => {
            let row = encode_six_row(table, record)?;
            Ok(format!("R\t{tx_id}\t{row}"))
        }
        Operation::Delete => {
            let id = record
                .fields()
                .get("id")
                .ok_or_else(|| FormatError::BadSixRecord("delete missing id".to_owned()))?;
            Ok(format!(
                "D\t{tx_id}\t{}",
                escape_six_value(&value_to_string(id))
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SixOperationRecord {
    Put { tx_id: u64, record: Record },
    Delete { tx_id: u64, id: String },
}

impl SixOperationRecord {
    pub fn tx_id(&self) -> u64 {
        match self {
            Self::Put { tx_id, .. } | Self::Delete { tx_id, .. } => *tx_id,
        }
    }
}

/// Parses one current-profile `.6` operation line.
pub fn decode_six_operation(
    table: &TableSchema,
    line: &str,
) -> Result<SixOperationRecord, FormatError> {
    let mut fixed = line.splitn(3, '\t');
    let tag = fixed.next().unwrap_or_default();
    let tx = fixed.next().ok_or(FormatError::BadSixColumnCount {
        expected: 3,
        found: 1,
    })?;
    let tail = fixed.next().ok_or(FormatError::BadSixColumnCount {
        expected: 3,
        found: 2,
    })?;
    let tx_id = tx.parse::<u64>().map_err(FormatError::BadSixTx)?;
    match tag {
        "R" => Ok(SixOperationRecord::Put {
            tx_id,
            record: decode_six_row(table, tail)?,
        }),
        "D" => Ok(SixOperationRecord::Delete {
            tx_id,
            id: unescape_six_value(tail)?,
        }),
        other => Err(FormatError::BadSixOperation(other.to_owned())),
    }
}

/// Parses one `.6` row into a typed record.
pub fn decode_six_row(table: &TableSchema, line: &str) -> Result<Record, FormatError> {
    let parts: Vec<_> = line.split('\t').collect();
    let expected = table.field_order().len();
    if parts.len() != expected {
        return Err(FormatError::BadSixColumnCount {
            expected,
            found: parts.len(),
        });
    }

    let mut record = Record::new(table.name());
    for (index, field_name) in table.field_order().iter().enumerate() {
        let field = table
            .field(field_name)
            .expect("field order only contains declared fields");
        let raw = unescape_six_value(parts[index])?;
        let value = parse_ten_value(field.kind(), field_name, &raw)?;
        record
            .insert_field(field_name, value)
            .map_err(|error| FormatError::BadSixRecord(error.to_string()))?;
    }

    Ok(record)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowPointer {
    pub chunk_name: String,
    pub offset: u64,
    pub len: u32,
    pub tx_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SixbRowEntry {
    pub id: String,
    pub ptr: RowPointer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SixbLookupEntry {
    pub field_name: String,
    pub key: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SixbCache {
    pub version: u32,
    pub table: String,
    pub schema_hash: String,
    pub source_hash: String,
    pub rows: Vec<SixbRowEntry>,
    pub lookups: Vec<SixbLookupEntry>,
}

pub fn source_hash(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn encode_sixb_cache(cache: &SixbCache) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"SIXB\0");
    write_u32(&mut out, SIXB_BINARY_VERSION);
    write_string(&mut out, &cache.table);
    write_string(&mut out, &cache.schema_hash);
    write_string(&mut out, &cache.source_hash);
    write_u32(&mut out, cache.rows.len() as u32);
    for row in &cache.rows {
        write_string(&mut out, &row.id);
        write_string(&mut out, &row.ptr.chunk_name);
        write_u64(&mut out, row.ptr.offset);
        write_u32(&mut out, row.ptr.len);
        write_u64(&mut out, row.ptr.tx_id);
    }
    write_u32(&mut out, cache.lookups.len() as u32);
    for lookup in &cache.lookups {
        write_string(&mut out, &lookup.field_name);
        write_string(&mut out, &lookup.key);
        write_string(&mut out, &lookup.id);
    }
    out
}

pub fn decode_sixb_cache(bytes: &[u8]) -> Result<SixbCache, FormatError> {
    if bytes.starts_with(b"SIXB\0") {
        return decode_sixb_cache_binary(bytes);
    }
    decode_sixb_cache_text(bytes)
}

fn decode_sixb_cache_binary(bytes: &[u8]) -> Result<SixbCache, FormatError> {
    let mut reader = BinaryReader::new(bytes);
    reader.expect_magic(b"SIXB\0")?;
    let version = reader.read_u32()?;
    let table = reader.read_string()?;
    let schema_hash = reader.read_string()?;
    let source_hash = reader.read_string()?;
    let row_count = reader.read_u32()? as usize;
    let mut rows = Vec::with_capacity(row_count);
    for _ in 0..row_count {
        rows.push(SixbRowEntry {
            id: reader.read_string()?,
            ptr: RowPointer {
                chunk_name: reader.read_string()?,
                offset: reader.read_u64()?,
                len: reader.read_u32()?,
                tx_id: reader.read_u64()?,
            },
        });
    }
    let lookup_count = reader.read_u32()? as usize;
    let mut lookups = Vec::with_capacity(lookup_count);
    for _ in 0..lookup_count {
        lookups.push(SixbLookupEntry {
            field_name: reader.read_string()?,
            key: reader.read_string()?,
            id: reader.read_string()?,
        });
    }
    reader.expect_eof()?;
    Ok(SixbCache {
        version,
        table,
        schema_hash,
        source_hash,
        rows,
        lookups,
    })
}

fn decode_sixb_cache_text(bytes: &[u8]) -> Result<SixbCache, FormatError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| FormatError::BadSixb(format!("invalid utf-8: {error}")))?;
    let mut lines = text.lines();
    let header = lines
        .next()
        .ok_or_else(|| FormatError::BadSixb("missing header".to_owned()))?;
    let header_parts: Vec<_> = header.split('\t').collect();
    if header_parts.len() != 5 || header_parts[0] != SIXB_MAGIC {
        return Err(FormatError::BadSixb("bad SIXB header".to_owned()));
    }
    let version = header_parts[1]
        .parse::<u32>()
        .map_err(|error| FormatError::BadSixb(format!("bad version: {error}")))?;
    let mut cache = SixbCache {
        version,
        table: unescape_six_value(header_parts[2])?,
        schema_hash: unescape_six_value(header_parts[3])?,
        source_hash: unescape_six_value(header_parts[4])?,
        rows: Vec::new(),
        lookups: Vec::new(),
    };

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<_> = line.split('\t').collect();
        match parts.first().copied() {
            Some("row") if parts.len() == 6 => {
                cache.rows.push(SixbRowEntry {
                    id: unescape_six_value(parts[1])?,
                    ptr: RowPointer {
                        chunk_name: unescape_six_value(parts[2])?,
                        offset: parts[3].parse::<u64>().map_err(|error| {
                            FormatError::BadSixb(format!("bad row offset: {error}"))
                        })?,
                        len: parts[4].parse::<u32>().map_err(|error| {
                            FormatError::BadSixb(format!("bad row len: {error}"))
                        })?,
                        tx_id: parts[5].parse::<u64>().map_err(|error| {
                            FormatError::BadSixb(format!("bad row tx: {error}"))
                        })?,
                    },
                });
            }
            Some("lookup") if parts.len() == 4 => {
                cache.lookups.push(SixbLookupEntry {
                    field_name: unescape_six_value(parts[1])?,
                    key: unescape_six_value(parts[2])?,
                    id: unescape_six_value(parts[3])?,
                });
            }
            _ => return Err(FormatError::BadSixb(format!("bad entry: {line}"))),
        }
    }

    Ok(cache)
}

struct BinaryReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> BinaryReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn expect_magic(&mut self, magic: &[u8]) -> Result<(), FormatError> {
        let actual = self.take(magic.len())?;
        if actual == magic {
            Ok(())
        } else {
            Err(FormatError::BadSixb("bad SIXB binary magic".to_owned()))
        }
    }

    fn read_u32(&mut self) -> Result<u32, FormatError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes(
            bytes.try_into().expect("take returned exact u32 width"),
        ))
    }

    fn read_u64(&mut self) -> Result<u64, FormatError> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes(
            bytes.try_into().expect("take returned exact u64 width"),
        ))
    }

    fn read_string(&mut self) -> Result<String, FormatError> {
        let len = self.read_u32()? as usize;
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|error| FormatError::BadSixb(format!("invalid utf-8 string: {error}")))
    }

    fn expect_eof(&self) -> Result<(), FormatError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(FormatError::BadSixb("trailing SIXB bytes".to_owned()))
        }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], FormatError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| FormatError::BadSixb("SIXB offset overflow".to_owned()))?;
        if end > self.bytes.len() {
            return Err(FormatError::BadSixb("truncated SIXB binary".to_owned()));
        }
        let out = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(out)
    }
}

fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_string(out: &mut Vec<u8>, value: &str) {
    write_u32(out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
}

/// Escapes one `.6` field value.
pub fn escape_six_value(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out
}

/// Unescapes one `.6` field value.
pub fn unescape_six_value(value: &str) -> Result<String, FormatError> {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(escaped) = chars.next() else {
            return Err(FormatError::BadSixEscape("dangling \\".to_owned()));
        };
        match escaped {
            '\\' => out.push('\\'),
            't' => out.push('\t'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            other => return Err(FormatError::BadSixEscape(format!("\\{other}"))),
        }
    }
    Ok(out)
}

/// Produces a stable now-ms timestamp for log entries.
pub fn now_ms() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(now) => now.as_millis() as u64,
        Err(_) => 0,
    }
}

fn value_to_json(value: &Value) -> JsonValue {
    match value {
        Value::Id(value) => JsonValue::String(value.clone()),
        Value::Text(value) => JsonValue::String(value.clone()),
        Value::Int(value) => JsonValue::from(*value),
        Value::Float(value) => JsonValue::from(*value),
        Value::Bool(value) => JsonValue::from(*value),
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::Id(value) => value.clone(),
        Value::Text(value) => value.clone(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
    }
}

fn parse_ten_value(kind: PrimitiveType, field: &str, value: &str) -> Result<Value, FormatError> {
    match kind {
        PrimitiveType::Id => Ok(Value::Id(value.to_owned())),
        PrimitiveType::Text => Ok(Value::Text(value.to_owned())),
        PrimitiveType::Int => {
            value
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|_| FormatError::BadSixValue {
                    field: field.to_owned(),
                    kind,
                    value: value.to_owned(),
                })
        }
        PrimitiveType::Float => {
            value
                .parse::<f64>()
                .map(Value::Float)
                .map_err(|_| FormatError::BadSixValue {
                    field: field.to_owned(),
                    kind,
                    value: value.to_owned(),
                })
        }
        PrimitiveType::Bool => {
            value
                .parse::<bool>()
                .map(Value::Bool)
                .map_err(|_| FormatError::BadSixValue {
                    field: field.to_owned(),
                    kind,
                    value: value.to_owned(),
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sixpack_core::{DatabaseSchema, PrimitiveType, Record, TableSchema};

    #[test]
    fn log_record_round_trip() {
        let mut table = TableSchema::new("messages");
        table.add_field("id", PrimitiveType::Id).unwrap();
        table.add_field("body", PrimitiveType::Text).unwrap();
        let mut schema = DatabaseSchema::new();
        schema.add_table(table).unwrap();

        let record = Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", "hello")
            .unwrap();
        let encoded = encode_log_record(&LogRecord::put(1, &record, now_ms())).unwrap();
        let decoded = decode_log_record(&encoded).unwrap();
        assert_eq!(decoded.version, FORMAT_VERSION);
        assert_eq!(decoded.tx_id, 1);
        assert_eq!(decoded.operation, Operation::Put);
        assert_eq!(decoded.table, "messages");
        assert_eq!(
            decoded.data.get("id"),
            Some(&JsonValue::String("m1".to_string()))
        );
        assert!(schema.table("messages").is_some());
    }

    #[test]
    fn six_row_round_trip() {
        let mut table = TableSchema::new("messages");
        table.add_field("id", PrimitiveType::Id).unwrap();
        table.add_field("body", PrimitiveType::Text).unwrap();
        table.add_field("created_at", PrimitiveType::Int).unwrap();

        let record = Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", "hello\tworld\nagain")
            .unwrap()
            .with_field("created_at", 42i64)
            .unwrap();

        assert_eq!(encode_six_header(&table), "id\tbody\tcreated_at");
        let encoded = encode_six_row(&table, &record).unwrap();
        assert_eq!(encoded, "m1\thello\\tworld\\nagain\t42");
        let decoded = decode_six_row(&table, &encoded).unwrap();
        assert_eq!(decoded.fields(), record.fields());
    }

    #[test]
    fn sixb_binary_round_trip() {
        let cache = SixbCache {
            version: SIXB_BINARY_VERSION,
            table: "messages".to_owned(),
            schema_hash: "schema".to_owned(),
            source_hash: "source".to_owned(),
            rows: vec![SixbRowEntry {
                id: "m1".to_owned(),
                ptr: RowPointer {
                    chunk_name: "zzz.6".to_owned(),
                    offset: 42,
                    len: 18,
                    tx_id: 7,
                },
            }],
            lookups: vec![SixbLookupEntry {
                field_name: "conversation_id".to_owned(),
                key: "cv1".to_owned(),
                id: "m1".to_owned(),
            }],
        };

        let encoded = encode_sixb_cache(&cache);
        assert!(encoded.starts_with(b"SIXB\0"));
        assert_eq!(
            u32::from_le_bytes(encoded[5..9].try_into().unwrap()),
            SIXB_BINARY_VERSION
        );
        let decoded = decode_sixb_cache(&encoded).unwrap();
        assert_eq!(decoded, cache);
    }

    #[test]
    fn legacy_text_sixb_still_decodes() {
        let legacy = b"SIXB\t1\tmessages\tschema\tsource\nrow\tm1\tzzz.6\t42\t18\t7\nlookup\tconversation_id\tcv1\tm1\n";
        let decoded = decode_sixb_cache(legacy).unwrap();
        assert_eq!(decoded.version, 1);
        assert_eq!(decoded.table, "messages");
        assert_eq!(decoded.rows.len(), 1);
        assert_eq!(decoded.lookups.len(), 1);
    }
}
