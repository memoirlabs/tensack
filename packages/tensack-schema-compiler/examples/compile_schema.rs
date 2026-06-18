use tensack_schema_compiler::{compile_schema, emit_raw_rust};

const SOURCE: &str = r#"
schema! {
    users {
        id id
        email text
        name text
        message_count int
        rating float
        disabled bool

        lookup email unique
        lookup disabled
    }

    conversations {
        id id
        owner_id id
        title text
        created_at int

        lookup owner_id
        lookup created_at
    }

    messages {
        id id
        conversation_id id
        sender_id id
        body text
        token_count int
        cost float
        flagged bool

        lookup conversation_id
        lookup flagged
    }
}
"#;

fn main() {
    let ir = compile_schema(SOURCE).expect("schema should compile");

    println!("tables: {}", ir.tables.len());
    for table in &ir.tables {
        println!(
            "- {}: {} fields, {} lookups",
            table.name,
            table.fields.len(),
            table.lookups.len()
        );
    }

    println!("\n--- raw generated rust ---");
    println!("{}", emit_raw_rust(&ir));
}
