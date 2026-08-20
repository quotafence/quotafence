use std::{path::Path, sync::Mutex};

use crate::application::{ApplicationResult, QuotaService};

use super::{IpcError, IpcResult};

pub(crate) struct AppState {
    service: Mutex<QuotaService>,
}

impl AppState {
    pub(crate) fn open(database_path: impl AsRef<Path>) -> ApplicationResult<Self> {
        Ok(Self::new(QuotaService::open(database_path)?))
    }

    pub(crate) fn new(service: QuotaService) -> Self {
        Self {
            service: Mutex::new(service),
        }
    }

    pub(crate) fn execute<T>(
        &self,
        operation: impl FnOnce(&mut QuotaService) -> ApplicationResult<T>,
    ) -> IpcResult<T> {
        let mut service = self
            .service
            .lock()
            .map_err(|_| IpcError::service_unavailable())?;

        operation(&mut service).map_err(Into::into)
    }

    pub(crate) fn execute_integration<T>(
        &self,
        operation: impl FnOnce(&mut QuotaService) -> Result<T, String>,
    ) -> IpcResult<T> {
        let mut service = self
            .service
            .lock()
            .map_err(|_| IpcError::service_unavailable())?;
        operation(&mut service).map_err(IpcError::integration_error)
    }
}
