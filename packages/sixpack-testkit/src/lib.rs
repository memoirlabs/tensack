//! Shared sixpack testing support.
//!
//! This crate is for reusable test harnesses, builders, assertions, and
//! compatibility checks used by workspace tests. It should not contain product
//! runtime logic.

use std::path::PathBuf;

use sixpack::Database;

/// Creates a database handle suitable for tests without touching the filesystem.
pub fn test_database() -> Database {
    Database::open_local(PathBuf::from(".sixpack-test"), "test")
}
