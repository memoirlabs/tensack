//! Basic schema compiler crate.
//!
//! It parses a small Rust-adjacent `schema! { ... }` authoring surface into:
//! - `SchemaIr`: a canonical in-memory schema model
//! - validation with line/column errors
//! - optional low-level Rust code output for generated row types

use std::collections::HashSet;
use std::fmt;
use tensack_core::{PrimitiveType, rust_type_name};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct SchemaIr {
    pub version: u32,
    pub tables: Vec<TableIr>,
}

#[derive(Debug, Clone)]
pub struct TableIr {
    pub name: String,
    pub fields: Vec<FieldIr>,
    pub lookups: Vec<LookupIr>,
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

/// Emits a minimal generated Rust schema module as raw source.
///
/// This is intentionally tiny and compiler-oriented.
pub fn emit_raw_rust(ir: &SchemaIr) -> String {
    let mut out = String::new();
    out.push_str("pub mod tensack_generated_schema {\n");

    for table in &ir.tables {
        out.push_str(&format!("    pub mod {} {{\n", table.name));
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
        out.push_str("    }\n");
    }

    out.push_str("}\n");
    out
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
    }

    #[test]
    fn emits_row_structs() {
        let source = "schema! { users { id id \n email text \n lookup email unique \n } }";
        let ir = compile_schema(source).unwrap();
        let code = emit_raw_rust(&ir);
        assert!(code.contains("pub struct Row"));
        assert!(code.contains("pub id: String"));
        assert!(code.contains("pub email: String"));
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
