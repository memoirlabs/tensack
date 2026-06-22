//! Basic type declarations for the schema surface and their Rust equivalents.
//!
//! This file is intentionally small: just a mapping of the public primitive names
//! (`id`, `text`, `int`, `float`, `bool`) to Rust-native types.

use crate::value::PrimitiveType;

/// Canonical Rust type names for the public primitives.
#[derive(Debug, Clone, Copy)]
pub struct PrimitiveTypeDecl {
    pub schema_name: &'static str,
    pub rust_name: &'static str,
    pub rust_expr: &'static str,
    pub rust_docs: &'static str,
}

impl PrimitiveTypeDecl {
    pub const fn for_type(primitive: PrimitiveType) -> Self {
        match primitive {
            PrimitiveType::Id => Self {
                schema_name: "id",
                rust_name: "String",
                rust_expr: "String",
                rust_docs: "Stable string identifier",
            },
            PrimitiveType::Text => Self {
                schema_name: "text",
                rust_name: "String",
                rust_expr: "String",
                rust_docs: "UTF-8 text",
            },
            PrimitiveType::Int => Self {
                schema_name: "int",
                rust_name: "i64",
                rust_expr: "i64",
                rust_docs: "Signed integer",
            },
            PrimitiveType::Float => Self {
                schema_name: "float",
                rust_name: "f64",
                rust_expr: "f64",
                rust_docs: "Floating point number",
            },
            PrimitiveType::Bool => Self {
                schema_name: "bool",
                rust_name: "bool",
                rust_expr: "bool",
                rust_docs: "Boolean",
            },
        }
    }
}

pub const PRIMITIVE_TYPE_DECLARATIONS: [PrimitiveTypeDecl; 5] = [
    PrimitiveTypeDecl::for_type(PrimitiveType::Id),
    PrimitiveTypeDecl::for_type(PrimitiveType::Text),
    PrimitiveTypeDecl::for_type(PrimitiveType::Int),
    PrimitiveTypeDecl::for_type(PrimitiveType::Float),
    PrimitiveTypeDecl::for_type(PrimitiveType::Bool),
];

pub fn rust_type_name(primitive: PrimitiveType) -> &'static str {
    PrimitiveTypeDecl::for_type(primitive).rust_expr
}

pub fn find_decl(schema_name: &str) -> Option<PrimitiveTypeDecl> {
    PRIMITIVE_TYPE_DECLARATIONS
        .iter()
        .copied()
        .find(|decl| decl.schema_name == schema_name)
}
