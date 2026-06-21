//! Crude schema-first example for a basic AI chat app.
//!
//! The example stays intentionally small and only uses the primitive types:
//! `id`, `text`, `int`, `float`, `bool`.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tensack::{Record, TensackDatabase, Value, change};

mod chat_schema {
    use tensack::schema;

    include!("chat_schema.tensack");
}

fn temp_root() -> PathBuf {
    let mut path = std::env::temp_dir();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    path.push(format!("tensack-chat-example-{stamp}"));
    path
}

fn main() {
    let root = temp_root();
    let db = TensackDatabase::open_local_with_schema(
        root.clone(),
        "chat",
        chat_schema::database_schema(),
    );

    let user = Record::new("users")
        .with_id("u1")
        .unwrap()
        .with_field("name", "Mira")
        .unwrap()
        .with_field("email", "mira@example.com")
        .unwrap()
        .with_field("is_ai_user", false)
        .unwrap();

    let convo = Record::new("conversations")
        .with_id("c1")
        .unwrap()
        .with_field("owner_id", Value::Id("u1".to_owned()))
        .unwrap()
        .with_field("title", "Demo chat")
        .unwrap()
        .with_field("created_at", 1_707_000_000_i64)
        .unwrap();

    let message = Record::new("messages")
        .with_id("m1")
        .unwrap()
        .with_field("conversation_id", Value::Id("c1".to_owned()))
        .unwrap()
        .with_field("sender_id", Value::Id("u1".to_owned()))
        .unwrap()
        .with_field("body", "hello from crude schema macro")
        .unwrap()
        .with_field("created_at", 1_707_000_001_i64)
        .unwrap();

    db.write(change::set(user)).unwrap();
    db.write(change::set(convo)).unwrap();
    db.write(change::set(message)).unwrap();

    println!("basic schema example written");
    let _ = fs::remove_dir_all(root);
}
