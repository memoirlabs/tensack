use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use sixpack::{CompactionResult, Database};
use sixpack_schema_compiler::{compile_schema, database_schema_from_ir, emit_raw_rust};

include!(concat!(env!("OUT_DIR"), "/sixpack_generated_schema.rs"));

use sixpack_generated_schema as sdk;

const SCHEMA_V1_SOURCE: &str = include_str!("../schema.v1.sixpack");
const SCHEMA_V2_SOURCE: &str = include_str!("../schema.v2.sixpack");
const SCHEMA_V3_SOURCE: &str = include_str!("../schema.sixpack");
const VIEWER_TEMPLATE: &str = include_str!("../viewer.html");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_root = output_root();
    let reset = std::env::args().any(|arg| arg == "--reset");
    let show_internals = std::env::args().any(|arg| arg == "--show-internals");
    let compact_after_speed = std::env::args().any(|arg| arg == "--compact");
    let speed_updates = speed_updates_arg()?;
    let phase = phase_arg();
    if speed_updates.is_some() && matches!(phase.as_deref(), Some("v1")) {
        return Err(
            "--speed-updates requires the notes table; use phase v2, phase v3, or the full run"
                .into(),
        );
    }
    if reset && output_root.exists() {
        fs::remove_dir_all(&output_root)?;
    }

    let active_db = match phase.as_deref() {
        Some("v1") => {
            let db = init_note_database(&output_root, SCHEMA_V1_SOURCE, "schema-v1.rs")?;
            println!("initialized v1");
            Some(db)
        }
        Some("v2") => {
            let db = init_note_database(&output_root, SCHEMA_V2_SOURCE, "schema-v2.rs")?;
            write_note_rows(&db)?;
            println!("initialized v2 and wrote sample rows");
            Some(db)
        }
        Some("v3") => {
            let db = init_note_database(&output_root, SCHEMA_V3_SOURCE, "schema-v3.rs")?;
            write_note_rows(&db)?;
            write_tag_rows(&db)?;
            println!("initialized v3 and wrote sample rows");
            Some(db)
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
            Some(db_v3)
        }
    };

    if let Some(update_count) = speed_updates {
        let db = active_db
            .as_ref()
            .ok_or("speed checks need an initialized database phase")?;
        write_note_rows(db)?;
        let mut report = run_update_speed_check(db, update_count)?;
        if compact_after_speed {
            let compaction = db.compact_table(sdk::notes::NAME)?;
            println!(
                "compacted {}: {} live row(s), {} -> {} bytes",
                compaction.table,
                compaction.live_rows,
                compaction.bytes_before,
                compaction.bytes_after
            );
            report.compaction = Some(CompactionSummary::from(compaction));
        }
        write_speed_report(&output_root, &report)?;
        println!(
            "updated note-1 {} time(s) in {:.3} ms ({:.3} us/update)",
            report.updates, report.elapsed_ms, report.micros_per_update
        );
        println!(
            "speed report {}",
            output_root.join("generated/report.html").display()
        );
    }

    println!();
    print_current_view(&output_root);
    if show_internals {
        println!();
        println!("internals");
        print_tree(&output_root)?;
    }
    Ok(())
}

