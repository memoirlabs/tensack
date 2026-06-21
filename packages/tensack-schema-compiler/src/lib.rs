//! Basic schema compiler crate.
//!
//! It parses a small Rust-adjacent `schema! { ... }` authoring surface into:
//! - `SchemaIr`: a canonical in-memory schema model
//! - validation with line/column errors
//! - optional low-level Rust code output for generated row types

use std::collections::HashSet;
use std::fmt;
use tensack_core::{
    DatabaseSchema, PrimitiveType, SchemaError as CoreSchemaError, TableSchema, rust_type_name,
};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct SchemaIr {
    pub version: u32,
    pub tables: Vec<TableIr>,
}

impl SchemaIr {
    pub fn schema_hash(&self) -> String {
        let mut hash = 0xcbf29ce484222325u64;
        for table in &self.tables {
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

#[derive(Debug, Clone)]
pub struct TableIr {
    pub name: String,
    pub fields: Vec<FieldIr>,
    pub lookups: Vec<LookupIr>,
}

impl TableIr {
    fn signature(&self) -> String {
        let mut out = String::new();
        out.push_str(&self.name);
        for field in &self.fields {
            out.push('|');
            out.push_str(&field.name);
            out.push(':');
            out.push_str(<&'static str>::from(field.ty));
        }
        out.push_str("|lookup:id:unique");
        for lookup in &self.lookups {
            out.push('|');
            out.push_str("lookup:");
            out.push_str(&lookup.field_name);
            out.push(':');
            out.push_str(if lookup.unique { "unique" } else { "many" });
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct FieldIr {
    pub name: String,
    pub ty: PrimitiveType,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct LookupIr {
    pub field_name: String,
    pub unique: bool,
}

#[derive(Debug, Clone)]
pub struct SchemaError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for SchemaError {}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Ident(String),
    Bang,
    LBrace,
    RBrace,
    Semicolon,
}

#[derive(Debug, Clone)]
struct SpannedToken {
    kind: TokenKind,
    line: usize,
    column: usize,
}

/// Parses schema declarations and validates into a canonical IR.
pub fn compile_schema(input: &str) -> Result<SchemaIr, SchemaError> {
    let tokens = lex(input)?;
    let mut ts = TokenStream::new(tokens);

    let ir = parse_schema(&mut ts)?;
    validate_schema(&ir)?;
    Ok(ir)
}

pub fn validate_schema(ir: &SchemaIr) -> Result<(), SchemaError> {
    if ir.version != SCHEMA_VERSION {
        return Err(SchemaError {
            line: 0,
            column: 0,
            message: format!(
                "schema version mismatch: expected {SCHEMA_VERSION}, got {}",
                ir.version
            ),
        });
    }

    let mut table_names = HashSet::new();

    for table in &ir.tables {
        if !is_snake_case(&table.name) {
            return Err(SchemaError {
                line: 0,
                column: 0,
                message: format!("invalid table name: {}", table.name),
            });
        }

        if !table_names.insert(&table.name) {
            return Err(SchemaError {
                line: 0,
                column: 0,
                message: format!("duplicate table name: {}", table.name),
            });
        }

        let mut field_names = HashSet::new();
        let mut has_id_field = false;

        for field in &table.fields {
            if field.name.starts_with('_') {
                return Err(SchemaError {
                    line: 0,
                    column: 0,
                    message: format!("field name cannot start with '_': {}", field.name),
                });
            }
            if !is_snake_case(&field.name) {
                return Err(SchemaError {
                    line: 0,
                    column: 0,
                    message: format!("invalid field name: {}", field.name),
                });
            }
            if !field_names.insert(&field.name) {
                return Err(SchemaError {
                    line: 0,
                    column: 0,
                    message: format!("duplicate field in table {}: {}", table.name, field.name),
                });
            }
            if field.name == "id" {
                has_id_field = true;
            }
        }

        if !has_id_field {
            return Err(SchemaError {
                line: 0,
                column: 0,
                message: format!("table '{}' is missing required field 'id'", table.name),
            });
        }

        let mut lookup_names = HashSet::new();
        for lookup in &table.lookups {
            if !field_names.contains(&lookup.field_name) {
                return Err(SchemaError {
                    line: 0,
                    column: 0,
                    message: format!(
                        "lookup '{}' refers to missing field '{}' in table '{}'",
                        lookup.field_name, lookup.field_name, table.name
                    ),
                });
            }
            if !lookup_names.insert(&lookup.field_name) {
                return Err(SchemaError {
                    line: 0,
                    column: 0,
                    message: format!(
                        "duplicate lookup on '{}' in table '{}'",
                        lookup.field_name, table.name
                    ),
                });
            }
        }
    }

    Ok(())
}

/// Converts a compiled schema IR into the runtime database schema model.
pub fn database_schema_from_ir(ir: &SchemaIr) -> Result<DatabaseSchema, CoreSchemaError> {
    let mut schema = DatabaseSchema::new();
    for table_ir in &ir.tables {
        let mut table = TableSchema::new(&table_ir.name);
        for field in &table_ir.fields {
            table.add_field(&field.name, field.ty)?;
        }
        for lookup in &table_ir.lookups {
            table.add_lookup(&lookup.field_name, lookup.unique)?;
        }
        schema.add_table(table)?;
    }
    Ok(schema)
}

/// Emits a minimal generated Rust schema module as raw source.
///
/// This is intentionally tiny and compiler-oriented, but it is useful as a
/// basic Rust SDK: each table gets a typed row, record conversions, CRUD
/// wrappers, and lookup helpers.
pub fn emit_raw_rust(ir: &SchemaIr) -> String {
    let mut out = String::new();
    out.push_str("pub mod tensack_generated_schema {\n");
    out.push_str(&format!(
        "    pub const SCHEMA_HASH: &str = \"{}\";\n\n",
        ir.schema_hash()
    ));
    out.push_str("    pub trait TensackGeneratedTables {\n");
    for table in &ir.tables {
        out.push_str(&format!(
            "        fn {}(&self) -> {}::TableHandle<'_>;\n",
            table.name, table.name
        ));
    }
    out.push_str("    }\n\n");
    out.push_str("    impl TensackGeneratedTables for tensack::TensackDatabase {\n");
    for table in &ir.tables {
        out.push_str(&format!(
            "        fn {}(&self) -> {}::TableHandle<'_> {{\n",
            table.name, table.name
        ));
        out.push_str(&format!(
            "            {}::TableHandle::new(self)\n",
            table.name
        ));
        out.push_str("        }\n");
    }
    out.push_str("    }\n\n");

    for table in &ir.tables {
        out.push_str(&format!("    pub mod {} {{\n", table.name));
        out.push_str(&format!(
            "        pub const NAME: &str = \"{}\";\n",
            table.name
        ));
        out.push_str("        #[derive(Debug, Clone, PartialEq)]\n");
        out.push_str("        pub struct Row {\n");
        for field in &table.fields {
            out.push_str(&format!(
                "            pub {}: {},\n",
                field.name,
                rust_type_name(field.ty)
            ));
        }
        out.push_str("        }\n");
        out.push_str("\n        impl Row {\n");
        out.push_str(
            "            pub fn into_record(self) -> Result<tensack::Record, tensack::SchemaError> {\n",
        );
        out.push_str("                let mut record = tensack::Record::new(NAME);\n");
        for field in &table.fields {
            if field.name == "id" {
                out.push_str("                record.insert_id(self.id);\n");
            } else {
                out.push_str(&format!(
                    "                record.insert_field(\"{}\", {})?;\n",
                    field.name,
                    rust_record_value_expr(field)
                ));
            }
        }
        out.push_str("                Ok(record)\n");
        out.push_str("            }\n\n");
        out.push_str(
            "            pub fn from_record(record: &tensack::Record) -> Result<Self, tensack::SchemaError> {\n",
        );
        out.push_str("                if record.table() != NAME {\n");
        out.push_str(
            "                    return Err(tensack::SchemaError::UnknownTable(record.table().to_owned()));\n",
        );
        out.push_str("                }\n");
        out.push_str("                Ok(Self {\n");
        for field in &table.fields {
            out.push_str(&format!(
                "                    {}: {},\n",
                field.name,
                rust_record_extract_expr(field)
            ));
        }
        out.push_str("                })\n");
        out.push_str("            }\n");
        out.push_str("        }\n");
        out.push_str("\n        pub fn table_schema() -> tensack::TableSchema {\n");
        out.push_str("            let mut table = tensack::TableSchema::new(NAME);\n");
        for field in &table.fields {
            out.push_str(&format!(
                "            table.add_field(\"{}\", tensack::PrimitiveType::{:?}).unwrap();\n",
                field.name, field.ty
            ));
        }
        for lookup in &table.lookups {
            out.push_str(&format!(
                "            table.add_lookup(\"{}\", {}).unwrap();\n",
                lookup.field_name, lookup.unique
            ));
        }
        out.push_str("            table\n");
        out.push_str("        }\n");

        out.push_str("\n        #[derive(Debug, Clone, PartialEq)]\n");
        out.push_str("        pub struct Patch {\n");
        out.push_str("            fields: ::std::collections::BTreeMap<String, tensack::Value>,\n");
        out.push_str("        }\n\n");
        out.push_str("        impl Patch {\n");
        out.push_str("            pub fn new() -> Self {\n");
        out.push_str("                Self { fields: ::std::collections::BTreeMap::new() }\n");
        out.push_str("            }\n");
        for field in &table.fields {
            if field.name == "id" {
                continue;
            }
            out.push_str(&format!(
                "\n            pub fn {}(mut self, value: {}) -> Self {{\n",
                field.name,
                rust_param_type(field)
            ));
            out.push_str(&format!(
                "                self.fields.insert(\"{}\".to_owned(), {});\n",
                field.name,
                rust_value_expr(field, "value")
            ));
            out.push_str("                self\n");
            out.push_str("            }\n");
        }
        out.push_str("        }\n\n");
        out.push_str("        impl Default for Patch {\n");
        out.push_str("            fn default() -> Self {\n");
        out.push_str("                Self::new()\n");
        out.push_str("            }\n");
        out.push_str("        }\n");

        out.push_str("\n        pub mod key {\n");
        out.push_str("            #[derive(Debug, Clone, PartialEq)]\n");
        out.push_str("            pub struct Key {\n");
        out.push_str("                pub lookup: &'static str,\n");
        out.push_str("                pub value: tensack::Value,\n");
        out.push_str("            }\n\n");
        let id_field = table
            .fields
            .iter()
            .find(|field| field.name == "id")
            .expect("validated schema has id");
        emit_key_constructor(&mut out, id_field, true);
        for lookup in &table.lookups {
            if lookup.unique
                && let Some(field) = table
                    .fields
                    .iter()
                    .find(|field| field.name == lookup.field_name)
            {
                emit_key_constructor(&mut out, field, false);
            }
        }
        out.push_str("        }\n");

        out.push_str("\n        pub mod by {\n");
        emit_by_constructor(&mut out, id_field, true, true);
        for lookup in &table.lookups {
            if let Some(field) = table
                .fields
                .iter()
                .find(|field| field.name == lookup.field_name)
            {
                emit_by_constructor(&mut out, field, false, lookup.unique);
            }
        }
        out.push_str("        }\n");

        out.push_str("\n        #[derive(Debug, Clone, PartialEq)]\n");
        out.push_str("        pub struct OneSelector {\n");
        out.push_str("            inner: tensack::GetOne,\n");
        out.push_str("        }\n\n");
        out.push_str("        impl tensack::GetRequest for OneSelector {\n");
        out.push_str("            type Output = Option<Row>;\n\n");
        out.push_str("            fn into_plan(self) -> Result<tensack::PlanEnvelope, tensack::TensackError> {\n");
        out.push_str("                Ok(self.inner.into_plan())\n");
        out.push_str("            }\n\n");
        out.push_str("            fn from_outcome(outcome: tensack::PlanOutcome) -> Result<Self::Output, tensack::TensackError> {\n");
        out.push_str("                match tensack::GetOne::from_outcome(outcome)? {\n");
        out.push_str("                    Some(record) => Ok(Some(Row::from_record(&record)?)),\n");
        out.push_str("                    None => Ok(None),\n");
        out.push_str("                }\n");
        out.push_str("            }\n");
        out.push_str("        }\n");

        out.push_str("\n        #[derive(Debug, Clone, PartialEq)]\n");
        out.push_str("        pub struct ManySelector {\n");
        out.push_str("            inner: tensack::GetMany,\n");
        out.push_str("        }\n\n");
        out.push_str("        impl ManySelector {\n");
        out.push_str("            pub fn limit(mut self, limit: usize) -> Self {\n");
        out.push_str("                self.inner = self.inner.limit(limit);\n");
        out.push_str("                self\n");
        out.push_str("            }\n\n");
        out.push_str("            pub fn cursor(mut self, cursor: impl Into<String>) -> Self {\n");
        out.push_str("                self.inner = self.inner.cursor(cursor);\n");
        out.push_str("                self\n");
        out.push_str("            }\n");
        out.push_str("        }\n\n");
        out.push_str("        impl tensack::GetRequest for ManySelector {\n");
        out.push_str("            type Output = Vec<Row>;\n\n");
        out.push_str("            fn into_plan(self) -> Result<tensack::PlanEnvelope, tensack::TensackError> {\n");
        out.push_str("                Ok(self.inner.into_plan())\n");
        out.push_str("            }\n\n");
        out.push_str("            fn from_outcome(outcome: tensack::PlanOutcome) -> Result<Self::Output, tensack::TensackError> {\n");
        out.push_str("                rows_from_records(tensack::GetMany::from_outcome(outcome)?).map_err(tensack::TensackError::from)\n");
        out.push_str("            }\n");
        out.push_str("        }\n");

        out.push_str("\n        #[derive(Debug, Clone, PartialEq)]\n");
        out.push_str("        pub struct PageSelector {\n");
        out.push_str("            inner: tensack::GetPage,\n");
        out.push_str("        }\n\n");
        out.push_str("        impl PageSelector {\n");
        out.push_str("            pub fn limit(mut self, limit: usize) -> Self {\n");
        out.push_str("                self.inner = self.inner.limit(limit);\n");
        out.push_str("                self\n");
        out.push_str("            }\n\n");
        out.push_str("            pub fn cursor(mut self, cursor: impl Into<String>) -> Self {\n");
        out.push_str("                self.inner = self.inner.cursor(cursor);\n");
        out.push_str("                self\n");
        out.push_str("            }\n");
        out.push_str("        }\n\n");
        out.push_str("        impl tensack::GetRequest for PageSelector {\n");
        out.push_str("            type Output = (Vec<Row>, Option<String>);\n\n");
        out.push_str("            fn into_plan(self) -> Result<tensack::PlanEnvelope, tensack::TensackError> {\n");
        out.push_str("                Ok(self.inner.into_plan())\n");
        out.push_str("            }\n\n");
        out.push_str("            fn from_outcome(outcome: tensack::PlanOutcome) -> Result<Self::Output, tensack::TensackError> {\n");
        out.push_str("                let page = tensack::GetPage::from_outcome(outcome)?;\n");
        out.push_str("                Ok((rows_from_records(page.rows)?, page.next_cursor))\n");
        out.push_str("            }\n");
        out.push_str("        }\n");

        out.push_str("\n        #[derive(Debug, Clone, PartialEq)]\n");
        out.push_str("        pub struct CountSelector {\n");
        out.push_str("            inner: tensack::GetCount,\n");
        out.push_str("        }\n\n");
        out.push_str("        impl tensack::GetRequest for CountSelector {\n");
        out.push_str("            type Output = usize;\n\n");
        out.push_str("            fn into_plan(self) -> Result<tensack::PlanEnvelope, tensack::TensackError> {\n");
        out.push_str("                Ok(self.inner.into_plan())\n");
        out.push_str("            }\n\n");
        out.push_str("            fn from_outcome(outcome: tensack::PlanOutcome) -> Result<Self::Output, tensack::TensackError> {\n");
        out.push_str("                tensack::GetCount::from_outcome(outcome)\n");
        out.push_str("            }\n");
        out.push_str("        }\n");

        out.push_str("\n        pub fn all() -> PageSelector {\n");
        out.push_str("            PageSelector { inner: tensack::GetPage::new(NAME) }\n");
        out.push_str("        }\n\n");
        out.push_str("        pub fn count() -> CountSelector {\n");
        out.push_str("            CountSelector { inner: tensack::GetCount::table(NAME) }\n");
        out.push_str("        }\n\n");
        out.push_str("        pub fn add(row: Row) -> tensack::WriteChange {\n");
        out.push_str("            tensack::WriteChange::add_record(record_from_row(row))\n");
        out.push_str("        }\n\n");
        out.push_str("        pub fn set(row: Row) -> tensack::WriteChange {\n");
        out.push_str("            tensack::WriteChange::set_record(record_from_row(row))\n");
        out.push_str("        }\n\n");
        out.push_str(
            "        pub fn edit(target: key::Key, patch: Patch) -> tensack::WriteChange {\n",
        );
        out.push_str("            tensack::WriteChange::edit(NAME, target.lookup, target.value, patch.fields)\n");
        out.push_str("        }\n\n");
        out.push_str("        pub fn remove(target: key::Key) -> tensack::WriteChange {\n");
        out.push_str(
            "            tensack::WriteChange::remove(NAME, target.lookup, target.value)\n",
        );
        out.push_str("        }\n");

        out.push_str("\n        pub struct TableHandle<'a> {\n");
        out.push_str("            db: &'a tensack::TensackDatabase,\n");
        out.push_str("        }\n\n");
        out.push_str("        impl<'a> TableHandle<'a> {\n");
        out.push_str("            pub fn new(db: &'a tensack::TensackDatabase) -> Self {\n");
        out.push_str("                Self { db }\n");
        out.push_str("            }\n\n");
        out.push_str("            pub fn insert(&self, row: Row) -> Result<tensack::AppendResult, tensack::TensackError> {\n");
        out.push_str("                let record = row.into_record()?;\n");
        out.push_str("                match self.db.execute_plan(tensack::PlanEnvelope::new(tensack::PlanOp::Insert, NAME).with_record_value(record))? {\n");
        out.push_str("                    tensack::PlanOutcome::Append(result) => Ok(result),\n");
        out.push_str(
            "                    _ => unreachable!(\"insert plans return append results\"),\n",
        );
        out.push_str("                }\n");
        out.push_str("            }\n\n");
        out.push_str("            pub fn upsert(&self, row: Row) -> Result<tensack::AppendResult, tensack::TensackError> {\n");
        out.push_str("                let record = row.into_record()?;\n");
        out.push_str("                match self.db.execute_plan(tensack::PlanEnvelope::new(tensack::PlanOp::Upsert, NAME).with_record_value(record))? {\n");
        out.push_str("                    tensack::PlanOutcome::Append(result) => Ok(result),\n");
        out.push_str(
            "                    _ => unreachable!(\"upsert plans return append results\"),\n",
        );
        out.push_str("                }\n");
        out.push_str("            }\n\n");
        out.push_str("            pub fn put(&self, row: Row) -> Result<tensack::AppendResult, tensack::TensackError> {\n");
        out.push_str("                self.upsert(row)\n");
        out.push_str("            }\n\n");
        out.push_str("            pub fn patch(&self, target: key::Key, patch: Patch) -> Result<tensack::AppendResult, tensack::TensackError> {\n");
        out.push_str("                let mut plan = tensack::PlanEnvelope::new(tensack::PlanOp::Patch, NAME).with_lookup(target.lookup);\n");
        out.push_str("                plan.key.insert(target.lookup.to_owned(), target.value);\n");
        out.push_str("                plan.value = patch.fields;\n");
        out.push_str("                match self.db.execute_plan(plan)? {\n");
        out.push_str("                    tensack::PlanOutcome::Append(result) => Ok(result),\n");
        out.push_str(
            "                    _ => unreachable!(\"patch plans return append results\"),\n",
        );
        out.push_str("                }\n");
        out.push_str("            }\n\n");
        out.push_str("            pub fn remove(&self, target: key::Key) -> Result<tensack::AppendResult, tensack::TensackError> {\n");
        out.push_str("                let mut plan = tensack::PlanEnvelope::new(tensack::PlanOp::Remove, NAME).with_lookup(target.lookup);\n");
        out.push_str("                plan.key.insert(target.lookup.to_owned(), target.value);\n");
        out.push_str("                match self.db.execute_plan(plan)? {\n");
        out.push_str("                    tensack::PlanOutcome::Append(result) => Ok(result),\n");
        out.push_str(
            "                    _ => unreachable!(\"remove plans return append results\"),\n",
        );
        out.push_str("                }\n");
        out.push_str("            }\n\n");
        out.push_str("            pub fn get(&self) -> GetHandle<'a> {\n");
        out.push_str("                GetHandle { db: self.db }\n");
        out.push_str("            }\n\n");
        out.push_str("            pub fn find(&self) -> FindHandle<'a> {\n");
        out.push_str("                FindHandle { db: self.db }\n");
        out.push_str("            }\n\n");
        out.push_str("            pub fn scan(&self) -> ScanBuilder<'a> {\n");
        out.push_str("                ScanBuilder { db: self.db, limit: None, cursor: None }\n");
        out.push_str("            }\n\n");
        out.push_str("            pub fn count(&self) -> Result<usize, tensack::TensackError> {\n");
        out.push_str("                match self.db.execute_plan(tensack::PlanEnvelope::new(tensack::PlanOp::Count, NAME))? {\n");
        out.push_str("                    tensack::PlanOutcome::Count(count) => Ok(count),\n");
        out.push_str("                    _ => unreachable!(\"count plans return counts\"),\n");
        out.push_str("                }\n");
        out.push_str("            }\n");
        out.push_str("        }\n");

        out.push_str("\n        pub struct GetHandle<'a> {\n");
        out.push_str("            db: &'a tensack::TensackDatabase,\n");
        out.push_str("        }\n\n");
        out.push_str("        impl<'a> GetHandle<'a> {\n");
        emit_get_method(&mut out, id_field, true);
        for lookup in &table.lookups {
            if lookup.unique
                && let Some(field) = table
                    .fields
                    .iter()
                    .find(|field| field.name == lookup.field_name)
            {
                emit_get_method(&mut out, field, false);
            }
        }
        out.push_str("        }\n");

        out.push_str("\n        pub struct FindHandle<'a> {\n");
        out.push_str("            db: &'a tensack::TensackDatabase,\n");
        out.push_str("        }\n\n");
        out.push_str("        impl<'a> FindHandle<'a> {\n");
        for lookup in &table.lookups {
            if let Some(field) = table
                .fields
                .iter()
                .find(|field| field.name == lookup.field_name)
            {
                emit_find_method(&mut out, field);
            }
        }
        out.push_str("        }\n");

        out.push_str("\n        pub struct ScanBuilder<'a> {\n");
        out.push_str("            db: &'a tensack::TensackDatabase,\n");
        out.push_str("            limit: Option<usize>,\n");
        out.push_str("            cursor: Option<String>,\n");
        out.push_str("        }\n\n");
        out.push_str("        impl<'a> ScanBuilder<'a> {\n");
        out.push_str("            pub fn limit(mut self, limit: usize) -> Self {\n");
        out.push_str("                self.limit = Some(limit);\n");
        out.push_str("                self\n");
        out.push_str("            }\n\n");
        out.push_str("            pub fn cursor(mut self, cursor: impl Into<String>) -> Self {\n");
        out.push_str("                self.cursor = Some(cursor.into());\n");
        out.push_str("                self\n");
        out.push_str("            }\n\n");
        out.push_str("            pub fn run(self) -> Result<(Vec<Row>, Option<String>), tensack::TensackError> {\n");
        out.push_str("                let mut plan = tensack::PlanEnvelope::new(tensack::PlanOp::Scan, NAME);\n");
        out.push_str("                plan.limit = self.limit;\n");
        out.push_str("                plan.cursor = self.cursor;\n");
        out.push_str("                match self.db.execute_plan(plan)? {\n");
        out.push_str("                    tensack::PlanOutcome::Rows(page) => Ok((rows_from_records(page.rows)?, page.next_cursor)),\n");
        out.push_str("                    _ => unreachable!(\"scan plans return row pages\"),\n");
        out.push_str("                }\n");
        out.push_str("            }\n");
        out.push_str("        }\n");

        out.push_str("\n        fn record_from_row(row: Row) -> tensack::Record {\n");
        out.push_str(
            "            row.into_record().expect(\"generated rows only contain schema fields\")\n",
        );
        out.push_str("        }\n");

        out.push_str("\n        fn rows_from_records(records: Vec<tensack::Record>) -> Result<Vec<Row>, tensack::SchemaError> {\n");
        out.push_str("            let mut rows = Vec::with_capacity(records.len());\n");
        out.push_str("            for record in records {\n");
        out.push_str("                rows.push(Row::from_record(&record)?);\n");
        out.push_str("            }\n");
        out.push_str("            Ok(rows)\n");
        out.push_str("        }\n");

        out.push_str("    }\n");
    }

    out.push_str("\n    pub fn database_schema() -> tensack::DatabaseSchema {\n");
    out.push_str("        let mut schema = tensack::DatabaseSchema::new();\n");
    for table in &ir.tables {
        out.push_str(&format!(
            "        schema.add_table({}::table_schema()).unwrap();\n",
            table.name
        ));
    }
    out.push_str("        schema\n");
    out.push_str("    }\n");
    out.push_str("}\n");
    out
}

fn emit_key_constructor(out: &mut String, field: &FieldIr, is_id: bool) {
    let name = if is_id { "id" } else { &field.name };
    out.push_str(&format!(
        "            pub fn {}(value: {}) -> Key {{\n",
        name,
        rust_param_type(field)
    ));
    out.push_str("                Key {\n");
    out.push_str(&format!("                    lookup: \"{}\",\n", name));
    out.push_str(&format!(
        "                    value: {},\n",
        rust_value_expr(field, "value")
    ));
    out.push_str("                }\n");
    out.push_str("            }\n");
}

fn emit_by_constructor(out: &mut String, field: &FieldIr, is_id: bool, unique: bool) {
    let name = if is_id { "id" } else { &field.name };
    let selector = if unique {
        "OneSelector"
    } else {
        "ManySelector"
    };
    let inner = if unique { "GetOne" } else { "GetMany" };
    out.push_str(&format!(
        "            pub fn {}(value: {}) -> super::{} {{\n",
        name,
        rust_param_type(field),
        selector
    ));
    out.push_str(&format!(
        "                super::{} {{ inner: tensack::{}::new(super::NAME, \"{}\", {}) }}\n",
        selector,
        inner,
        name,
        rust_value_expr(field, "value")
    ));
    out.push_str("            }\n");
}

fn emit_get_method(out: &mut String, field: &FieldIr, is_id: bool) {
    let name = if is_id { "id" } else { &field.name };
    out.push_str(&format!(
        "            pub fn {}(&self, value: {}) -> Result<Option<Row>, tensack::TensackError> {{\n",
        name,
        rust_param_type(field)
    ));
    out.push_str(&format!(
        "                let plan = tensack::PlanEnvelope::new(tensack::PlanOp::Get, NAME).with_lookup(\"{}\").with_key(\"{}\", {});\n",
        name,
        name,
        rust_value_expr(field, "value")
    ));
    out.push_str("                match self.db.execute_plan(plan)? {\n");
    out.push_str("                    tensack::PlanOutcome::Row(Some(record)) => Ok(Some(Row::from_record(&record)?)),\n");
    out.push_str("                    tensack::PlanOutcome::Row(None) => Ok(None),\n");
    out.push_str("                    _ => unreachable!(\"get plans return row results\"),\n");
    out.push_str("                }\n");
    out.push_str("            }\n");
}

fn emit_find_method(out: &mut String, field: &FieldIr) {
    out.push_str(&format!(
        "            pub fn {}(&self, value: {}) -> Result<Vec<Row>, tensack::TensackError> {{\n",
        field.name,
        rust_param_type(field)
    ));
    out.push_str(&format!(
        "                let plan = tensack::PlanEnvelope::new(tensack::PlanOp::Find, NAME).with_lookup(\"{}\").with_key(\"{}\", {}).with_limit(1000);\n",
        field.name,
        field.name,
        rust_value_expr(field, "value")
    ));
    out.push_str("                match self.db.execute_plan(plan)? {\n");
    out.push_str("                    tensack::PlanOutcome::Rows(page) => rows_from_records(page.rows).map_err(tensack::TensackError::from),\n");
    out.push_str("                    _ => unreachable!(\"find plans return row pages\"),\n");
    out.push_str("                }\n");
    out.push_str("            }\n");
}

fn rust_param_type(field: &FieldIr) -> String {
    match field.ty {
        PrimitiveType::Id | PrimitiveType::Text => "impl Into<String>".to_owned(),
        PrimitiveType::Int => "i64".to_owned(),
        PrimitiveType::Float => "f64".to_owned(),
        PrimitiveType::Bool => "bool".to_owned(),
    }
}

fn rust_value_expr(field: &FieldIr, value_expr: &str) -> String {
    match field.ty {
        PrimitiveType::Id => format!("tensack::Value::Id({value_expr}.into())"),
        PrimitiveType::Text => format!("tensack::Value::Text({value_expr}.into())"),
        PrimitiveType::Int => format!("tensack::Value::Int({value_expr})"),
        PrimitiveType::Float => format!("tensack::Value::Float({value_expr})"),
        PrimitiveType::Bool => format!("tensack::Value::Bool({value_expr})"),
    }
}

fn rust_record_value_expr(field: &FieldIr) -> String {
    match field.ty {
        PrimitiveType::Id => format!("tensack::Value::Id(self.{})", field.name),
        PrimitiveType::Text | PrimitiveType::Int | PrimitiveType::Float | PrimitiveType::Bool => {
            format!("self.{}", field.name)
        }
    }
}

fn rust_record_extract_expr(field: &FieldIr) -> String {
    let variant = match field.ty {
        PrimitiveType::Id => "Id",
        PrimitiveType::Text => "Text",
        PrimitiveType::Int => "Int",
        PrimitiveType::Float => "Float",
        PrimitiveType::Bool => "Bool",
    };
    let value_expr = match field.ty {
        PrimitiveType::Id | PrimitiveType::Text => "value.clone()",
        PrimitiveType::Int | PrimitiveType::Float | PrimitiveType::Bool => "*value",
    };
    format!(
        "match record.fields().get(\"{field}\") {{
                        Some(tensack::Value::{variant}(value)) => {value_expr},
                        Some(value) => return Err(tensack::SchemaError::TypeMismatch {{
                            table: NAME.to_owned(),
                            field: \"{field}\".to_owned(),
                            expected: tensack::PrimitiveType::{expected:?},
                            found: value.value_type(),
                        }}),
                        None => return Err(tensack::SchemaError::MissingField {{
                            table: NAME.to_owned(),
                            field: \"{field}\".to_owned(),
                        }}),
                    }}",
        field = field.name,
        variant = variant,
        value_expr = value_expr,
        expected = field.ty
    )
}

fn parse_schema(ts: &mut TokenStream) -> Result<SchemaIr, SchemaError> {
    // Support plain table blocks or schema! wrapped blocks.
    if let Some(TokenKind::Ident(name)) = ts.peek_kind()
        && name == "schema"
    {
        ts.next()?;
        ts.consume(TokenKind::Bang)?;
        ts.consume(TokenKind::LBrace)?;
        let tables = parse_table_list(ts)?;
        ts.consume(TokenKind::RBrace)?;
        return Ok(SchemaIr {
            version: SCHEMA_VERSION,
            tables,
        });
    }

    Ok(SchemaIr {
        version: SCHEMA_VERSION,
        tables: parse_table_list(ts)?,
    })
}

fn parse_table_list(ts: &mut TokenStream) -> Result<Vec<TableIr>, SchemaError> {
    let mut tables = Vec::new();
    loop {
        match ts.peek_kind() {
            Some(TokenKind::Ident(name)) if name != "}" => {
                let table_name = ts.next_ident()?;
                let name = table_name;
                ts.consume(TokenKind::LBrace)?;
                let mut table = TableIr {
                    name,
                    fields: Vec::new(),
                    lookups: Vec::new(),
                };
                parse_table_items(ts, &mut table)?;
                ts.consume(TokenKind::RBrace)?;
                tables.push(table);
            }
            Some(TokenKind::RBrace) => break,
            Some(_) => {
                return Err(ts.unexpected("expected table name or block end"));
            }
            None => {
                return Err(SchemaError {
                    line: 0,
                    column: 0,
                    message: "unexpected end of input while parsing schema".to_string(),
                });
            }
        }
    }
    Ok(tables)
}

fn parse_table_items(ts: &mut TokenStream, table: &mut TableIr) -> Result<(), SchemaError> {
    loop {
        match ts.peek_kind() {
            Some(TokenKind::RBrace) | None => break,
            Some(TokenKind::Ident(id)) if id == "lookup" => {
                ts.next()?;
                let field_name = ts.next_ident()?;
                let unique = match ts.peek_kind() {
                    Some(TokenKind::Ident(name)) if name == "unique" => {
                        ts.next()?;
                        true
                    }
                    _ => false,
                };
                let _ = ts.consume_optional(TokenKind::Semicolon);
                table.lookups.push(LookupIr { field_name, unique });
            }
            Some(TokenKind::Ident(_field_name)) => {
                let name = ts.next_ident()?;
                let type_name = ts.next_ident()?;
                let ty = primitive_type_from_schema_name(&type_name).ok_or_else(|| {
                    ts.error(format!(
                        "unknown type '{type_name}' (expected id, text, int, float, bool)"
                    ))
                })?;
                let _ = ts.consume_optional(TokenKind::Semicolon);
                table.fields.push(FieldIr {
                    name,
                    ty,
                    required: false,
                });
            }
            Some(_) => {
                return Err(ts.unexpected("expected lookup declaration or field declaration"));
            }
        }
    }
    Ok(())
}

fn lex(input: &str) -> Result<Vec<SpannedToken>, SchemaError> {
    let mut out = Vec::new();
    let mut line = 1usize;
    let mut col = 1usize;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\n' => {
                line += 1;
                col = 1;
            }
            ' ' | '\t' | '\r' => {
                col += 1;
            }
            '{' => {
                out.push(SpannedToken {
                    kind: TokenKind::LBrace,
                    line,
                    column: col,
                });
                col += 1;
            }
            '}' => {
                out.push(SpannedToken {
                    kind: TokenKind::RBrace,
                    line,
                    column: col,
                });
                col += 1;
            }
            '!' => {
                out.push(SpannedToken {
                    kind: TokenKind::Bang,
                    line,
                    column: col,
                });
                col += 1;
            }
            ';' => {
                out.push(SpannedToken {
                    kind: TokenKind::Semicolon,
                    line,
                    column: col,
                });
                col += 1;
            }
            '/' => {
                let is_comment = matches!(chars.peek(), Some('/'));
                if is_comment {
                    for c in chars.by_ref() {
                        col += 1;
                        if c == '\n' {
                            line += 1;
                            col = 1;
                            break;
                        }
                    }
                } else {
                    return Err(SchemaError {
                        line,
                        column: col,
                        message: "unexpected '/'".to_string(),
                    });
                }
            }
            _ if is_ident_start(ch) => {
                let start_col = col;
                let mut ident = String::new();
                ident.push(ch);
                col += 1;
                while let Some(next) = chars.peek() {
                    if is_ident_continue(*next) {
                        ident.push(*next);
                        chars.next();
                        col += 1;
                    } else {
                        break;
                    }
                }
                out.push(SpannedToken {
                    kind: TokenKind::Ident(ident),
                    line,
                    column: start_col,
                });
            }
            _ => {
                return Err(SchemaError {
                    line,
                    column: col,
                    message: format!("unexpected character '{ch}'"),
                });
            }
        }
        // keep column aligned for multi-byte tokenized lines
        if ch == '\n' {
            continue;
        }
        // col already moved for ASCII token start in branches above.
    }

    Ok(out)
}

struct TokenStream {
    tokens: Vec<SpannedToken>,
    pos: usize,
}

impl TokenStream {
    fn new(tokens: Vec<SpannedToken>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek_kind(&self) -> Option<&TokenKind> {
        self.tokens.get(self.pos).map(|t| &t.kind)
    }

    fn next(&mut self) -> Result<SpannedToken, SchemaError> {
        if self.pos >= self.tokens.len() {
            return Err(SchemaError {
                line: 0,
                column: 0,
                message: "unexpected end of input".to_string(),
            });
        }
        let out = self.tokens[self.pos].clone();
        self.pos += 1;
        Ok(out)
    }

    fn next_ident(&mut self) -> Result<String, SchemaError> {
        match self.next()?.kind {
            TokenKind::Ident(value) => Ok(value),
            other => Err(self.error(format!("expected identifier, got {}", token_pretty(&other)))),
        }
    }

    fn consume(&mut self, expected: TokenKind) -> Result<(), SchemaError> {
        match self.peek_kind() {
            Some(actual) if actual == &expected => {
                let _ = self.next()?;
                Ok(())
            }
            _ => Err(self.unexpected(&format!("expected {}", token_pretty(&expected)))),
        }
    }

    fn consume_optional(&mut self, expected: TokenKind) -> bool {
        if matches!(self.peek_kind(), Some(actual) if actual == &expected) {
            let _ = self.next();
            true
        } else {
            false
        }
    }

    fn unexpected(&self, message: &str) -> SchemaError {
        if let Some(tok) = self.tokens.get(self.pos) {
            SchemaError {
                line: tok.line,
                column: tok.column,
                message: format!("{} at {}", message, token_pretty(&tok.kind)),
            }
        } else {
            SchemaError {
                line: 0,
                column: 0,
                message: format!("{message} at end of input"),
            }
        }
    }

    fn error(&self, message: String) -> SchemaError {
        if let Some(tok) = self.tokens.get(self.pos.saturating_sub(1)) {
            SchemaError {
                line: tok.line,
                column: tok.column,
                message,
            }
        } else {
            SchemaError {
                line: 0,
                column: 0,
                message,
            }
        }
    }
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn primitive_type_from_schema_name(name: &str) -> Option<PrimitiveType> {
    match name {
        "id" => Some(PrimitiveType::Id),
        "text" => Some(PrimitiveType::Text),
        "int" => Some(PrimitiveType::Int),
        "float" => Some(PrimitiveType::Float),
        "bool" => Some(PrimitiveType::Bool),
        _ => None,
    }
}

fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn is_snake_case(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    let first = chars.next().expect("non-empty");
    if !first.is_ascii_lowercase() && first != '_' {
        return false;
    }
    !name.contains("__")
        && name
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn token_pretty(kind: &TokenKind) -> &'static str {
    match kind {
        TokenKind::Ident(_) => "identifier",
        TokenKind::Bang => "'!'",
        TokenKind::LBrace => "'{'",
        TokenKind::RBrace => "'}'",
        TokenKind::Semicolon => "';'",
    }
}

impl SchemaIr {
    pub fn with_version(version: u32) -> Self {
        Self {
            version,
            tables: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_schema_file() {
        let source = include_str!("../../tensack/examples/chat_schema.tensack");
        let ir = compile_schema(source).unwrap();
        assert!(!ir.tables.is_empty());
    }

    #[test]
    fn compiles_random_multi_table_schema_with_all_simple_types() {
        let source = r#"
            schema! {
                accounts {
                    id id
                    email text
                    login_count int
                    trust_score float
                    disabled bool

                    lookup email unique
                    lookup disabled
                }

                sessions {
                    id id
                    account_id id
                    started_at int
                    ip_address text

                    lookup account_id
                    lookup started_at
                }

                messages {
                    id id
                    account_id id
                    body text
                    token_count int
                    cost float
                    flagged bool

                    lookup account_id
                    lookup flagged
                }
            }
        "#;

        let ir = compile_schema(source).unwrap();
        assert_eq!(ir.tables.len(), 3);

        let accounts = ir
            .tables
            .iter()
            .find(|table| table.name == "accounts")
            .unwrap();
        assert_eq!(accounts.fields.len(), 5);
        assert_eq!(accounts.lookups.len(), 2);
        assert!(accounts.lookups.iter().any(|lookup| lookup.unique));

        let code = emit_raw_rust(&ir);
        assert!(code.contains("pub mod accounts"));
        assert!(code.contains("pub login_count: i64"));
        assert!(code.contains("pub trust_score: f64"));
        assert!(code.contains("pub disabled: bool"));
        assert!(code.contains("pub account_id: String"));
        assert!(code.contains("pub trait TensackGeneratedTables"));
        assert!(code.contains("pub struct TableHandle"));
        assert!(code.contains("pub mod by"));
        assert!(code.contains("pub fn add(row: Row)"));
        assert!(code.contains("pub fn set(row: Row)"));
        assert!(code.contains("pub fn edit(target: key::Key, patch: Patch)"));
        assert!(code.contains("pub fn all() -> PageSelector"));
        assert!(code.contains("pub fn account_id(&self, value: impl Into<String>)"));
        assert!(code.contains("pub fn email(&self, value: impl Into<String>)"));
        assert!(!code.contains("pub fn insert(db: &tensack::TensackDatabase, row: Row)"));
        assert!(!code.contains("pub fn get_many_by_account_id"));
        assert!(!code.contains("pub fn get_by_email"));
    }

    #[test]
    fn emits_row_structs() {
        let source = "schema! { users { id id \n email text \n lookup email unique \n } }";
        let ir = compile_schema(source).unwrap();
        let code = emit_raw_rust(&ir);
        assert!(code.contains("pub struct Row"));
        assert!(code.contains("pub id: String"));
        assert!(code.contains("pub email: String"));
        assert!(code.contains("pub fn into_record(self)"));
        assert!(code.contains("pub fn from_record(record: &tensack::Record)"));
        assert!(code.contains("pub struct Patch"));
        assert!(code.contains("pub mod key"));
    }

    #[test]
    fn rejects_unknown_type() {
        let source = "schema! { users { id uuid } }";
        let err = compile_schema(source).unwrap_err();
        assert!(err.message.contains("unknown type"));
    }

    #[test]
    fn rejects_duplicate_field() {
        let source = "schema! { users { id id \n id int } }";
        let err = compile_schema(source).unwrap_err();
        assert!(err.message.contains("duplicate field"));
    }
}
