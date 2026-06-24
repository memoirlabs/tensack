use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sixpack::{
    Database, DatabaseSchema, GetPage, PrimitiveType, Record, TableSchema, Value, WriteChange,
};

const HTML: &str = include_str!("../static/playground.html");
const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 4766;
const DEFAULT_ROOT: &str = "target/test-lab/note-taking-playground";
const WORKSPACE: &str = "notes-db";
const NOTEBOOK_ID: &str = "inbox";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_args()?;
    if config.reset && config.root.exists() {
        fs::remove_dir_all(&config.root)?;
    }

    let app = Arc::new(Mutex::new(App::open(config.root)?));
    let bind = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&bind)?;
    println!("note-taking playground http://{bind}/");
    println!(
        "database {}",
        app.lock()
            .map_err(|_| "app lock poisoned")?
            .database_path()
            .display()
    );

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = handle_connection(stream, Arc::clone(&app)) {
                    eprintln!("request failed: {error}");
                }
            }
            Err(error) => eprintln!("connection failed: {error}"),
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct Config {
    host: String,
    port: u16,
    root: PathBuf,
    reset: bool,
}

impl Config {
    fn from_args() -> Result<Self, Box<dyn std::error::Error>> {
        let mut host = DEFAULT_HOST.to_owned();
        let mut port = DEFAULT_PORT;
        let mut root = PathBuf::from(DEFAULT_ROOT);
        let mut reset = false;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--host" => host = args.next().ok_or("--host requires a value")?,
                "--port" => port = args.next().ok_or("--port requires a value")?.parse()?,
                "--out" => root = PathBuf::from(args.next().ok_or("--out requires a value")?),
                "--reset" => reset = true,
                "-h" | "--help" => {
                    println!(
                        "note-taking-playground [--host <host>] [--port <port>] [--out <path>] [--reset]"
                    );
                    std::process::exit(0);
                }
                other => return Err(format!("unknown argument `{other}`").into()),
            }
        }
        Ok(Self {
            host,
            port,
            root,
            reset,
        })
    }
}

#[derive(Debug)]
struct App {
    db: Database,
    root: PathBuf,
    revision: u64,
}

impl App {
    fn open(root: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        fs::create_dir_all(&root)?;
        let db = Database::open_local_with_schema(&root, WORKSPACE, note_schema());
        db.init()?;
        let mut app = Self {
            db,
            root,
            revision: 0,
        };
        app.ensure_notebook()?;
        if app.list_notes()?.is_empty() {
            app.seed()?;
        }
        Ok(app)
    }

    fn database_path(&self) -> PathBuf {
        self.root.join(WORKSPACE)
    }

    fn ensure_notebook(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let record = Record::new("notebooks")
            .with_id(NOTEBOOK_ID)?
            .with_field("title", "Inbox")?
            .with_field("created_at", now_seconds())?;
        self.db.write(WriteChange::set_record(record))?;
        Ok(())
    }

    fn seed(&mut self) -> Result<Timing, Box<dyn std::error::Error>> {
        let start = Instant::now();
        for (id, title, body, offset) in [
            (
                "note-welcome",
                "Welcome note",
                "This row was written through the playground server.",
                1,
            ),
            (
                "note-speed",
                "Speed check",
                "Edit this note and watch write/read timing update.",
                2,
            ),
        ] {
            self.db.write(WriteChange::set_record(note_record(
                id,
                title,
                body,
                now_seconds() + offset,
            )?))?;
        }
        self.bump();
        Ok(Timing::from_elapsed(start))
    }

    fn create_note(
        &mut self,
        input: NoteInput,
    ) -> Result<MutationResult, Box<dyn std::error::Error>> {
        let start = Instant::now();
        let id = format!("note-{}", now_millis());
        let note = NoteDto {
            id,
            title: clean_text(input.title, "Untitled note"),
            body: clean_text(input.body, ""),
            updated_at: now_seconds(),
        };
        self.db.write(WriteChange::add_record(note.to_record()?))?;
        self.bump();
        Ok(MutationResult {
            note,
            timings: Timing::from_elapsed(start),
        })
    }

