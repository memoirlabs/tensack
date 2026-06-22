use std::collections::BTreeMap;

use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main,
};
use rusqlite::{Connection, params};
use sixpack::{
    Database, DatabaseSchema, PrimitiveType, Record, TableSchema, Value, change, selector,
};
use tempfile::TempDir;

const ROW_COUNTS: &[usize] = &[25, 100];
const TABLE: &str = "users";

fn user_schema() -> DatabaseSchema {
    let mut schema = DatabaseSchema::new();
    let mut users = TableSchema::new(TABLE);
    users.add_field("id", PrimitiveType::Id).unwrap();
    users.add_field("email", PrimitiveType::Text).unwrap();
    users.add_field("name", PrimitiveType::Text).unwrap();
    users.add_field("age", PrimitiveType::Int).unwrap();
    users.add_lookup("email", true).unwrap();
    schema.add_table(users).unwrap();
    schema
}

fn user_record(index: usize) -> Record {
    Record::new(TABLE)
        .with_id(format!("u{index}"))
        .unwrap()
        .with_field("email", format!("user{index}@example.test"))
        .unwrap()
        .with_field("name", format!("User {index}"))
        .unwrap()
        .with_field("age", index as i64)
        .unwrap()
}

fn open_sixpack() -> (TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open_local_with_schema(dir.path(), "bench", user_schema());
    db.init().unwrap();
    (dir, db)
}

fn populated_sixpack(rows: usize) -> (TempDir, Database) {
    let (dir, db) = open_sixpack();
    for index in 0..rows {
        db.write(change::add(user_record(index))).unwrap();
    }
    (dir, db)
}

fn open_sqlite() -> (TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("bench.sqlite3");
    std::fs::File::create(&db_path).unwrap();
    let conn = Connection::open(db_path).unwrap();
    conn.execute_batch(
        "
        CREATE TABLE users (
            id TEXT PRIMARY KEY,
            email TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            age INTEGER NOT NULL
        );
        ",
    )
    .unwrap();
    (dir, conn)
}

fn open_sqlite_memory() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "
        CREATE TABLE users (
            id TEXT PRIMARY KEY,
            email TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            age INTEGER NOT NULL
        );
        ",
    )
    .unwrap();
    conn
}

fn insert_sqlite_user(conn: &Connection, index: usize) {
    conn.execute(
        "INSERT INTO users (id, email, name, age) VALUES (?1, ?2, ?3, ?4)",
        params![
            format!("u{index}"),
            format!("user{index}@example.test"),
            format!("User {index}"),
            index as i64
        ],
    )
    .unwrap();
}

fn insert_sqlite_users_in_transaction(conn: &mut Connection, rows: usize) {
    let tx = conn.transaction().unwrap();
    {
        let mut stmt = tx
            .prepare("INSERT INTO users (id, email, name, age) VALUES (?1, ?2, ?3, ?4)")
            .unwrap();
        for index in 0..rows {
            stmt.execute(params![
                format!("u{index}"),
                format!("user{index}@example.test"),
                format!("User {index}"),
                index as i64
            ])
            .unwrap();
        }
    }
    tx.commit().unwrap();
}

fn populated_sqlite(rows: usize) -> (TempDir, Connection) {
    let (dir, conn) = open_sqlite();
    for index in 0..rows {
        insert_sqlite_user(&conn, index);
    }
    (dir, conn)
}

