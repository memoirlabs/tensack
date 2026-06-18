use std::collections::BTreeMap;

use crate::error::{SchemaError, ensure_public_field_name};
use crate::value::SackValue;

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    table: String,
    fields: BTreeMap<String, SackValue>,
}

impl Record {
    pub fn new(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            fields: BTreeMap::new(),
        }
    }

    pub fn table(&self) -> &str {
        &self.table
    }

    pub fn fields(&self) -> &BTreeMap<String, SackValue> {
        &self.fields
    }

    pub fn fields_mut(&mut self) -> &mut BTreeMap<String, SackValue> {
        &mut self.fields
    }

    pub fn with_field(
        mut self,
        name: impl Into<String>,
        value: impl Into<SackValue>,
    ) -> Result<Self, SchemaError> {
        let name = name.into();
        ensure_public_field_name(&name)?;
        self.fields.insert(name, value.into());
        Ok(self)
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Result<Self, SchemaError> {
        self.fields
            .insert("id".to_owned(), SackValue::Id(id.into()));
        Ok(self)
    }

    pub fn insert_field(
        &mut self,
        name: impl Into<String>,
        value: impl Into<SackValue>,
    ) -> Result<(), SchemaError> {
        let name = name.into();
        ensure_public_field_name(&name)?;
        self.fields.insert(name, value.into());
        Ok(())
    }

    pub fn insert_id(&mut self, id: impl Into<String>) {
        self.fields
            .insert("id".to_owned(), SackValue::Id(id.into()));
    }
}