    fn update_note(
        &mut self,
        id: &str,
        input: NoteInput,
    ) -> Result<MutationResult, Box<dyn std::error::Error>> {
        let start = Instant::now();
        let updated_at = now_seconds();
        let mut patch = BTreeMap::new();
        patch.insert(
            "title".to_owned(),
            Value::Text(clean_text(input.title, "Untitled note")),
        );
        patch.insert("body".to_owned(), Value::Text(clean_text(input.body, "")));
        patch.insert("updated_at".to_owned(), Value::Int(updated_at));
        self.db.write(WriteChange::edit(
            "notes",
            "id",
            Value::Id(id.to_owned()),
            patch,
        ))?;
        self.bump();
        let note = self
            .list_notes()?
            .into_iter()
            .find(|note| note.id == id)
            .ok_or("updated note was not readable")?;
        Ok(MutationResult {
            note,
            timings: Timing::from_elapsed(start),
        })
    }

    fn delete_note(&mut self, id: &str) -> Result<Timing, Box<dyn std::error::Error>> {
        let start = Instant::now();
        self.db
            .write(WriteChange::remove("notes", "id", Value::Id(id.to_owned())))?;
        self.bump();
        Ok(Timing::from_elapsed(start))
    }

    fn compact(&mut self) -> Result<CompactDto, Box<dyn std::error::Error>> {
        let result = self.db.compact_table("notes")?;
        self.bump();
        Ok(CompactDto {
            table: result.table,
            live_rows: result.live_rows,
            bytes_before: result.bytes_before,
            bytes_after: result.bytes_after,
        })
    }

    fn state(&self) -> Result<StateDto, Box<dyn std::error::Error>> {
        let start = Instant::now();
        let notes = self.list_notes()?;
        Ok(StateDto {
            revision: self.revision,
            notes,
            disk: self.disk()?,
            timings: Timing::read(start),
        })
    }

