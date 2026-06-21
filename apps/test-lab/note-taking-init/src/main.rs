use std::fs;
use std::path::{Path, PathBuf};

use tensack::TensackDatabase;
use tensack_schema_compiler::{compile_schema, database_schema_from_ir, emit_raw_rust};

include!(concat!(env!("OUT_DIR"), "/tensack_generated_schema.rs"));

use tensack_generated_schema as sdk;

const SCHEMA_V1_SOURCE: &str = include_str!("../schema.v1.tensack");
const SCHEMA_V2_SOURCE: &str = include_str!("../schema.v2.tensack");
const SCHEMA_V3_SOURCE: &str = include_str!("../schema.tensack");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_root = output_root();
    let reset = std::env::args().any(|arg| arg == "--reset");
    let show_artifacts = std::env::args().any(|arg| arg == "--show-artifacts");
    if reset && output_root.exists() {
        fs::remove_dir_all(&output_root)?;
    }

    match phase_arg().as_deref() {
        Some("v1") => {
            init_note_database(&output_root, SCHEMA_V1_SOURCE, "schema-v1.rs")?;
            println!("initialized v1");
        }
        Some("v2") => {
            let db = init_note_database(&output_root, SCHEMA_V2_SOURCE, "schema-v2.rs")?;
            write_note_rows(&db)?;
            println!("initialized v2 and wrote sample rows");
        }
        Some("v3") => {
            let db = init_note_database(&output_root, SCHEMA_V3_SOURCE, "schema-v3.rs")?;
            write_note_rows(&db)?;
            write_tag_rows(&db)?;
            println!("initialized v3 and wrote sample rows");
        }
        Some(other) => {
            return Err(format!("unknown --phase `{other}`; expected v1, v2, or v3").into());
        }
        None => {
            init_note_database(&output_root, SCHEMA_V1_SOURCE, "schema-v1.rs")?;
            println!("after v1 init");

            let db_v2 = init_note_database(&output_root, SCHEMA_V2_SOURCE, "schema-v2.rs")?;
            write_note_rows(&db_v2)?;
            println!("after v2 init + write");

            let db_v3 = init_note_database(&output_root, SCHEMA_V3_SOURCE, "schema-v3.rs")?;
            write_tag_rows(&db_v3)?;
            println!("after v3 init + write");
        }
    }

    println!();
    print_current_view(&output_root);
    if show_artifacts {
        println!();
        println!("artifacts");
        print_tree(&output_root)?;
    }
    Ok(())
}

fn init_note_database(
    output_root: &Path,
    schema_source: &str,
    generated_file_name: &str,
) -> Result<TensackDatabase, Box<dyn std::error::Error>> {
    fs::create_dir_all(output_root)?;

    let ir = compile_schema(schema_source)?;
    let schema = database_schema_from_ir(&ir)?;

    let generated_dir = output_root.join("generated");
    let artifacts_dir = generated_dir.join("artifacts");
    fs::create_dir_all(&generated_dir)?;
    fs::create_dir_all(&artifacts_dir)?;
    let generated = emit_raw_rust(&ir);
    fs::write(artifacts_dir.join(generated_file_name), &generated)?;
    fs::write(generated_dir.join("schema.rs"), generated)?;

    let db = TensackDatabase::open_local_with_schema(output_root, "notes-db", schema);
    db.init()?;
    Ok(db)
}

fn print_current_view(output_root: &Path) {
    println!(
        "current schema {}",
        output_root.join("generated/schema.rs").display()
    );
    println!(
        "current database {}",
        output_root.join("notes-db").display()
    );
}

fn write_note_rows(db: &TensackDatabase) -> Result<(), Box<dyn std::error::Error>> {
    if db.get(sdk::notebooks::by::id("notebook-1"))?.is_none() {
        db.write(sdk::notebooks::add(sdk::notebooks::Row {
            id: "notebook-1".to_owned(),
            title: "Inbox".to_owned(),
            created_at: 1_700_000_000,
        }))?;
    }
    if db.get(sdk::notes::by::id("note-1"))?.is_none() {
        db.write(sdk::notes::add(sdk::notes::Row {
            id: "note-1".to_owned(),
            notebook_id: "notebook-1".to_owned(),
            title: "First note".to_owned(),
            body: "Prove a generated Rust SDK can write rows.".to_owned(),
            updated_at: 1_700_000_010,
        }))?;
    }

    let notes = db.get(sdk::notes::by::notebook_id("notebook-1"))?;
    println!("wrote {} note row(s) for notebook-1", notes.len());
    Ok(())
}

fn write_tag_rows(db: &TensackDatabase) -> Result<(), Box<dyn std::error::Error>> {
    if db.get(sdk::tags::by::id("tag-1"))?.is_none() {
        db.write(sdk::tags::add(sdk::tags::Row {
            id: "tag-1".to_owned(),
            note_id: "note-1".to_owned(),
            label: "demo".to_owned(),
            created_at: 1_700_000_020,
        }))?;
    }

    let tags = db.get(sdk::tags::by::note_id("note-1"))?;
    println!("wrote {} tag row(s) for note-1", tags.len());
    Ok(())
}

fn output_root() -> PathBuf {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--out"
            && let Some(path) = args.next()
        {
            return PathBuf::from(path);
        }
    }
    PathBuf::from("target/test-lab/note-taking-init")
}

