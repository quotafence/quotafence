use std::fmt;

use crate::domain::DomainError;

pub type StorageResult<T> = Result<T, StorageError>;

#[derive(Debug)]
pub enum StorageError {
    Database(rusqlite::Error),
    Domain(DomainError),
    NotFound {
        entity: &'static str,
        id: String,
    },
    InvalidState {
        message: String,
    },
    NumericOutOfRange {
        field: &'static str,
        value: u64,
    },
    InsufficientCapacity {
        scope_id: String,
        requested: u64,
        available: u64,
        unit: String,
    },
    UnsupportedSchemaVersion {
        current: i64,
        supported: i64,
    },
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "database error: {error}"),
            Self::Domain(error) => write!(formatter, "invalid domain data: {error}"),
            Self::NotFound { entity, id } => write!(formatter, "{entity} {id} was not found"),
            Self::InvalidState { message } => formatter.write_str(message),
            Self::NumericOutOfRange { field, value } => {
                write!(
                    formatter,
                    "{field} value {value} cannot be stored in SQLite"
                )
            }
            Self::InsufficientCapacity {
                scope_id,
                requested,
                available,
                unit,
            } => write!(
                formatter,
                "requested {requested} {unit}, but scope {scope_id} only has {available} {unit} available"
            ),
            Self::UnsupportedSchemaVersion { current, supported } => write!(
                formatter,
                "database schema version {current} is newer than supported version {supported}"
            ),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Domain(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

impl From<DomainError> for StorageError {
    fn from(error: DomainError) -> Self {
        Self::Domain(error)
    }
}

pub(crate) fn to_sql_integer(value: u64, field: &'static str) -> StorageResult<i64> {
    i64::try_from(value).map_err(|_| StorageError::NumericOutOfRange { field, value })
}

pub(crate) fn from_sql_integer(value: i64, field: &'static str) -> StorageResult<u64> {
    u64::try_from(value).map_err(|_| StorageError::InvalidState {
        message: format!("database contains a negative {field}: {value}"),
    })
}
