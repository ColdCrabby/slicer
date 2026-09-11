//! SeaORM entity for the `requests` table.

use sea_orm::entity::prelude::*;

/// ORM model for a row in the `requests` table.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "requests")]
pub struct Model {
    /// UUID stored as TEXT (RFC 4122 hyphenated string).
    #[sea_orm(primary_key, auto_increment = false)]
    pub request_uuid: String,
    /// Lifecycle status string (see [`crate::db::RequestStatus`]).
    pub status: String,
    /// Absolute path to the generated G-code file, if slicing is complete.
    pub download_file_path: Option<String>,
    /// Byte size of the generated G-code file.
    pub download_file_size: Option<i64>,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
    /// RFC 3339 last-update timestamp.
    pub updated_at: String,
    /// The saved workplate as JSON — presets, override diff, object placements.
    /// `None` for a plate the user never configured. See
    /// [`crate::workplate::WorkplateSetup`].
    pub setup: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::files::Entity")]
    Files,
}

impl Related<super::files::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Files.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
