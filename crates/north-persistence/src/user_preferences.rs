use crate::{AuthStore, PersistenceError, RequirementSort};
use north_domain::status::RequirementStatus;
use sea_orm::FromQueryResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RequirementWorkspaceView {
    #[default]
    Board,
    List,
}

impl RequirementWorkspaceView {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Board => "board",
            Self::List => "list",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequirementWorkspacePreferences {
    pub default_view: RequirementWorkspaceView,
    pub list_status: Option<RequirementStatus>,
    pub list_created_by: Option<String>,
    pub list_sort: RequirementSort,
}

#[derive(FromQueryResult)]
struct PreferenceRow {
    default_view: String,
    list_status: Option<String>,
    list_created_by: Option<String>,
    list_sort: String,
}

impl PreferenceRow {
    fn into_preferences(self) -> Result<RequirementWorkspacePreferences, PersistenceError> {
        let default_view = match self.default_view.as_str() {
            "board" => RequirementWorkspaceView::Board,
            "list" => RequirementWorkspaceView::List,
            _ => return Err(PersistenceError::InvalidUserPreferences),
        };
        let list_status = self
            .list_status
            .map(|status| match status.as_str() {
                "draft" => Ok(RequirementStatus::Draft),
                "discussing" => Ok(RequirementStatus::Discussing),
                "ready" => Ok(RequirementStatus::Ready),
                "accepted" => Ok(RequirementStatus::Accepted),
                "rejected" => Ok(RequirementStatus::Rejected),
                _ => Err(PersistenceError::InvalidUserPreferences),
            })
            .transpose()?;
        let list_sort = match self.list_sort.as_str() {
            "updated" => RequirementSort::UpdatedDescending,
            "updated_asc" => RequirementSort::UpdatedAscending,
            _ => return Err(PersistenceError::InvalidUserPreferences),
        };

        Ok(RequirementWorkspacePreferences {
            default_view,
            list_status,
            list_created_by: self.list_created_by,
            list_sort,
        })
    }
}

impl AuthStore {
    pub async fn get_requirement_workspace_preferences(
        &self,
        user_id: &str,
    ) -> Result<RequirementWorkspacePreferences, PersistenceError> {
        let row = crate::query::query_as::<PreferenceRow>(
            "SELECT default_view, list_status, list_created_by, list_sort
             FROM user_requirement_preferences
             WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(PreferenceRow::into_preferences)
            .transpose()
            .map(|preferences| preferences.unwrap_or_default())
    }

    pub async fn save_requirement_workspace_preferences(
        &self,
        user_id: &str,
        preferences: &RequirementWorkspacePreferences,
    ) -> Result<(), PersistenceError> {
        let list_sort = match preferences.list_sort {
            RequirementSort::UpdatedDescending => "updated",
            RequirementSort::UpdatedAscending => "updated_asc",
            RequirementSort::Board => return Err(PersistenceError::InvalidUserPreferences),
        };
        let list_status = preferences
            .list_status
            .map(|status| status.as_str().to_owned());
        let list_created_by = match preferences.list_created_by.as_deref() {
            Some(value) => {
                let value = value.trim();
                if value.is_empty() || value.len() > 256 {
                    return Err(PersistenceError::InvalidUserPreferences);
                }
                Some(value.to_owned())
            }
            None => None,
        };

        crate::query::query(
            "INSERT INTO user_requirement_preferences
                (user_id, default_view, list_status, list_created_by, list_sort)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (user_id) DO UPDATE SET
                default_view = EXCLUDED.default_view,
                list_status = EXCLUDED.list_status,
                list_created_by = EXCLUDED.list_created_by,
                list_sort = EXCLUDED.list_sort,
                updated_at = CURRENT_TIMESTAMP",
        )
        .bind(user_id)
        .bind(preferences.default_view.as_str())
        .bind(list_status)
        .bind(list_created_by)
        .bind(list_sort)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
