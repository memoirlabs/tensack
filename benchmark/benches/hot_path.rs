use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use rusqlite::{Connection, params};
use sixpack::{
    Database, DatabaseSchema, PrimitiveType, Record, TableSchema, Value, change, selector,
};
use tempfile::TempDir;

const HOT_ROWS: usize = 10_000;
const OPS_PER_ITER: usize = 1_000;
const TABLE: &str = "events";

fn event_schema() -> DatabaseSchema {
    let mut schema = DatabaseSchema::new();
    let mut events = TableSchema::new(TABLE);
    events.add_field("id", PrimitiveType::Id).unwrap();
    events.add_field("stream_id", PrimitiveType::Id).unwrap();
    events.add_field("kind", PrimitiveType::Text).unwrap();
    events.add_field("payload", PrimitiveType::Text).unwrap();
    events.add_field("score", PrimitiveType::Int).unwrap();
    events.add_lookup("stream_id", false).unwrap();
    schema.add_table(events).unwrap();
    schema
}

fn payload(index: usize) -> String {
    format!(
        "event-{index:08}:abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ:{}",
        index % 997
    )
}

fn event_id(index: usize) -> String {
    format!("e{index:012}")
}

fn event_record(index: usize) -> Record {
    Record::new(TABLE)
        .with_id(event_id(index))
        .unwrap()
        .with_field("stream_id", Value::Id(format!("s{}", index % 128)))
        .unwrap()
        .with_field("kind", format!("kind-{}", index % 16))
        .unwrap()
        .with_field("payload", payload(index))
        .unwrap()
        .with_field("score", index as i64)
        .unwrap()
}

fn open_sixpack_hot(rows: usize) -> (TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open_local_with_schema(dir.path(), "hot", event_schema());
    db.init().unwrap();
    let records = (0..rows).map(event_record).collect::<Vec<_>>();
    db.insert_many(&records).unwrap();
    (dir, db)
}

fn open_sqlite_hot(rows: usize) -> (TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("hot.sqlite3");
    let mut conn = Connection::open(db_path).unwrap();
    conn.execute_batch(
        "
        CREATE TABLE events (
            id TEXT PRIMARY KEY,
            stream_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            payload TEXT NOT NULL,
            score INTEGER NOT NULL
        );
        CREATE INDEX events_stream_id ON events(stream_id);
        ",
    )
    .unwrap();
    insert_sqlite_events_in_transaction(&mut conn, 0, rows);
    (dir, conn)
}

fn insert_sqlite_event(conn: &Connection, index: usize) {
    conn.execute(
        "INSERT INTO events (id, stream_id, kind, payload, score) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            event_id(index),
            format!("s{}", index % 128),
            format!("kind-{}", index % 16),
            payload(index),
            index as i64
        ],
    )
    .unwrap();
}

fn insert_sqlite_events_in_transaction(conn: &mut Connection, start: usize, rows: usize) {
    let tx = conn.transaction().unwrap();
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO events (id, stream_id, kind, payload, score) VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .unwrap();
        for index in start..start + rows {
            stmt.execute(params![
                event_id(index),
                format!("s{}", index % 128),
                format!("kind-{}", index % 16),
                payload(index),
                index as i64
            ])
            .unwrap();
        }
    }
    tx.commit().unwrap();
}

fn update_sqlite_events_in_transaction(conn: &mut Connection, start: usize, rows: usize) {
    let tx = conn.transaction().unwrap();
    {
        let mut stmt = tx
            .prepare("UPDATE events SET payload = ?1, score = ?2 WHERE id = ?3")
            .unwrap();
        for offset in 0..rows {
            let index = (start + offset) % HOT_ROWS;
            stmt.execute(params![
                payload(start + offset + HOT_ROWS),
                (start + offset + HOT_ROWS) as i64,
                event_id(index)
            ])
            .unwrap();
        }
    }
    tx.commit().unwrap();
}

