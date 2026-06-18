//! Tensack file format primitives.
//!
//! The current durable row segment format is `.ten`: tab-separated, one row per
//! line, with explicit escaping for tabs/newlines inside values. Legacy JSONL
//! helpers remain here while older tests and prototypes still use them.

use std::collections::BTreeMap;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use tensack_core::{PrimitiveType, Record, SackValue, TableSchema};

/// File format version recognized by this shell.
pub const FORMAT_VERSION: u32 = 1;

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
    pub data: BTreeMap<String, Value>,
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
    /// A `.ten` row has the wrong column count.
    BadTenColumnCount { expected: usize, found: usize },
    /// A `.ten` row has an invalid transaction id.
    BadTenTx(std::num::ParseIntError),
    /// A `.ten` row has an invalid operation.
    BadTenOperation(String),
    /// A `.ten` field value cannot be parsed as the schema type.
    BadTenValue {
        field: String,
        kind: PrimitiveType,
        value: String,
    },
    /// A `.ten` value has an invalid escape sequence.
    BadTenEscape(String),
    /// A `.ten` record could not be built.
    BadTenRecord(String),
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
            Self::BadTenColumnCount { expected, found } => {
                write!(
                    formatter,
                    ".ten row column count mismatch: expected {expected}, found {found}"
                )
            }
            Self::BadTenTx(error) => write!(formatter, ".ten row has invalid tx id: {error}"),
            Self::BadTenOperation(operation) => {
                write!(formatter, ".ten row has invalid operation: {operation}")
            }
            Self::BadTenValue { field, kind, value } => {
                write!(
                    formatter,
                    ".ten field `{field}` expected {kind}, got `{value}`"
                )
            }
            Self::BadTenEscape(value) => write!(formatter, ".ten value has bad escape: {value}"),
            Self::BadTenRecord(error) => write!(formatter, ".ten record error: {error}"),
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

/// Returns the exact `.ten` header for a table.
pub fn encode_ten_header(table: &TableSchema) -> String {
    table.field_order().join("\t")
}

/// Encodes one `.ten` data row in schema field order.
pub fn encode_ten_row(table: &TableSchema, record: &Record) -> Result<String, FormatError> {
    let mut columns = Vec::with_capacity(table.field_order().len());
    for field_name in table.field_order() {
        let value = record
            .fields()
            .get(field_name)
            .ok_or_else(|| FormatError::BadTenRecord(format!("missing field `{field_name}`")))?;
        columns.push(escape_ten_value(&value_to_string(value)));
    }
    Ok(columns.join("\t"))
}

/// Parses one `.ten` row into a typed record.
pub fn decode_ten_row(table: &TableSchema, line: &str) -> Result<Record, FormatError> {
    let parts: Vec<_> = line.split('\t').collect();
    let expected = table.field_order().len();
    if parts.len() != expected {
        return Err(FormatError::BadTenColumnCount {
            expected,
            found: parts.len(),
        });
    }

    let mut record = Record::new(table.name());
    for (index, field_name) in table.field_order().iter().enumerate() {
        let field = table
            .field(field_name)
            .expect("field order only contains declared fields");
        let raw = unescape_ten_value(parts[index])?;
        let value = parse_ten_value(field.kind(), field_name, &raw)?;
        record
            .insert_field(field_name, value)
            .map_err(|error| FormatError::BadTenRecord(error.to_string()))?;
    }

    Ok(record)
}

/// Escapes one `.ten` field value.
pub fn escape_ten_value(value: &str) -> String {
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

/// Unescapes one `.ten` field value.
pub fn unescape_ten_value(value: &str) -> Result<String, FormatError> {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(escaped) = chars.next() else {
            return Err(FormatError::BadTenEscape("dangling \\".to_owned()));
        };
        match escaped {
            '\\' => out.push('\\'),
            't' => out.push('\t'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            other => return Err(FormatError::BadTenEscape(format!("\\{other}"))),
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

fn value_to_json(value: &SackValue) -> Value {
    match value {
        SackValue::Id(value) => Value::String(value.clone()),
        SackValue::Text(value) => Value::String(value.clone()),
        SackValue::Int(value) => Value::from(*value),
        SackValue::Float(value) => Value::from(*value),
        SackValue::Bool(value) => Value::from(*value),
    }
}

fn value_to_string(value: &SackValue) -> String {
    match value {
        SackValue::Id(value) => value.clone(),
        SackValue::Text(value) => value.clone(),
        SackValue::Int(value) => value.to_string(),
        SackValue::Float(value) => value.to_string(),
        SackValue::Bool(value) => value.to_string(),
    }
}

fn parse_ten_value(
    kind: PrimitiveType,
    field: &str,
    value: &str,
) -> Result<SackValue, FormatError> {
    match kind {
        PrimitiveType::Id => Ok(SackValue::Id(value.to_owned())),
        PrimitiveType::Text => Ok(SackValue::Text(value.to_owned())),
        PrimitiveType::Int => {
            value
                .parse::<i64>()
                .map(SackValue::Int)
                .map_err(|_| FormatError::BadTenValue {
                    field: field.to_owned(),
                    kind,
                    value: value.to_owned(),
                })
        }
        PrimitiveType::Float => {
            value
                .parse::<f64>()
                .map(SackValue::Float)
                .map_err(|_| FormatError::BadTenValue {
                    field: field.to_owned(),
                    kind,
                    value: value.to_owned(),
                })
        }
        PrimitiveType::Bool => {
            value
                .parse::<bool>()
                .map(SackValue::Bool)
                .map_err(|_| FormatError::BadTenValue {
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
    use tensack_core::{DatabaseSchema, PrimitiveType, Record, TableSchema};

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
            Some(&Value::String("m1".to_string()))
        );
        assert!(schema.table("messages").is_some());
    }

    #[test]
    fn ten_row_round_trip() {
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

        assert_eq!(encode_ten_header(&table), "id\tbody\tcreated_at");
        let encoded = encode_ten_row(&table, &record).unwrap();
        assert_eq!(encoded, "m1\thello\\tworld\\nagain\t42");
        let decoded = decode_ten_row(&table, &encoded).unwrap();
        assert_eq!(decoded.fields(), record.fields());
    }
}
