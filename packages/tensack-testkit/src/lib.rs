//! Shared Tensack testing support.
//!
//! This crate is for reusable test harnesses, builders, assertions, and
//! compatibility checks used by workspace tests. It should not contain product
//! runtime logic.

use std::path::PathBuf;

use tensack::TensackDatabase;

/// Creates a database handle suitable for tests without touching the filesystem.
pub fn test_database() -> TensackDatabase {
    TensackDatabase::open_local(PathBuf::from(".tensack-test"), "test")
}
