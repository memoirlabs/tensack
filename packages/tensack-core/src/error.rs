use std::fmt;

use crate::value::PrimitiveType;

#[derive(Debug, Clone, PartialEq)]
pub enum SchemaError {
    ReservedFieldName(String),
    InvalidFieldName(String),
    UnknownTable(String),
    UnknownField {
        table: String,
        field: String,
    },
    TypeMismatch {
        table: String,
        field: String,
        expected: PrimitiveType,
        found: PrimitiveType,
    },
    DuplicateField {
        table: String,
        field: String,
    },
    DuplicateLookup {
        table: String,
        field: String,
    },
    DuplicateTable(String),
    MissingIdField {
        table: String,
    },
    IdFieldMustBeId {
        table: String,
    },
    UnknownLookupField {
        table: String,
        field: String,
    },
    MissingField {
        table: String,
        field: String,
    },
}

impl SchemaError {
    pub(crate) fn invalid_field_name(name: impl Into<String>) -> Self {
        Self::InvalidFieldName(name.into())
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReservedFieldName(field) => write!(
                formatter,
                "field name is reserved for internal storage metadata: {field}"
            ),
            Self::InvalidFieldName(field) => write!(formatter, "invalid field name: {field}"),
            Self::UnknownTable(table) => write!(formatter, "unknown table: {table}"),
            Self::UnknownField { table, field } => {
                write!(formatter, "table `{table}` has no field `{field}`")
            }
            Self::TypeMismatch {
                table,
                field,
                expected,
                found,
            } => write!(
                formatter,
                "table `{table}` field `{field}` type mismatch: expected {expected}, found {found}"
            ),
            Self::DuplicateField { table, field } => {
                write!(formatter, "table `{table}` already defines field `{field}`")
            }
            Self::DuplicateLookup { table, field } => {
                write!(
                    formatter,
                    "table `{table}` already defines lookup `{field}`"
                )
            }
            Self::DuplicateTable(table) => write!(formatter, "table `{table}` already exists"),
            Self::MissingIdField { table } => {
                write!(formatter, "table `{table}` must include an `id` field")
            }
            Self::IdFieldMustBeId { table } => {
                write!(
                    formatter,
                    "table `{table}` field `id` must use primitive type `id`"
                )
            }
            Self::UnknownLookupField { table, field } => {
                write!(formatter, "table `{table}` lookup `{field}` has no field")
            }
            Self::MissingField { table, field } => {
                write!(
                    formatter,
                    "table `{table}` is missing required field `{field}`"
                )
            }
        }
    }
}

impl std::error::Error for SchemaError {}

pub(crate) fn ensure_public_field_name(name: &str) -> Result<(), SchemaError> {
    if name.is_empty() {
        return Err(SchemaError::invalid_field_name("<empty>"));
    }
    if name.starts_with('_') {
        return Err(SchemaError::ReservedFieldName(name.to_owned()));
    }
    Ok(())
}
