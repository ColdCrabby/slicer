//! Database migrator — applies all pending migrations on startup.
//!
//! Add new migration structs to the `migrations()` vec in chronological order.
//! [`sea_orm_migration::MigratorTrait::up`] is idempotent: already-applied
//! migrations are skipped automatically via the `seaql_migrations` bookkeeping
//! table that SeaORM maintains.

use sea_orm_migration::prelude::*;

use crate::db::migrations::m20250101_000001_initial;
use crate::db::migrations::m20250201_000002_gcode_cache;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250101_000001_initial::Migration),
            Box::new(m20250201_000002_gcode_cache::Migration),
        ]
    }
}