fn phase_arg() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--phase" {
            return args.next();
        }
    }
    None
}

fn print_tree(root: &Path) -> std::io::Result<()> {
    println!("{}", root.display());
    print_tree_inner(root, 0)
}

fn print_tree_inner(path: &Path, depth: usize) -> std::io::Result<()> {
    let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let indent = "  ".repeat(depth + 1);
        let name = entry.file_name();
        println!("{indent}{}", name.to_string_lossy());
        if path.is_dir() {
            print_tree_inner(&path, depth + 1)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn compiler_init_can_evolve_empty_note_database() {
        let root = temp_root();
        init_note_database(&root, SCHEMA_V1_SOURCE, "schema-v1.rs").unwrap();

        let db = root.join("notes-db");
        assert!(root.join("generated/schema.rs").exists());
        assert!(root.join("generated/artifacts/schema-v1.rs").exists());
        assert!(!root.join("generated/schema-v1.rs").exists());
        assert!(db.join("tensack.toml").exists());
        assert!(db.join("engine/notebooks.tenb").exists());
        assert!(db.join("tables/notebooks").exists());
        assert!(!db.join("engine/notes.tenb").exists());
        assert!(!db.join("tables/notes").exists());

        let generated = fs::read_to_string(root.join("generated/schema.rs")).unwrap();
        assert!(generated.contains("pub mod notebooks"));
        assert!(!generated.contains("pub mod notes"));
        assert!(generated.contains("pub fn database_schema()"));

        let db_v2 = init_note_database(&root, SCHEMA_V2_SOURCE, "schema-v2.rs").unwrap();
        write_note_rows(&db_v2).unwrap();

        assert!(root.join("generated/artifacts/schema-v2.rs").exists());
        assert!(db.join("engine/notebooks.tenb").exists());
        assert!(db.join("engine/notes.tenb").exists());
        assert!(db.join("tables/notebooks/zzz.ten").exists());
        assert!(db.join("tables/notes/zzz.ten").exists());

        let generated = fs::read_to_string(root.join("generated/schema.rs")).unwrap();
        assert!(generated.contains("pub mod notebooks"));
        assert!(generated.contains("pub mod notes"));

        let notes = fs::read_to_string(db.join("tables/notes/zzz.ten")).unwrap();
        assert!(notes.contains("TEN\t1\ttable\tnotes\t"));
        assert!(notes.contains("@field\tnotebook_id\tid\n"));
        assert!(notes.contains("@lookup\tnotebook_id\tmany\n"));
        assert!(notes.contains("@data\n"));
        assert!(notes.contains("R\t2\tnote-1\tnotebook-1\tFirst note\t"));

        let note_rows = db_v2
            .get(sdk::notes::by::notebook_id("notebook-1"))
            .unwrap();
        assert_eq!(note_rows.len(), 1);
        assert_eq!(note_rows[0].title, "First note");

        let notes_cache = fs::read(db.join("engine/notes.tenb")).unwrap();
        assert!(notes_cache.starts_with(b"TENB\0"));

        let metadata = fs::read_to_string(db.join("tensack.toml")).unwrap();
        assert!(metadata.contains("[tables.notebooks]"));
        assert!(metadata.contains("[tables.notes]"));
        assert!(metadata.contains("file = \"engine/notebooks.tenb\""));
        assert!(metadata.contains("file = \"engine/notes.tenb\""));

        let db_v3 = init_note_database(&root, SCHEMA_V3_SOURCE, "schema-v3.rs").unwrap();
        write_tag_rows(&db_v3).unwrap();

        assert!(root.join("generated/artifacts/schema-v3.rs").exists());
        assert!(db.join("engine/tags.tenb").exists());
        assert!(db.join("tables/tags/zzz.ten").exists());

        let generated = fs::read_to_string(root.join("generated/schema.rs")).unwrap();
        assert!(generated.contains("pub mod tags"));
        assert!(generated.contains("pub struct TableHandle"));
        assert!(generated.contains("pub fn add(row: Row)"));
        assert!(generated.contains("pub fn note_id(&self, value: impl Into<String>)"));
        assert!(!generated.contains("pub fn get_many_by_note_id"));

        let tag_rows = db_v3.get(sdk::tags::by::note_id("note-1")).unwrap();
        assert_eq!(tag_rows.len(), 1);
        assert_eq!(tag_rows[0].label, "demo");

        db_v3
            .write(sdk::notes::edit(
                sdk::notes::key::id("note-1"),
                sdk::notes::Patch::new().title("Updated note"),
            ))
            .unwrap();
        let note = db_v3.get(sdk::notes::by::id("note-1")).unwrap().unwrap();
        assert_eq!(note.title, "Updated note");

        db_v3
            .write(sdk::tags::remove(sdk::tags::key::id("tag-1")))
            .unwrap();
        assert!(db_v3.get(sdk::tags::by::id("tag-1")).unwrap().is_none());
        assert!(
            db_v3
                .get(sdk::tags::by::note_id("note-1"))
                .unwrap()
                .is_empty()
        );

        let tags_cache = fs::read(db.join("engine/tags.tenb")).unwrap();
        assert!(tags_cache.starts_with(b"TENB\0"));

        let _ = fs::remove_dir_all(root);
    }

    fn temp_root() -> PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        path.push(format!("tensack-note-init-{stamp}"));
        path
    }
}
