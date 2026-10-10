use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        // Keep the recorded migration ID stable for databases migrated before this rename.
        "m0003_user_requirement_preferences"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TABLE IF NOT EXISTS user_requirement_preferences (
                    user_id TEXT PRIMARY KEY
                        REFERENCES users(id) ON DELETE CASCADE,
                    default_view TEXT NOT NULL DEFAULT 'board'
                        CHECK (default_view IN ('board', 'list')),
                    list_status TEXT
                        CHECK (list_status IS NULL OR list_status IN ('draft', 'discussing', 'ready', 'accepted', 'rejected')),
                    list_created_by TEXT
                        CHECK (list_created_by IS NULL OR CHAR_LENGTH(BTRIM(list_created_by)) BETWEEN 1 AND 256),
                    list_sort TEXT NOT NULL DEFAULT 'updated'
                        CHECK (list_sort IN ('updated', 'updated_asc')),
                    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
                )",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Personal preferences are durable user data; rolling back code must not delete them.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_name_preserves_existing_history() {
        assert_eq!(Migration.name(), "m0003_user_requirement_preferences");
    }
}