fn bench_create(c: &mut Criterion) {
    let mut group = c.benchmark_group("crud_create");

    for &rows in ROW_COUNTS {
        group.throughput(Throughput::Elements(rows as u64));

        group.bench_with_input(
            BenchmarkId::new("sixpack_write_add", rows),
            &rows,
            |b, &rows| {
                b.iter_batched(
                    open_sixpack,
                    |(_dir, db)| {
                        for index in 0..rows {
                            db.write(change::add(user_record(index))).unwrap();
                        }
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("sqlite_insert", rows),
            &rows,
            |b, &rows| {
                b.iter_batched(
                    open_sqlite,
                    |(_dir, conn)| {
                        for index in 0..rows {
                            insert_sqlite_user(&conn, index);
                        }
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("sixpack_insert_many", rows),
            &rows,
            |b, &rows| {
                b.iter_batched(
                    open_sixpack,
                    |(_dir, db)| {
                        let records: Vec<_> = (0..rows).map(user_record).collect();
                        db.insert_many(&records).unwrap();
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("sqlite_insert_transaction_memory", rows),
            &rows,
            |b, &rows| {
                b.iter_batched(
                    open_sqlite_memory,
                    |mut conn| {
                        insert_sqlite_users_in_transaction(&mut conn, rows);
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("crud_read_by_id");

    for &rows in ROW_COUNTS {
        group.throughput(Throughput::Elements(rows as u64));

        group.bench_with_input(BenchmarkId::new("sixpack_get", rows), &rows, |b, &rows| {
            b.iter_batched(
                || populated_sixpack(rows),
                |(_dir, db)| {
                    for index in 0..rows {
                        let row = db.get(selector::id(TABLE, format!("u{index}"))).unwrap();
                        black_box(row);
                    }
                },
                BatchSize::SmallInput,
            );
        });

        group.bench_with_input(
            BenchmarkId::new("sqlite_select", rows),
            &rows,
            |b, &rows| {
                b.iter_batched(
                    || populated_sqlite(rows),
                    |(_dir, conn)| {
                        let mut stmt = conn
                            .prepare("SELECT id, email, name, age FROM users WHERE id = ?1")
                            .unwrap();
                        for index in 0..rows {
                            let row = stmt
                                .query_row([format!("u{index}")], |row| {
                                    Ok((
                                        row.get::<_, String>(0)?,
                                        row.get::<_, String>(1)?,
                                        row.get::<_, String>(2)?,
                                        row.get::<_, i64>(3)?,
                                    ))
                                })
                                .unwrap();
                            black_box(row);
                        }
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_count(c: &mut Criterion) {
    let mut group = c.benchmark_group("crud_count_binary_projection");

    for &rows in ROW_COUNTS {
        group.throughput(Throughput::Elements(rows as u64));

        group.bench_with_input(
            BenchmarkId::new("sixpack_count", rows),
            &rows,
            |b, &rows| {
                b.iter_batched(
                    || populated_sixpack(rows),
                    |(_dir, db)| {
                        let count = db.count(TABLE).unwrap();
                        black_box(count);
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(BenchmarkId::new("sqlite_count", rows), &rows, |b, &rows| {
            b.iter_batched(
                || populated_sqlite(rows),
                |(_dir, conn)| {
                    let count: i64 = conn
                        .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
                        .unwrap();
                    black_box(count);
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn bench_update(c: &mut Criterion) {
    let mut group = c.benchmark_group("crud_update");

    for &rows in ROW_COUNTS {
        group.throughput(Throughput::Elements(rows as u64));

        group.bench_with_input(
            BenchmarkId::new("sixpack_write_edit", rows),
            &rows,
            |b, &rows| {
                b.iter_batched(
                    || populated_sixpack(rows),
                    |(_dir, db)| {
                        for index in 0..rows {
                            db.write(change::edit_id(
                                TABLE,
                                format!("u{index}"),
                                BTreeMap::from([
                                    ("name".to_owned(), Value::Text(format!("Updated {index}"))),
                                    ("age".to_owned(), Value::Int((index + 1) as i64)),
                                ]),
                            ))
                            .unwrap();
                        }
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("sixpack_write_many_edit", rows),
            &rows,
            |b, &rows| {
                b.iter_batched(
                    || populated_sixpack(rows),
                    |(_dir, db)| {
                        let changes = (0..rows)
                            .map(|index| {
                                change::edit_id(
                                    TABLE,
                                    format!("u{index}"),
                                    BTreeMap::from([
                                        (
                                            "name".to_owned(),
                                            Value::Text(format!("Updated {index}")),
                                        ),
                                        ("age".to_owned(), Value::Int((index + 1) as i64)),
                                    ]),
                                )
                            })
                            .collect::<Vec<_>>();
                        db.write_many(&changes).unwrap();
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("sqlite_update", rows),
            &rows,
            |b, &rows| {
                b.iter_batched(
                    || populated_sqlite(rows),
                    |(_dir, conn)| {
                        for index in 0..rows {
                            conn.execute(
                                "UPDATE users SET name = ?1, age = ?2 WHERE id = ?3",
                                params![
                                    format!("Updated {index}"),
                                    (index + 1) as i64,
                                    format!("u{index}")
                                ],
                            )
                            .unwrap();
                        }
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_delete(c: &mut Criterion) {
    let mut group = c.benchmark_group("crud_delete");

    for &rows in ROW_COUNTS {
        group.throughput(Throughput::Elements(rows as u64));

        group.bench_with_input(
            BenchmarkId::new("sixpack_write_remove", rows),
            &rows,
            |b, &rows| {
                b.iter_batched(
                    || populated_sixpack(rows),
                    |(_dir, db)| {
                        for index in 0..rows {
                            db.write(change::remove_id(TABLE, format!("u{index}")))
                                .unwrap();
                        }
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("sixpack_write_many_remove", rows),
            &rows,
            |b, &rows| {
                b.iter_batched(
                    || populated_sixpack(rows),
                    |(_dir, db)| {
                        let changes = (0..rows)
                            .map(|index| change::remove_id(TABLE, format!("u{index}")))
                            .collect::<Vec<_>>();
                        db.write_many(&changes).unwrap();
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("sqlite_delete", rows),
            &rows,
            |b, &rows| {
                b.iter_batched(
                    || populated_sqlite(rows),
                    |(_dir, conn)| {
                        for index in 0..rows {
                            conn.execute("DELETE FROM users WHERE id = ?1", [format!("u{index}")])
                                .unwrap();
                        }
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = bench_create, bench_read, bench_count, bench_update, bench_delete
);
criterion_main!(benches);