fn init_note_database(
    output_root: &Path,
    schema_source: &str,
    generated_file_name: &str,
) -> Result<Database, Box<dyn std::error::Error>> {
    fs::create_dir_all(output_root)?;

    let ir = compile_schema(schema_source)?;
    let schema = database_schema_from_ir(&ir)?;

    let generated_dir = output_root.join("generated");
    let internals_dir = generated_dir.join("internals");
    fs::create_dir_all(&generated_dir)?;
    fs::create_dir_all(&internals_dir)?;
    let generated = emit_raw_rust(&ir);
    fs::write(internals_dir.join(generated_file_name), &generated)?;
    fs::write(generated_dir.join("schema.rs"), generated)?;

    let db = Database::open_local_with_schema(output_root, "notes-db", schema);
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

fn write_note_rows(db: &Database) -> Result<(), Box<dyn std::error::Error>> {
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

fn write_tag_rows(db: &Database) -> Result<(), Box<dyn std::error::Error>> {
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

#[derive(Debug, Clone)]
struct SpeedReport {
    updates: usize,
    elapsed_ms: f64,
    micros_per_update: f64,
    final_title: String,
    final_updated_at: i64,
    compaction: Option<CompactionSummary>,
}

#[derive(Debug, Clone)]
struct CompactionSummary {
    table: String,
    live_rows: usize,
    chunks_before: usize,
    chunks_after: usize,
    bytes_before: u64,
    bytes_after: u64,
    chunk_name: String,
}

impl From<CompactionResult> for CompactionSummary {
    fn from(result: CompactionResult) -> Self {
        Self {
            table: result.table,
            live_rows: result.live_rows,
            chunks_before: result.chunks_before,
            chunks_after: result.chunks_after,
            bytes_before: result.bytes_before,
            bytes_after: result.bytes_after,
            chunk_name: result.chunk_name,
        }
    }
}

fn run_update_speed_check(
    db: &Database,
    updates: usize,
) -> Result<SpeedReport, Box<dyn std::error::Error>> {
    if updates == 0 {
        return Err("--speed-updates must be greater than 0".into());
    }

    let start = Instant::now();
    for index in 0..updates {
        let updated_at = 1_700_100_000 + i64::try_from(index)?;
        db.write(sdk::notes::edit(
            sdk::notes::key::id("note-1"),
            sdk::notes::Patch::new()
                .title(format!("Speed pass {}", index + 1))
                .body(format!("Updated through generated SDK write {}", index + 1))
                .updated_at(updated_at),
        ))?;
    }
    let elapsed = start.elapsed();
    let note = db.get(sdk::notes::by::id("note-1"))?.unwrap();
    let elapsed_ms = elapsed.as_secs_f64() * 1_000.0;
    let micros_per_update = elapsed.as_secs_f64() * 1_000_000.0 / updates as f64;

    Ok(SpeedReport {
        updates,
        elapsed_ms,
        micros_per_update,
        final_title: note.title,
        final_updated_at: note.updated_at,
        compaction: None,
    })
}

fn write_speed_report(
    output_root: &Path,
    report: &SpeedReport,
) -> Result<(), Box<dyn std::error::Error>> {
    let generated_dir = output_root.join("generated");
    fs::create_dir_all(&generated_dir)?;
    let json = speed_report_json(report);
    fs::write(generated_dir.join("speed-report.json"), &json)?;
    let html = VIEWER_TEMPLATE.replace(
        "window.__SIXPACK_NOTE_REPORT__ = null;",
        &format!("window.__SIXPACK_NOTE_REPORT__ = {json};"),
    );
    fs::write(generated_dir.join("report.html"), html)?;
    Ok(())
}

fn speed_report_json(report: &SpeedReport) -> String {
    let compaction = report
        .compaction
        .as_ref()
        .map(compaction_summary_json)
        .unwrap_or_else(|| "null".to_owned());
    format!(
        concat!(
            "{{\n",
            "  \"updates\": {},\n",
            "  \"elapsed_ms\": {:.3},\n",
            "  \"micros_per_update\": {:.3},\n",
            "  \"final_title\": \"{}\",\n",
            "  \"final_updated_at\": {},\n",
            "  \"compaction\": {},\n",
            "  \"layout\": {{\n",
            "    \"canonical_data\": \"tables/<table>/*.6\",\n",
            "    \"current_engine_state\": \"engine/*.6b\",\n",
            "    \"target_engine_state\": \"engine/state.6pack\"\n",
            "  }}\n",
            "}}\n"
        ),
        report.updates,
        report.elapsed_ms,
        report.micros_per_update,
        json_escape(&report.final_title),
        report.final_updated_at,
        compaction
    )
}

fn compaction_summary_json(summary: &CompactionSummary) -> String {
    format!(
        concat!(
            "{{\n",
            "    \"table\": \"{}\",\n",
            "    \"live_rows\": {},\n",
            "    \"chunks_before\": {},\n",
            "    \"chunks_after\": {},\n",
            "    \"bytes_before\": {},\n",
            "    \"bytes_after\": {},\n",
            "    \"chunk_name\": \"{}\"\n",
            "  }}"
        ),
        json_escape(&summary.table),
        summary.live_rows,
        summary.chunks_before,
        summary.chunks_after,
        summary.bytes_before,
        summary.bytes_after,
        json_escape(&summary.chunk_name)
    )
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped
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

fn speed_updates_arg() -> Result<Option<usize>, Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--speed-updates" {
            let value = args
                .next()
                .ok_or("--speed-updates requires an update count")?
                .parse::<usize>()?;
            return Ok(Some(value));
        }
    }
    Ok(None)
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
        assert!(root.join("generated/internals/schema-v1.rs").exists());
        assert!(!root.join("generated/schema-v1.rs").exists());
        assert!(db.join("sixpack.toml").exists());
        assert!(db.join("engine/notebooks.6b").exists());
        assert!(db.join("tables/notebooks").exists());
        assert!(!db.join("engine/notes.6b").exists());
        assert!(!db.join("tables/notes").exists());

        let generated = fs::read_to_string(root.join("generated/schema.rs")).unwrap();
        assert!(generated.contains("pub mod notebooks"));
        assert!(!generated.contains("pub mod notes"));
        assert!(generated.contains("pub fn database_schema()"));

        let db_v2 = init_note_database(&root, SCHEMA_V2_SOURCE, "schema-v2.rs").unwrap();
        write_note_rows(&db_v2).unwrap();

        assert!(root.join("generated/internals/schema-v2.rs").exists());
        assert!(db.join("engine/notebooks.6b").exists());
        assert!(db.join("engine/notes.6b").exists());
        assert!(db.join("tables/notebooks/zzz.6").exists());
        assert!(db.join("tables/notes/zzz.6").exists());

        let generated = fs::read_to_string(root.join("generated/schema.rs")).unwrap();
        assert!(generated.contains("pub mod notebooks"));
        assert!(generated.contains("pub mod notes"));

        let notes = fs::read_to_string(db.join("tables/notes/zzz.6")).unwrap();
        assert!(notes.contains("SIX\t1\ttable\tnotes\t"));
        assert!(notes.contains("@field\tnotebook_id\tid\n"));
        assert!(notes.contains("@lookup\tnotebook_id\tmany\n"));
        assert!(notes.contains("@data\n"));
        assert!(notes.contains("R\t2\tnote-1\tnotebook-1\tFirst note\t"));

        let note_rows = db_v2
            .get(sdk::notes::by::notebook_id("notebook-1"))
            .unwrap();
        assert_eq!(note_rows.len(), 1);
        assert_eq!(note_rows[0].title, "First note");

        let notes_cache = fs::read(db.join("engine/notes.6b")).unwrap();
        assert!(notes_cache.starts_with(b"SIXB\0"));

        let metadata = fs::read_to_string(db.join("sixpack.toml")).unwrap();
        assert!(metadata.contains("[tables.notebooks]"));
        assert!(metadata.contains("[tables.notes]"));
        assert!(metadata.contains("file = \"engine/notebooks.6b\""));
        assert!(metadata.contains("file = \"engine/notes.6b\""));

        let db_v3 = init_note_database(&root, SCHEMA_V3_SOURCE, "schema-v3.rs").unwrap();
        write_tag_rows(&db_v3).unwrap();

        assert!(root.join("generated/internals/schema-v3.rs").exists());
        assert!(db.join("engine/tags.6b").exists());
        assert!(db.join("tables/tags/zzz.6").exists());

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

        let tags_cache = fs::read(db.join("engine/tags.6b")).unwrap();
        assert!(tags_cache.starts_with(b"SIXB\0"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn speed_check_writes_report_without_changing_layout_contract() {
        let root = temp_root();
        let db = init_note_database(&root, SCHEMA_V3_SOURCE, "schema-v3.rs").unwrap();
        write_note_rows(&db).unwrap();
        write_tag_rows(&db).unwrap();

        let report = run_update_speed_check(&db, 5).unwrap();
        write_speed_report(&root, &report).unwrap();

        assert_eq!(report.updates, 5);
        assert!(report.micros_per_update >= 0.0);
        assert!(root.join("generated/speed-report.json").exists());
        assert!(root.join("generated/report.html").exists());
        assert!(root.join("notes-db/tables/notes/zzz.6").exists());
        assert!(root.join("notes-db/engine/notes.6b").exists());
        assert!(!root.join("notes-db/engine/state.6pack").exists());

        let json = fs::read_to_string(root.join("generated/speed-report.json")).unwrap();
        assert!(json.contains("\"updates\": 5"));
        assert!(json.contains("\"current_engine_state\": \"engine/*.6b\""));

        let _ = fs::remove_dir_all(root);
    }

    fn temp_root() -> PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        path.push(format!("sixpack-note-init-{stamp}"));
        path
    }
}
