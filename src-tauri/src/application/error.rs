use std::fmt;

use crate::{domain::DomainError, storage::StorageError};

pub type ApplicationResult<T> = Result<T, ApplicationError>;

#[derive(Debug)]
pub enum ApplicationError {
    Validation(DomainError),
    Storage(StorageError),
    NotFound { resource: &'static str, id: String },
    InconsistentData { message: String },
    NumericOutOfRange { field: &'static str, value: i128 },
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => write!(formatter, "validation failed: {error}"),
            Self::Storage(error) => write!(formatter, "storage operation failed: {error}"),
            Self::NotFound { resource, id } => write!(formatter, "{resource} {id} was not found"),
            Self::InconsistentData { message } => formatter.write_str(message),
            Self::NumericOutOfRange { field, value } => {
                write!(formatter, "{field} value {value} cannot be represented")
            }
        }
    }
}

impl std::error::Error for ApplicationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Validation(error) => Some(error),
            Self::Storage(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DomainError> for ApplicationError {
    fn from(error: DomainError) -> Self {
        Self::Validation(error)
    }
}

impl From<StorageError> for ApplicationError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

pub(crate) fn to_view_integer(value: i128, field: &'static str) -> ApplicationResult<i64> {
    i64::try_from(value).map_err(|_| ApplicationError::NumericOutOfRange { field, value })
}
