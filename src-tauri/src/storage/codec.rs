use crate::domain::{Confidence, ReservationStatus, ScopeKind, UsageSource};

use super::{StorageError, StorageResult};

pub(crate) fn scope_kind_to_str(kind: ScopeKind) -> &'static str {
    match kind {
        ScopeKind::Project => "project",
        ScopeKind::Repository => "repository",
        ScopeKind::Task => "task",
        ScopeKind::Reserve => "reserve",
    }
}

pub(crate) fn scope_kind_from_str(value: &str) -> StorageResult<ScopeKind> {
    match value {
        "project" => Ok(ScopeKind::Project),
        "repository" => Ok(ScopeKind::Repository),
        "task" => Ok(ScopeKind::Task),
        "reserve" => Ok(ScopeKind::Reserve),
        _ => Err(invalid_enum("scope kind", value)),
    }
}

pub(crate) fn reservation_status_to_str(status: ReservationStatus) -> &'static str {
    match status {
        ReservationStatus::Active => "active",
        ReservationStatus::Released => "released",
        ReservationStatus::Consumed => "consumed",
        ReservationStatus::Expired => "expired",
    }
}

pub(crate) fn reservation_status_from_str(value: &str) -> StorageResult<ReservationStatus> {
    match value {
        "active" => Ok(ReservationStatus::Active),
        "released" => Ok(ReservationStatus::Released),
        "consumed" => Ok(ReservationStatus::Consumed),
        "expired" => Ok(ReservationStatus::Expired),
        _ => Err(invalid_enum("reservation status", value)),
    }
}

pub(crate) fn usage_source_to_str(source: UsageSource) -> &'static str {
    match source {
        UsageSource::ProviderConfirmed => "provider_confirmed",
        UsageSource::ProviderObserved => "provider_observed",
        UsageSource::LocalMeasured => "local_measured",
        UsageSource::Estimated => "estimated",
    }
}

pub(crate) fn usage_source_from_str(value: &str) -> StorageResult<UsageSource> {
    match value {
        "provider_confirmed" => Ok(UsageSource::ProviderConfirmed),
        "provider_observed" => Ok(UsageSource::ProviderObserved),
        "local_measured" => Ok(UsageSource::LocalMeasured),
        "estimated" => Ok(UsageSource::Estimated),
        _ => Err(invalid_enum("usage source", value)),
    }
}

pub(crate) fn confidence_to_str(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::Confirmed => "confirmed",
        Confidence::Observed => "observed",
        Confidence::Inferred => "inferred",
        Confidence::Estimated => "estimated",
    }
}

pub(crate) fn confidence_from_str(value: &str) -> StorageResult<Confidence> {
    match value {
        "confirmed" => Ok(Confidence::Confirmed),
        "observed" => Ok(Confidence::Observed),
        "inferred" => Ok(Confidence::Inferred),
        "estimated" => Ok(Confidence::Estimated),
        _ => Err(invalid_enum("confidence", value)),
    }
}

fn invalid_enum(field: &'static str, value: &str) -> StorageError {
    StorageError::InvalidState {
        message: format!("database contains an unknown {field}: {value}"),
    }
}