fn bench_hot_reads(c: &mut Criterion) {
    let mut group = c.benchmark_group("hot_reads_10k_rows");
    group.throughput(Throughput::Elements(OPS_PER_ITER as u64));

    let (_sixpack_dir, sixpack) = open_sixpack_hot(HOT_ROWS);
    group.bench_function("sixpack_get_by_id", |b| {
        let mut start = 0usize;
        b.iter(|| {
            for offset in 0..OPS_PER_ITER {
                let index = (start + offset) % HOT_ROWS;
                let row = sixpack.get(selector::id(TABLE, event_id(index))).unwrap();
                black_box(row);
            }
            start = (start + OPS_PER_ITER) % HOT_ROWS;
        });
    });

    let (_sqlite_dir, conn) = open_sqlite_hot(HOT_ROWS);
    group.bench_function("sqlite_select_by_id", |b| {
        let mut start = 0usize;
        b.iter(|| {
            let mut stmt = conn
                .prepare("SELECT id, stream_id, kind, payload, score FROM events WHERE id = ?1")
                .unwrap();
            for offset in 0..OPS_PER_ITER {
                let index = (start + offset) % HOT_ROWS;
                let row = stmt
                    .query_row([event_id(index)], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                        ))
                    })
                    .unwrap();
                black_box(row);
            }
            start = (start + OPS_PER_ITER) % HOT_ROWS;
        });
    });

    group.finish();
}

fn bench_hot_counts(c: &mut Criterion) {
    let mut group = c.benchmark_group("hot_counts_10k_rows");
    group.throughput(Throughput::Elements(OPS_PER_ITER as u64));

    let (_sixpack_dir, sixpack) = open_sixpack_hot(HOT_ROWS);
    group.bench_function("sixpack_count", |b| {
        b.iter(|| {
            for _ in 0..OPS_PER_ITER {
                black_box(sixpack.count(TABLE).unwrap());
            }
        });
    });

    let (_sqlite_dir, conn) = open_sqlite_hot(HOT_ROWS);
    group.bench_function("sqlite_count", |b| {
        b.iter(|| {
            let mut stmt = conn.prepare("SELECT COUNT(*) FROM events").unwrap();
            for _ in 0..OPS_PER_ITER {
                let count: i64 = stmt.query_row([], |row| row.get(0)).unwrap();
                black_box(count);
            }
        });
    });

    group.finish();
}

fn bench_hot_appends(c: &mut Criterion) {
    let mut group = c.benchmark_group("hot_appends_10k_rows");
    group.throughput(Throughput::Elements(OPS_PER_ITER as u64));

    let (_sixpack_dir, sixpack) = open_sixpack_hot(HOT_ROWS);
    let sixpack_next = AtomicUsize::new(HOT_ROWS);
    group.bench_function("sixpack_write_add", |b| {
        b.iter(|| {
            let next = sixpack_next.fetch_add(OPS_PER_ITER, Ordering::Relaxed);
            for index in next..next + OPS_PER_ITER {
                sixpack.write(change::add(event_record(index))).unwrap();
            }
        });
    });

    let (_sixpack_batch_dir, sixpack_batch) = open_sixpack_hot(HOT_ROWS);
    let sixpack_batch_next = AtomicUsize::new(HOT_ROWS);
    group.bench_function("sixpack_insert_many", |b| {
        b.iter(|| {
            let next = sixpack_batch_next.fetch_add(OPS_PER_ITER, Ordering::Relaxed);
            let records = (next..next + OPS_PER_ITER)
                .map(event_record)
                .collect::<Vec<_>>();
            sixpack_batch.insert_many(&records).unwrap();
        });
    });

    let (_sqlite_dir, conn) = open_sqlite_hot(HOT_ROWS);
    let sqlite_next = AtomicUsize::new(HOT_ROWS);
    group.bench_function("sqlite_insert", |b| {
        b.iter(|| {
            let next = sqlite_next.fetch_add(OPS_PER_ITER, Ordering::Relaxed);
            for index in next..next + OPS_PER_ITER {
                insert_sqlite_event(&conn, index);
            }
        });
    });

    let (_sqlite_batch_dir, mut sqlite_batch) = open_sqlite_hot(HOT_ROWS);
    let sqlite_batch_next = AtomicUsize::new(HOT_ROWS);
    group.bench_function("sqlite_insert_transaction", |b| {
        b.iter(|| {
            let next = sqlite_batch_next.fetch_add(OPS_PER_ITER, Ordering::Relaxed);
            insert_sqlite_events_in_transaction(&mut sqlite_batch, next, OPS_PER_ITER);
        });
    });

    group.finish();
}

