use std::collections::HashSet;

use rusqlite::{params, OptionalExtension, Transaction};

use crate::{
    domain::{
        Confidence, QuotaAmount, QuotaPoolId, QuotaUnit, ScopeId, UnixMillis, UsageAttribution,
        UsageEvent, UsageEventId, UsageSource, WindowId,
    },
    workspace::{contains_path, path_depth},
};

use super::{
    error::{from_sql_integer, to_sql_integer},
    ledger::insert_usage_event,
    ProviderQuotaSnapshot, StorageResult,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopThreadObservation {
    pub thread_id: String,
    pub canonical_path: String,
    pub total_tokens: u64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopReconciliationStatus {
    BaselineEstablished,
    NoActivity,
    PendingProviderDelta,
    Attributed,
    Ambiguous,
    WindowRolledOver,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopReconciliation {
    pub status: DesktopReconciliationStatus,
    pub observed_threads: u32,
    pub pending_tokens: u64,
    pub attributed_amount: u64,
    pub scope_id: Option<ScopeId>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn reconcile(
    transaction: &Transaction<'_>,
    pool_id: &QuotaPoolId,
    current_window_id: &WindowId,
    target_window_id: &WindowId,
    unit: &QuotaUnit,
    previous_snapshot: Option<&ProviderQuotaSnapshot>,
    snapshot: &ProviderQuotaSnapshot,
    observations: &[DesktopThreadObservation],
) -> StorageResult<DesktopReconciliation> {
    let existing_cursor_count: i64 = transaction.query_row(
        "SELECT COUNT(*)
         FROM codex_desktop_thread_cursors
         WHERE pool_id = ?1",
        [pool_id.as_str()],
        |row| row.get(0),
    )?;
    let has_baseline = existing_cursor_count > 0;
    let previous_observed_at = previous_snapshot.map(|value| value.observed_at().value());

    for observation in observations {
        let existing: Option<(i64, i64)> = transaction
            .query_row(
                "SELECT last_tokens, pending_tokens
                 FROM codex_desktop_thread_cursors
                 WHERE pool_id = ?1 AND thread_id = ?2",
                params![pool_id.as_str(), observation.thread_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let current_tokens = to_sql_integer(observation.total_tokens, "desktop thread tokens")?;
        let pending_tokens = match existing {
            Some((last_tokens, pending_tokens))
                if previous_observed_at
                    .is_some_and(|baseline| observation.updated_at > baseline) =>
            {
                let delta = current_tokens.saturating_sub(last_tokens);
                pending_tokens.saturating_add(delta)
            }
            Some((_, pending_tokens)) => pending_tokens,
            None if has_baseline
                && previous_observed_at
                    .is_some_and(|baseline| observation.updated_at > baseline) =>
            {
                current_tokens
            }
            None => 0,
        };

        transaction.execute(
            "INSERT INTO codex_desktop_thread_cursors (
                 pool_id, thread_id, canonical_path, last_tokens,
                 pending_tokens, observed_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(pool_id, thread_id) DO UPDATE SET
                 canonical_path = excluded.canonical_path,
                 last_tokens = excluded.last_tokens,
                 pending_tokens = excluded.pending_tokens,
                 observed_at = excluded.observed_at",
            params![
                pool_id.as_str(),
                observation.thread_id,
                observation.canonical_path,
                current_tokens,
                pending_tokens,
                observation.updated_at,
            ],
        )?;
    }

    let observed_threads = u32::try_from(observations.len()).unwrap_or(u32::MAX);
    if previous_snapshot.is_none() || !has_baseline {
        clear_pending(transaction, pool_id)?;
        return Ok(DesktopReconciliation {
            status: DesktopReconciliationStatus::BaselineEstablished,
            observed_threads,
            pending_tokens: 0,
            attributed_amount: 0,
            scope_id: None,
        });
    }
    if target_window_id != current_window_id {
        clear_pending(transaction, pool_id)?;
        return Ok(DesktopReconciliation {
            status: DesktopReconciliationStatus::WindowRolledOver,
            observed_threads,
            pending_tokens: 0,
            attributed_amount: 0,
            scope_id: None,
        });
    }

    let pending = pending_activity(transaction, pool_id)?;
    let pending_tokens = pending
        .iter()
        .fold(0_u64, |total, (_, tokens)| total.saturating_add(*tokens));
    if pending_tokens == 0 {
        return Ok(DesktopReconciliation {
            status: DesktopReconciliationStatus::NoActivity,
            observed_threads,
            pending_tokens: 0,
            attributed_amount: 0,
            scope_id: None,
        });
    }

    let previous_used = previous_snapshot.map_or(0, ProviderQuotaSnapshot::used);
    let provider_delta = snapshot.used().saturating_sub(previous_used);
    if provider_delta == 0 {
        return Ok(DesktopReconciliation {
            status: DesktopReconciliationStatus::PendingProviderDelta,
            observed_threads,
            pending_tokens,
            attributed_amount: 0,
            scope_id: None,
        });
    }

    let bindings = load_bindings(transaction)?;
    let mut scopes = HashSet::new();
    let mut has_unmapped_activity = false;
    for (canonical_path, _) in &pending {
        let scope = bindings
            .iter()
            .filter(|(binding_path, _)| contains_path(binding_path, canonical_path))
            .max_by_key(|(binding_path, _)| path_depth(binding_path))
            .map(|(_, scope_id)| scope_id.clone());
        if let Some(scope) = scope {
            scopes.insert(scope);
        } else {
            has_unmapped_activity = true;
        }
    }

    let scope_id = if !has_unmapped_activity && scopes.len() == 1 {
        scopes.into_iter().next()
    } else {
        None
    };
    if let Some(scope_id) = scope_id {
        let event = UsageEvent::new(
            UsageEventId::new(format!(
                "codex-desktop:{}:{}",
                target_window_id,
                snapshot.observed_at().value()
            ))?,
            target_window_id.clone(),
            UsageAttribution::Scope(scope_id.clone()),
            QuotaAmount::new(provider_delta, unit.clone()),
            UnixMillis::new(snapshot.observed_at().value()),
            UsageSource::ProviderObserved,
            Confidence::Inferred,
        )?;
        insert_usage_event(transaction, &event)?;
        clear_pending(transaction, pool_id)?;
        return Ok(DesktopReconciliation {
            status: DesktopReconciliationStatus::Attributed,
            observed_threads,
            pending_tokens,
            attributed_amount: provider_delta,
            scope_id: Some(scope_id),
        });
    }

    clear_pending(transaction, pool_id)?;
    Ok(DesktopReconciliation {
        status: DesktopReconciliationStatus::Ambiguous,
        observed_threads,
        pending_tokens,
        attributed_amount: 0,
        scope_id: None,
    })
}

fn pending_activity(
    transaction: &Transaction<'_>,
    pool_id: &QuotaPoolId,
) -> StorageResult<Vec<(String, u64)>> {
    let mut statement = transaction.prepare(
        "SELECT canonical_path, pending_tokens
         FROM codex_desktop_thread_cursors
         WHERE pool_id = ?1 AND pending_tokens > 0",
    )?;
    let rows = statement.query_map([pool_id.as_str()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    rows.map(|row| {
        let (path, tokens) = row?;
        Ok((path, from_sql_integer(tokens, "pending desktop tokens")?))
    })
    .collect()
}

fn load_bindings(transaction: &Transaction<'_>) -> StorageResult<Vec<(String, ScopeId)>> {
    let mut statement =
        transaction.prepare("SELECT canonical_path, scope_id FROM workspace_bindings")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.map(|row| {
        let (path, scope_id) = row?;
        Ok((path, ScopeId::new(scope_id)?))
    })
    .collect()
}

fn clear_pending(transaction: &Transaction<'_>, pool_id: &QuotaPoolId) -> StorageResult<()> {
    transaction.execute(
        "UPDATE codex_desktop_thread_cursors
         SET pending_tokens = 0
         WHERE pool_id = ?1 AND pending_tokens > 0",
        [pool_id.as_str()],
    )?;
    Ok(())
}

pub(crate) fn discard_pending(
    transaction: &Transaction<'_>,
    pool_id: &QuotaPoolId,
) -> StorageResult<()> {
    clear_pending(transaction, pool_id)
}
