//! Profile business-data storage (single SQLite database per Profile).
//!
//! The storage layer is deliberately small: it owns the `Database` connection
//! and the unified schema/migrations. All business logic (CRUD, domain rules,
//! use-cases) lives in the repository / domain / application layer of
//! `velowork-workspace`.

pub mod db;
pub mod migrations;

pub use db::{Database, database, init_database};

/// Re-exported so downstream crates can name `rusqlite` types (e.g. `Connection`,
/// `params!`) without taking a direct dependency on a possibly-different
/// rusqlite version/feature set.
pub use rusqlite;
