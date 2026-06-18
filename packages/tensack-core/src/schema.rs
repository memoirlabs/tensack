use std::collections::BTreeMap;

use crate::error::{SchemaError, ensure_public_field_name};
use crate::record::Record;
use crate::value::PrimitiveType;

#[derive(Debug, Clone, PartialEq)]
pub struct FieldSpec {
    name: String,
    kind: PrimitiveType,
}

impl FieldSpec {
    pub fn new(name: impl Into<String>, kind: PrimitiveType) -> Result<Self, SchemaError> {
        let name = name.into();
        ensure_public_field_name(&name)?;
        Ok(Self { name, kind })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> PrimitiveType {
        self.kind
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableSchema {
    name: String,
    field_order: Vec<String>,
    fields: BTreeMap<String, FieldSpec>,
    lookups: BTreeMap<String, LookupSpec>,
}

impl TableSchema {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            field_order: Vec::new(),
            fields: BTreeMap::new(),
            lookups: BTreeMap::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn add_field(
        &mut self,
        name: impl Into<String>,
        kind: PrimitiveType,
    ) -> Result<(), SchemaError> {
        let spec = FieldSpec::new(name, kind)?;
        if self.fields.contains_key(&spec.name) {
            return Err(SchemaError::DuplicateField {
                table: self.name.clone(),
                field: spec.name,
            });
        }
        self.field_order.push(spec.name.clone());
        self.fields.insert(spec.name.clone(), spec);
        Ok(())
    }

    pub fn field(&self, name: &str) -> Option<&FieldSpec> {
        self.fields.get(name)
    }

    pub fn fields(&self) -> &BTreeMap<String, FieldSpec> {
        &self.fields
    }

    pub fn field_order(&self) -> &[String] {
        &self.field_order
    }

    pub fn add_lookup(
        &mut self,
        field_name: impl Into<String>,
        unique: bool,
    ) -> Result<(), SchemaError> {
        let spec = LookupSpec::new(field_name, unique)?;
        if self.lookups.contains_key(spec.field_name()) {
            return Err(SchemaError::DuplicateLookup {
                table: self.name.clone(),
                field: spec.field_name().to_owned(),
            });
        }
        self.lookups.insert(spec.field_name().to_owned(), spec);
        Ok(())
    }

    pub fn lookup(&self, field_name: &str) -> Option<&LookupSpec> {
        self.lookups.get(field_name)
    }

    pub fn lookups(&self) -> &BTreeMap<String, LookupSpec> {
        &self.lookups
    }

    pub fn lookup_specs_with_implicit_id(&self) -> Vec<LookupSpec> {
        let mut lookups = Vec::with_capacity(self.lookups.len() + 1);
        lookups.push(LookupSpec {
            field_name: "id".to_owned(),
            unique: true,
        });
        lookups.extend(self.lookups.values().cloned());
        lookups
    }

    pub fn signature(&self) -> String {
        let mut out = String::new();
        out.push_str(self.name());
        for field_name in &self.field_order {
            let field = self
                .field(field_name)
                .expect("field order only contains declared fields");
            out.push('|');
            out.push_str(field.name());
            out.push(':');
            out.push_str(<&'static str>::from(field.kind()));
        }
        for lookup in self.lookup_specs_with_implicit_id() {
            out.push('|');
            out.push_str("lookup:");
            out.push_str(lookup.field_name());
            out.push(':');
            out.push_str(if lookup.unique() { "unique" } else { "many" });
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LookupSpec {
    field_name: String,
    unique: bool,
}

impl LookupSpec {
    pub fn new(field_name: impl Into<String>, unique: bool) -> Result<Self, SchemaError> {
        let field_name = field_name.into();
        ensure_public_field_name(&field_name)?;
        Ok(Self { field_name, unique })
    }

    pub fn field_name(&self) -> &str {
        &self.field_name
    }

    pub fn unique(&self) -> bool {
        self.unique
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DatabaseSchema {
    tables: BTreeMap<String, TableSchema>,
}

impl Default for DatabaseSchema {
    fn default() -> Self {
        Self::new()
    }
}

impl DatabaseSchema {
    pub fn new() -> Self {
        Self {
            tables: BTreeMap::new(),
        }
    }

    pub fn add_table(&mut self, table: TableSchema) -> Result<(), SchemaError> {
        if self.tables.contains_key(table.name()) {
            return Err(SchemaError::DuplicateTable(table.name().to_owned()));
        }

        let id = table
            .field("id")
            .ok_or_else(|| SchemaError::MissingIdField {
                table: table.name.to_owned(),
            })?;
        if id.kind() != PrimitiveType::Id {
            return Err(SchemaError::IdFieldMustBeId {
                table: table.name.to_owned(),
            });
        }
        for lookup in table.lookups.values() {
            if !table.fields.contains_key(lookup.field_name()) {
                return Err(SchemaError::UnknownLookupField {
                    table: table.name.to_owned(),
                    field: lookup.field_name().to_owned(),
                });
            }
        }

        self.tables.insert(table.name.clone(), table);
        Ok(())
    }

    pub fn table(&self, name: &str) -> Option<&TableSchema> {
        self.tables.get(name)
    }

    pub fn tables(&self) -> &BTreeMap<String, TableSchema> {
        &self.tables
    }

    pub fn validate_record(&self, record: &Record) -> Result<(), SchemaError> {
        let table = self
            .table(record.table())
            .ok_or_else(|| SchemaError::UnknownTable(record.table().to_owned()))?;

        let id_field = table
            .field("id")
            .expect("table schema validation guarantees id exists");
        if !record.fields().contains_key(id_field.name()) {
            return Err(SchemaError::MissingField {
                table: table.name().to_owned(),
                field: id_field.name().to_owned(),
            });
        }

        for field_name in table.field_order() {
            if !record.fields().contains_key(field_name) {
                return Err(SchemaError::MissingField {
                    table: table.name().to_owned(),
                    field: field_name.to_owned(),
                });
            }
        }

        for (name, value) in record.fields() {
            let field = table.field(name).ok_or_else(|| SchemaError::UnknownField {
                table: table.name().to_owned(),
                field: name.to_owned(),
            })?;
            if !field.kind().matches_value(value) {
                return Err(SchemaError::TypeMismatch {
                    table: table.name().to_owned(),
                    field: name.to_owned(),
                    expected: field.kind(),
                    found: value.value_type(),
                });
            }
        }

        Ok(())
    }

    pub fn schema_hash(&self) -> String {
        let mut hash = 0xcbf29ce484222325u64;
        for table in self.tables.values() {
            for byte in table.signature().as_bytes() {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x100000001b3);
            }
            hash ^= b'\n' as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        format!("{hash:016x}")
    }
}
