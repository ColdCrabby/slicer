//! SeaORM entity for the `gcode_cache` table.

use sea_orm::entity::prelude::*;

/// ORM model for a row in the `gcode_cache` table.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "gcode_cache")]
pub struct Model {
    /// Content-derived cache key (hex hash of params + scene + engine version).
    #[sea_orm(primary_key, auto_increment = false)]
    pub cache_key: String,
    /// Absolute path to the cached G-code file.
    pub file_path: String,
    /// Byte size of the cached G-code file.
    pub file_size: i64,
    /// Layer count of the cached slice.
    pub layer_count: i64,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
