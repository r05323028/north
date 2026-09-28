use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TABLE requirement_board_positions (
                    requirement_id TEXT PRIMARY KEY
                        REFERENCES requirements(id) ON DELETE CASCADE,
                    rank BIGINT NOT NULL
                )",
            )
            .await?;
        manager
            .get_connection()
            .execute_unprepared(
                "INSERT INTO requirement_board_positions (requirement_id, rank)
                 SELECT id,
                        ROW_NUMBER() OVER (
                            PARTITION BY status ORDER BY created_at ASC, id ASC
                        ) * 1024
                 FROM requirements",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE requirement_board_positions")
            .await?;
        Ok(())
    }
}
