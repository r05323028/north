use crate::{AuthStore, PersistenceError};
use north_domain::repository::{validate_metadata, RepositoryError, RepositoryMetadata};
use rand::{rng, Rng};
use sqlx::FromRow;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryRecord {
    pub id: String,
    pub name: String,
    pub name_normalized: String,
    pub url: String,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
    pub disabled_at: Option<String>,
}

impl RepositoryRecord {
    pub fn enabled(&self) -> bool {
        self.disabled_at.is_none()
    }
}

impl From<RepositoryRow> for RepositoryRecord {
    fn from(row: RepositoryRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            name_normalized: row.name_normalized,
            url: row.url,
            description: row.description,
            created_at: row.created_at,
            updated_at: row.updated_at,
            disabled_at: row.disabled_at,
        }
    }
}

#[derive(Debug, FromRow)]
struct RepositoryRow {
    id: String,
    name: String,
    name_normalized: String,
    url: String,
    description: String,
    created_at: String,
    updated_at: String,
    disabled_at: Option<String>,
}

fn repository_row_select() -> &'static str {
    "SELECT id, name, name_normalized, url, description,
            created_at::text AS created_at, updated_at::text AS updated_at,
            disabled_at::text AS disabled_at
     FROM repositories"
}

pub fn repository_metadata(
    name: &str,
    url: &str,
    description: &str,
) -> Result<RepositoryMetadata, RepositoryError> {
    validate_metadata(name, url, description)
}

impl AuthStore {
    pub async fn create_repository(
        &self,
        name: &str,
        url: &str,
        description: &str,
    ) -> Result<RepositoryRecord, PersistenceError> {
        let metadata = repository_metadata(name, url, description)
            .map_err(PersistenceError::InvalidRepository)?;
        let id = repository_id();
        let result = sqlx::query_as::<_, RepositoryRow>(
            "INSERT INTO repositories (id, name, name_normalized, url, description)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id, name, name_normalized, url, description,
                       created_at::text AS created_at, updated_at::text AS updated_at,
                       disabled_at::text AS disabled_at",
        )
        .bind(id)
        .bind(&metadata.name)
        .bind(&metadata.name_normalized)
        .bind(&metadata.url)
        .bind(&metadata.description)
        .fetch_one(&self.pool)
        .await;
        result
            .map(Into::into)
            .map_err(map_repository_database_error)
    }

    pub async fn repository_by_normalized_name(
        &self,
        name: &str,
    ) -> Result<Option<RepositoryRecord>, PersistenceError> {
        let normalized = name.trim().to_lowercase();
        let query = format!("{} WHERE name_normalized = $1", repository_row_select());
        sqlx::query_as::<_, RepositoryRow>(&query)
            .bind(normalized)
            .fetch_optional(&self.pool)
            .await
            .map(|row| row.map(Into::into))
            .map_err(Into::into)
    }

    pub async fn repository_by_id(
        &self,
        repository_id: &str,
    ) -> Result<Option<RepositoryRecord>, PersistenceError> {
        let query = format!("{} WHERE id = $1", repository_row_select());
        sqlx::query_as::<_, RepositoryRow>(&query)
            .bind(repository_id)
            .fetch_optional(&self.pool)
            .await
            .map(|row| row.map(Into::into))
            .map_err(Into::into)
    }

