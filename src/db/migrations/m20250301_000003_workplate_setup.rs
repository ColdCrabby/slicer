//! Migration — adds `requests.setup`, the saved form of a workplate.
//!
//! A plate's scene is ephemeral per WebSocket connection, so without this the
//! only record of where the user put things — and which printer, filament and
//! process they set the plate up with — lived in one browser's `localStorage`.
//! That is the same failure the profile library exists to prevent: a cloud user
//! who clears their browser should not lose their work.
//!
//! Nullable, and holding a whole JSON document rather than columns, because the
//! document is [`WorkplateSetup`](crate::workplate::WorkplateSetup)'s business:
//! it is written and read whole, never queried into.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20250301_000003_workplate_setup"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Requests::Table)
                    .add_column(ColumnDef::new(Requests::Setup).text().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Requests::Table)
                    .drop_column(Requests::Setup)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Requests {
    Table,
    Setup,
}