fn bench_hot_edits(c: &mut Criterion) {
    let mut group = c.benchmark_group("hot_edits_10k_rows");
    group.throughput(Throughput::Elements(OPS_PER_ITER as u64));

    let (_sixpack_dir, sixpack) = open_sixpack_hot(HOT_ROWS);
    group.bench_function("sixpack_write_edit", |b| {
        let mut start = 0usize;
        b.iter(|| {
            for offset in 0..OPS_PER_ITER {
                let index = (start + offset) % HOT_ROWS;
                sixpack
                    .write(change::edit_id(
                        TABLE,
                        event_id(index),
                        BTreeMap::from([
                            (
                                "payload".to_owned(),
                                Value::Text(payload(start + offset + HOT_ROWS)),
                            ),
                            (
                                "score".to_owned(),
                                Value::Int((start + offset + HOT_ROWS) as i64),
                            ),
                        ]),
                    ))
                    .unwrap();
            }
            start = (start + OPS_PER_ITER) % HOT_ROWS;
        });
    });

    let (_sixpack_batch_dir, sixpack_batch) = open_sixpack_hot(HOT_ROWS);
    group.bench_function("sixpack_write_many_edit", |b| {
        let mut start = 0usize;
        b.iter(|| {
            let changes = (0..OPS_PER_ITER)
                .map(|offset| {
                    let index = (start + offset) % HOT_ROWS;
                    change::edit_id(
                        TABLE,
                        event_id(index),
                        BTreeMap::from([
                            (
                                "payload".to_owned(),
                                Value::Text(payload(start + offset + HOT_ROWS)),
                            ),
                            (
                                "score".to_owned(),
                                Value::Int((start + offset + HOT_ROWS) as i64),
                            ),
                        ]),
                    )
                })
                .collect::<Vec<_>>();
            sixpack_batch.write_many(&changes).unwrap();
            start = (start + OPS_PER_ITER) % HOT_ROWS;
        });
    });

    let (_sqlite_dir, conn) = open_sqlite_hot(HOT_ROWS);
    group.bench_function("sqlite_update", |b| {
        let mut start = 0usize;
        b.iter(|| {
            for offset in 0..OPS_PER_ITER {
                let index = (start + offset) % HOT_ROWS;
                conn.execute(
                    "UPDATE events SET payload = ?1, score = ?2 WHERE id = ?3",
                    params![
                        payload(start + offset + HOT_ROWS),
                        (start + offset + HOT_ROWS) as i64,
                        event_id(index)
                    ],
                )
                .unwrap();
            }
            start = (start + OPS_PER_ITER) % HOT_ROWS;
        });
    });

    let (_sqlite_batch_dir, mut sqlite_batch) = open_sqlite_hot(HOT_ROWS);
    group.bench_function("sqlite_update_transaction", |b| {
        let mut start = 0usize;
        b.iter(|| {
            update_sqlite_events_in_transaction(&mut sqlite_batch, start, OPS_PER_ITER);
            start = (start + OPS_PER_ITER) % HOT_ROWS;
        });
    });

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = bench_hot_reads, bench_hot_counts, bench_hot_appends, bench_hot_edits
);
criterion_main!(benches);