    /// Complete Admin/Owner management list, including disabled rows.
    pub async fn list_repositories(&self) -> Result<Vec<RepositoryRecord>, PersistenceError> {
        let query = format!(
            "{} ORDER BY name_normalized ASC, id ASC",
            repository_row_select()
        );
        sqlx::query_as::<_, RepositoryRow>(&query)
            .fetch_all(&self.pool)
            .await
            .map(|rows| rows.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    /// Internal enabled-only catalog for server-assembled runtime context.
    pub async fn active_repositories(&self) -> Result<Vec<RepositoryRecord>, PersistenceError> {
        let query = format!(
            "{} WHERE disabled_at IS NULL ORDER BY name_normalized ASC, id ASC",
            repository_row_select()
        );
        sqlx::query_as::<_, RepositoryRow>(&query)
            .fetch_all(&self.pool)
            .await
            .map(|rows| rows.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub async fn update_repository_metadata(
        &self,
        repository_id: &str,
        name: &str,
        description: &str,
        url: Option<&str>,
    ) -> Result<RepositoryRecord, PersistenceError> {
        self.update_repository_fields(repository_id, Some(name), Some(description), url)
            .await
    }

    pub async fn update_repository_fields(
        &self,
        repository_id: &str,
        name: Option<&str>,
        description: Option<&str>,
        url: Option<&str>,
    ) -> Result<RepositoryRecord, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let query = format!("{} WHERE id = $1 FOR UPDATE", repository_row_select());
        let existing = sqlx::query_as::<_, RepositoryRow>(&query)
            .bind(repository_id)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(PersistenceError::RepositoryNotFound)?;
        if let Some(url) = url {
            if url.trim() != existing.url {
                return Err(PersistenceError::RepositoryUrlImmutable);
            }
        }
        let name = name.unwrap_or(existing.name.as_str());
        let description = description.unwrap_or(existing.description.as_str());
        let metadata = repository_metadata(name, existing.url.as_str(), description)
            .map_err(PersistenceError::InvalidRepository)?;
        let changed =
            existing.name != metadata.name || existing.description != metadata.description;
        if !changed {
            transaction.commit().await?;
            return Ok(existing.into());
        }
        let result = sqlx::query_as::<_, RepositoryRow>(
            "UPDATE repositories
             SET name = $2, name_normalized = $3, description = $4,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = $1
             RETURNING id, name, name_normalized, url, description,
                       created_at::text AS created_at, updated_at::text AS updated_at,
                       disabled_at::text AS disabled_at",
        )
        .bind(repository_id)
        .bind(&metadata.name)
        .bind(&metadata.name_normalized)
        .bind(&metadata.description)
        .fetch_one(&mut *transaction)
        .await
        .map_err(map_repository_database_error)?;
        transaction.commit().await?;
        Ok(result.into())
    }

    pub async fn disable_repository(
        &self,
        repository_id: &str,
    ) -> Result<RepositoryRecord, PersistenceError> {
        lifecycle_repository(&self.pool, repository_id, false).await
    }

    pub async fn reenable_repository(
        &self,
        repository_id: &str,
    ) -> Result<RepositoryRecord, PersistenceError> {
        lifecycle_repository(&self.pool, repository_id, true).await
    }

    pub async fn repository_exists(&self, repository_id: &str) -> Result<bool, PersistenceError> {
        Ok(
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM repositories WHERE id = $1)")
                .bind(repository_id)
                .fetch_one(&self.pool)
                .await?,
        )
    }
}

async fn lifecycle_repository(
    pool: &sqlx::PgPool,
    repository_id: &str,
    enable: bool,
) -> Result<RepositoryRecord, PersistenceError> {
    let mut transaction = pool.begin().await?;
    let query = format!("{} WHERE id = $1 FOR UPDATE", repository_row_select());
    let existing = sqlx::query_as::<_, RepositoryRow>(&query)
        .bind(repository_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(PersistenceError::RepositoryNotFound)?;
    let already_in_target_state = existing.disabled_at.is_none() == enable;
    if already_in_target_state {
        transaction.commit().await?;
        return Ok(existing.into());
    }
    let query = if enable {
        "UPDATE repositories
         SET disabled_at = NULL, updated_at = CURRENT_TIMESTAMP
         WHERE id = $1
         RETURNING id, name, name_normalized, url, description,
                   created_at::text AS created_at, updated_at::text AS updated_at,
                   disabled_at::text AS disabled_at"
    } else {
        "UPDATE repositories
         SET disabled_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
         WHERE id = $1
         RETURNING id, name, name_normalized, url, description,
                   created_at::text AS created_at, updated_at::text AS updated_at,
                   disabled_at::text AS disabled_at"
    };
    let updated = sqlx::query_as::<_, RepositoryRow>(query)
        .bind(repository_id)
        .fetch_one(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(updated.into())
}

fn map_repository_database_error(error: sqlx::Error) -> PersistenceError {
    if error
        .as_database_error()
        .and_then(|database| database.code())
        .is_some_and(|code| code == "23505")
    {
        PersistenceError::RepositoryNameConflict
    } else {
        PersistenceError::Database(error)
    }
}

fn repository_id() -> String {
    let mut bytes = [0_u8; 16];
    rng().fill(&mut bytes);
    // RFC 4122 version 4 shape: the ID remains opaque to the domain.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_repository_id_has_uuid_shape() {
        let id = repository_id();
        assert_eq!(id.len(), 36);
        assert_eq!(id.as_bytes()[8], b'-');
        assert_eq!(id.as_bytes()[13], b'-');
        assert_eq!(id.as_bytes()[18], b'-');
        assert_eq!(id.as_bytes()[23], b'-');
    }
}
