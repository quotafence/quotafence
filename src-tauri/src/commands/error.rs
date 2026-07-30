use serde::Serialize;

use crate::{application::ApplicationError, storage::StorageError};

pub type IpcResult<T> = Result<T, IpcError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcError {
    pub code: &'static str,
    pub message: String,
}

impl IpcError {
    pub fn service_unavailable() -> Self {
        Self {
            code: "service_unavailable",
            message: "The local quota service is unavailable.".to_owned(),
        }
    }

    pub fn invalid_workspace(message: impl Into<String>) -> Self {
        Self::new("invalid_workspace", message)
    }

    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<ApplicationError> for IpcError {
    fn from(error: ApplicationError) -> Self {
        match error {
            ApplicationError::Validation(error) => Self::new("validation_error", error.to_string()),
            ApplicationError::Storage(error) => error.into(),
            ApplicationError::NotFound { resource, id } => {
                Self::new("not_found", format!("{resource} {id} was not found"))
            }
            ApplicationError::InconsistentData { message } => {
                Self::new("inconsistent_data", message)
            }
            ApplicationError::InvalidRequest { message } => Self::new("validation_error", message),
            ApplicationError::NumericOutOfRange { field, value } => Self::new(
                "numeric_out_of_range",
                format!("{field} value {value} cannot be represented"),
            ),
        }
    }
}

impl From<StorageError> for IpcError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::Database(error) => database_error(error),
            StorageError::Domain(error) => {
                Self::new("invalid_stored_data", format!("Stored data is invalid: {error}"))
            }
            StorageError::NotFound { entity, id } => {
                Self::new("not_found", format!("{entity} {id} was not found"))
            }
            StorageError::InvalidState { message } => Self::new("inconsistent_data", message),
            StorageError::DuplicateSource { .. } => Self::new(
                "duplicate_source",
                "This provider quota source is already active. Remove the existing source before adding it again.",
            ),
            StorageError::NumericOutOfRange { field, value } => Self::new(
                "numeric_out_of_range",
                format!("{field} value {value} cannot be stored"),
            ),
            StorageError::InsufficientCapacity {
                scope_id,
                requested,
                available,
                unit,
            } => Self::new(
                "insufficient_capacity",
                format!(
                    "Requested {requested} {unit}, but scope {scope_id} only has {available} {unit} available"
                ),
            ),
            StorageError::UnsupportedSchemaVersion { current, supported } => Self::new(
                "incompatible_database",
                format!(
                    "Database schema version {current} is newer than supported version {supported}"
                ),
            ),
        }
    }
}

fn database_error(error: rusqlite::Error) -> IpcError {
    match error {
        rusqlite::Error::SqliteFailure(sqlite_error, _)
            if sqlite_error.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            IpcError::new(
                "conflict",
                "The request conflicts with an existing record or relationship.",
            )
        }
        _ => IpcError::new(
            "storage_unavailable",
            "The local quota database operation failed.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_errors_have_a_stable_serialized_shape() {
        let error = IpcError::from(ApplicationError::Validation(
            crate::domain::ProviderId::new(" ").unwrap_err(),
        ));

        assert_eq!(error.code, "validation_error");
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "code": "validation_error",
                "message": "provider ID cannot be empty"
            })
        );
    }

    #[test]
    fn capacity_errors_keep_actionable_details() {
        let error = IpcError::from(StorageError::InsufficientCapacity {
            scope_id: "project-a".to_owned(),
            requested: 20,
            available: 10,
            unit: "quota_points".to_owned(),
        });

        assert_eq!(error.code, "insufficient_capacity");
        assert!(error.message.contains("project-a"));
        assert!(error.message.contains("20"));
        assert!(error.message.contains("10"));
    }
}
