mod error;
mod record;
mod schema;
mod type_declarations;
mod value;
mod workspace;

pub use error::SchemaError;
pub use record::Record;
pub use schema::{DatabaseSchema, FieldSpec, LookupSpec, TableSchema};
pub use type_declarations::{
    PRIMITIVE_TYPE_DECLARATIONS, PrimitiveTypeDecl, find_decl, rust_type_name,
};
pub use value::{PrimitiveType, SackValue};
pub use workspace::Workspace;

#[cfg(test)]
mod tests {
    use crate::error::SchemaError;
    use crate::record::Record;
    use crate::schema::{DatabaseSchema, TableSchema};
    use crate::type_declarations::{find_decl, rust_type_name};
    use crate::value::PrimitiveType;

    #[test]
    fn table_requires_id_field() {
        let mut schema = DatabaseSchema::new();
        let mut users = TableSchema::new("users");
        users.add_field("id", PrimitiveType::Id).unwrap();
        users.add_field("name", PrimitiveType::Text).unwrap();
        assert!(schema.add_table(users).is_ok());
    }

    #[test]
    fn schema_validates_types() {
        let mut schema = DatabaseSchema::new();
        let mut messages = TableSchema::new("messages");
        messages.add_field("id", PrimitiveType::Id).unwrap();
        messages.add_field("body", PrimitiveType::Text).unwrap();
        messages
            .add_field("created_at", PrimitiveType::Int)
            .unwrap();
        schema.add_table(messages).unwrap();

        let record = Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", "hello")
            .unwrap()
            .with_field("created_at", 42i64)
            .unwrap();
        assert!(schema.validate_record(&record).is_ok());

        let bad_record = Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", "hello")
            .unwrap()
            .with_field("created_at", "bad")
            .unwrap();
        assert!(schema.validate_record(&bad_record).is_err());
    }

    #[test]
    fn record_reserves_internal_prefix() {
        let value = Record::new("messages").with_field("_tx", "nope");
        assert!(matches!(value, Err(SchemaError::ReservedFieldName(_))));
    }

    #[test]
    fn primitive_type_declarations_map_to_rust_types() {
        let cases = [
            ("id", PrimitiveType::Id, "String"),
            ("text", PrimitiveType::Text, "String"),
            ("int", PrimitiveType::Int, "i64"),
            ("float", PrimitiveType::Float, "f64"),
            ("bool", PrimitiveType::Bool, "bool"),
        ];

        for (schema_name, primitive, rust_name) in cases {
            let decl = find_decl(schema_name).unwrap();
            assert_eq!(decl.schema_name, schema_name);
            assert_eq!(decl.rust_expr, rust_name);
            assert_eq!(rust_type_name(primitive), rust_name);
        }
    }
}
