//! Migration — adds the `gcode_cache` table.
//!
//! Maps a content-derived cache key (hash of the resolved slicing parameters +
//! the placed scene + engine version) to a previously-generated G-code file so
//! an identical scene is never re-sliced. Purely a performance cache: rows may
//! be evicted at any time and the referenced file re-created on the next slice.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20250201_000002_gcode_cache"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(GcodeCache::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(GcodeCache::CacheKey)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(GcodeCache::FilePath).string().not_null())
                    .col(
                        ColumnDef::new(GcodeCache::FileSize)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(GcodeCache::LayerCount)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(GcodeCache::CreatedAt).string().not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_gcode_cache_created_at")
                    .table(GcodeCache::Table)
                    .col(GcodeCache::CreatedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(GcodeCache::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum GcodeCache {
    Table,
    CacheKey,
    FilePath,
    FileSize,
    LayerCount,
    CreatedAt,
}