    fn list_notes(&self) -> Result<Vec<NoteDto>, Box<dyn std::error::Error>> {
        let mut notes = self
            .db
            .get(GetPage::new("notes").limit(1_000))?
            .rows
            .into_iter()
            .map(NoteDto::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        notes.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(notes)
    }

    fn disk(&self) -> Result<DiskDto, Box<dyn std::error::Error>> {
        let database_path = self.database_path();
        let notes_dir = database_path.join("tables").join("notes");
        let mut six_bytes = 0u64;
        let mut chunk_count = 0usize;
        if notes_dir.exists() {
            for entry in fs::read_dir(&notes_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) == Some("6") {
                    chunk_count += 1;
                    six_bytes += fs::metadata(path)?.len();
                }
            }
        }
        let sixb_path = database_path.join("engine").join("notes.6b");
        let sixb_bytes = fs::metadata(sixb_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        Ok(DiskDto {
            database_path: database_path.display().to_string(),
            six_bytes,
            sixb_bytes,
            chunk_count,
        })
    }

    fn bump(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}

fn note_schema() -> DatabaseSchema {
    let mut schema = DatabaseSchema::new();
    let mut notebooks = TableSchema::new("notebooks");
    notebooks.add_field("id", PrimitiveType::Id).unwrap();
    notebooks.add_field("title", PrimitiveType::Text).unwrap();
    notebooks
        .add_field("created_at", PrimitiveType::Int)
        .unwrap();
    schema.add_table(notebooks).unwrap();

    let mut notes = TableSchema::new("notes");
    notes.add_field("id", PrimitiveType::Id).unwrap();
    notes.add_field("notebook_id", PrimitiveType::Id).unwrap();
    notes.add_field("title", PrimitiveType::Text).unwrap();
    notes.add_field("body", PrimitiveType::Text).unwrap();
    notes.add_field("updated_at", PrimitiveType::Int).unwrap();
    notes.add_lookup("notebook_id", false).unwrap();
    notes.add_lookup("updated_at", false).unwrap();
    schema.add_table(notes).unwrap();
    schema
}

fn note_record(
    id: &str,
    title: &str,
    body: &str,
    updated_at: i64,
) -> Result<Record, Box<dyn std::error::Error>> {
    let mut record = Record::new("notes").with_id(id)?;
    record.insert_field("notebook_id", Value::Id(NOTEBOOK_ID.to_owned()))?;
    record.insert_field("title", Value::Text(title.to_owned()))?;
    record.insert_field("body", Value::Text(body.to_owned()))?;
    record.insert_field("updated_at", Value::Int(updated_at))?;
    Ok(record)
}

#[derive(Debug, Deserialize)]
struct NoteInput {
    title: Option<String>,
    body: Option<String>,
}

#[derive(Debug, Serialize)]
struct MutationResult {
    note: NoteDto,
    timings: Timing,
}

#[derive(Debug, Serialize)]
struct StateDto {
    revision: u64,
    notes: Vec<NoteDto>,
    disk: DiskDto,
    timings: Timing,
}

#[derive(Debug, Serialize)]
struct NoteDto {
    id: String,
    title: String,
    body: String,
    updated_at: i64,
}

impl NoteDto {
    fn to_record(&self) -> Result<Record, Box<dyn std::error::Error>> {
        note_record(&self.id, &self.title, &self.body, self.updated_at)
    }
}

impl TryFrom<Record> for NoteDto {
    type Error = Box<dyn std::error::Error>;

    fn try_from(record: Record) -> Result<Self, Self::Error> {
        Ok(Self {
            id: required_id(&record, "id")?,
            title: required_text(&record, "title")?,
            body: required_text(&record, "body")?,
            updated_at: required_int(&record, "updated_at")?,
        })
    }
}

#[derive(Debug, Serialize)]
struct DiskDto {
    database_path: String,
    six_bytes: u64,
    sixb_bytes: u64,
    chunk_count: usize,
}

#[derive(Debug, Serialize)]
struct CompactDto {
    table: String,
    live_rows: usize,
    bytes_before: u64,
    bytes_after: u64,
}

#[derive(Debug, Serialize)]
struct Timing {
    #[serde(skip_serializing_if = "Option::is_none")]
    read_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    write_ms: Option<f64>,
}

impl Timing {
    fn from_elapsed(start: Instant) -> Self {
        Self {
            read_ms: None,
            write_ms: Some(start.elapsed().as_secs_f64() * 1_000.0),
        }
    }

    fn read(start: Instant) -> Self {
        Self {
            read_ms: Some(start.elapsed().as_secs_f64() * 1_000.0),
            write_ms: None,
        }
    }
}

fn required_id(record: &Record, field: &str) -> Result<String, Box<dyn std::error::Error>> {
    match record.fields().get(field) {
        Some(Value::Id(value)) => Ok(value.clone()),
        _ => Err(format!("missing id field `{field}`").into()),
    }
}

fn required_text(record: &Record, field: &str) -> Result<String, Box<dyn std::error::Error>> {
    match record.fields().get(field) {
        Some(Value::Text(value)) => Ok(value.clone()),
        _ => Err(format!("missing text field `{field}`").into()),
    }
}

fn required_int(record: &Record, field: &str) -> Result<i64, Box<dyn std::error::Error>> {
    match record.fields().get(field) {
        Some(Value::Int(value)) => Ok(*value),
        _ => Err(format!("missing int field `{field}`").into()),
    }
}

fn clean_text(value: Option<String>, fallback: &str) -> String {
    let value = value.unwrap_or_else(|| fallback.to_owned());
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_secs() as i64
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis()
}

#[derive(Debug)]
struct Request {
    method: String,
    path: String,
    body: Vec<u8>,
}

fn handle_connection(
    stream: TcpStream,
    app: Arc<Mutex<App>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let request = read_request(&stream)?;
    let response = route(request, app);
    write_response(stream, response)
}

fn read_request(stream: &TcpStream) -> Result<Request, Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or("missing method")?.to_owned();
    let raw_path = parts.next().ok_or("missing path")?;
    let path = raw_path.split('?').next().unwrap_or(raw_path).to_owned();

    let mut content_len = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            content_len = value.trim().parse()?;
        } else if let Some(value) = line.strip_prefix("content-length:") {
            content_len = value.trim().parse()?;
        }
    }
    let mut body = vec![0u8; content_len];
    if content_len > 0 {
        reader.read_exact(&mut body)?;
    }
    Ok(Request { method, path, body })
}

fn route(request: Request, app: Arc<Mutex<App>>) -> Response {
    let result = route_inner(request, app);
    match result {
        Ok(response) => response,
        Err(error) => Response::json(
            500,
            &serde_json::json!({
                "error": error.to_string()
            }),
        ),
    }
}

fn route_inner(
    request: Request,
    app: Arc<Mutex<App>>,
) -> Result<Response, Box<dyn std::error::Error>> {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => Ok(Response::html(HTML)),
        ("GET", "/api/state") => {
            let app = app.lock().map_err(|_| "app lock poisoned")?;
            Ok(Response::json(200, &app.state()?))
        }
        ("POST", "/api/notes") => {
            let input = parse_json::<NoteInput>(&request.body)?;
            let mut app = app.lock().map_err(|_| "app lock poisoned")?;
            Ok(Response::json(200, &app.create_note(input)?))
        }
        ("POST", "/api/seed") => {
            let mut app = app.lock().map_err(|_| "app lock poisoned")?;
            let timings = app.seed()?;
            Ok(Response::json(
                200,
                &serde_json::json!({ "timings": timings }),
            ))
        }
        ("POST", "/api/compact") => {
            let mut app = app.lock().map_err(|_| "app lock poisoned")?;
            Ok(Response::json(200, &app.compact()?))
        }
        _ if request.method == "PATCH" && request.path.starts_with("/api/notes/") => {
            let id = request.path.trim_start_matches("/api/notes/");
            let input = parse_json::<NoteInput>(&request.body)?;
            let mut app = app.lock().map_err(|_| "app lock poisoned")?;
            Ok(Response::json(200, &app.update_note(id, input)?))
        }
        _ if request.method == "DELETE" && request.path.starts_with("/api/notes/") => {
            let id = request.path.trim_start_matches("/api/notes/");
            let mut app = app.lock().map_err(|_| "app lock poisoned")?;
            let timings = app.delete_note(id)?;
            Ok(Response::json(
                200,
                &serde_json::json!({ "timings": timings }),
            ))
        }
        _ => Ok(Response::json(
            404,
            &serde_json::json!({ "error": "not found" }),
        )),
    }
}

fn parse_json<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, Box<dyn std::error::Error>> {
    if body.is_empty() {
        return Ok(serde_json::from_str("{}")?);
    }
    Ok(serde_json::from_slice(body)?)
}

#[derive(Debug)]
struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Response {
    fn html(body: &str) -> Self {
        Self {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: body.as_bytes().to_vec(),
        }
    }

    fn json<T: Serialize>(status: u16, value: &T) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            body: serde_json::to_vec(value).expect("json serialization failed"),
        }
    }
}

fn write_response(
    mut stream: TcpStream,
    response: Response,
) -> Result<(), Box<dyn std::error::Error>> {
    let status_text = match response.status {
        200 => "OK",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        status_text,
        response.content_type,
        response.body.len()
    )?;
    stream.write_all(&response.body)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn playground_crud_uses_real_database() {
        let root = temp_root();
        let mut app = App::open(root.clone()).unwrap();
        let before = app.state().unwrap();
        assert!(!before.notes.is_empty());

        let created = app
            .create_note(NoteInput {
                title: Some("Test note".to_owned()),
                body: Some("Created in test".to_owned()),
            })
            .unwrap();
        assert_eq!(created.note.title, "Test note");
        assert!(created.timings.write_ms.unwrap() >= 0.0);

        let updated = app
            .update_note(
                &created.note.id,
                NoteInput {
                    title: Some("Updated note".to_owned()),
                    body: Some("Patched in test".to_owned()),
                },
            )
            .unwrap();
        assert_eq!(updated.note.body, "Patched in test");

        let compacted = app.compact().unwrap();
        assert_eq!(compacted.table, "notes");
        assert!(compacted.bytes_after <= compacted.bytes_before);

        app.delete_note(&created.note.id).unwrap();
        assert!(
            app.list_notes()
                .unwrap()
                .into_iter()
                .all(|note| note.id != created.note.id)
        );

        let _ = fs::remove_dir_all(root);
    }

    fn temp_root() -> PathBuf {
        let mut path = std::env::temp_dir();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        path.push(format!(
            "sixpack-note-playground-{}-{counter}",
            now_millis()
        ));
        path
    }
}
