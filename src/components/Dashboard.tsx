import { useEffect, useState } from "react";
import type {
  AllocationSnapshot,
  DepletionForecast,
  LocalState,
  QuotaSourceSummary,
  ScopeSummary,
} from "../types";
import { Icon } from "./Icon";

type DashboardProps = {
  state: LocalState;
  refreshing: boolean;
  onSelectSource: (windowId: string) => void;
  onAddSource: () => void;
  onAddScope: () => void;
  onEditAllocation: (scope: ScopeSummary) => void;
  onRefresh: () => void;
  onRemoveSource: (source: QuotaSourceSummary) => void;
  removingSource: boolean;
};

function labelUnit(unit: string): string {
  return unit.split("_").join(" ");
}

function formatAmount(value: number, unit: string): string {
  if (unit === "percent") {
    return `${value}%`;
  }

  return `${value.toLocaleString()} ${labelUnit(unit)}`;
}

function formatReset(endsAt: number): string {
  const remaining = endsAt - Date.now();
  if (remaining <= 0) {
    return "Window ended";
  }

  const hours = Math.ceil(remaining / 3_600_000);
  if (hours < 48) {
    return `${hours}h remaining`;
  }

  return `${Math.ceil(hours / 24)}d remaining`;
}

function dateRange(source: QuotaSourceSummary): string {
  const formatter = new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
  });
  return `${formatter.format(source.startsAt)} – ${formatter.format(source.endsAt)}`;
}

function formatLastSync(timestamp: number | null): string {
  if (timestamp === null) {
    return "Local estimate";
  }

  const seconds = Math.max(0, Math.round((Date.now() - timestamp) / 1_000));
  if (seconds < 60) {
    return "Synced just now";
  }
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return `Synced ${minutes}m ago`;
  }
  return `Synced ${Math.floor(minutes / 60)}h ago`;
}

function formatObservationDuration(forecast: DepletionForecast): string {
  if (forecast.observationStart === null) {
    return "No observation window";
  }
  const hours = Math.max(
    0,
    Math.round(
      (forecast.observationEnd - forecast.observationStart) / 3_600_000,
    ),
  );
  if (hours < 48) {
    return `${hours}h observed`;
  }
  return `${Math.round(hours / 24)}d observed`;
}

function formatForecast(
  forecast: DepletionForecast,
  unit: string,
): { label: string; detail: string } {
  const coverage = Math.round(forecast.coverageBasisPoints / 100);
  const evidence =
    forecast.sampleCount === 1
      ? "1 managed session"
      : `${forecast.sampleCount} managed sessions`;
  if (forecast.status === "window_ended") {
    return { label: "Window ended", detail: "Waiting for provider rollover" };
  }
  if (forecast.status === "insufficient_data") {
    return {
      label: "Forecast not reliable yet",
      detail: `${evidence} · ${coverage}% coverage · ${forecast.confidence} confidence`,
    };
  }

  const rate = forecast.burnRatePerDayMilliunits ?? 0;
  const rateLabel = `${formatAmount(rate / 1_000, unit)} / day`;
  const confidence = `${forecast.confidence} confidence`;
  const detail = `${rateLabel} · ${formatObservationDuration(
    forecast,
  )} · ${confidence}`;
  if (forecast.status === "no_managed_burn") {
    return { label: "No managed burn observed", detail };
  }
  if (
    forecast.status === "depletes_before_reset" &&
    forecast.projectedDepletionAt !== null
  ) {
    const depletion = new Intl.DateTimeFormat(undefined, {
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
    }).format(forecast.projectedDepletionAt);
    return { label: `May run out around ${depletion}`, detail };
  }
  return { label: "Likely to last until reset", detail };
}

function orderedScopes(scopes: ScopeSummary[]): ScopeSummary[] {
  return scopes
    .filter(
      (scope) =>
        scope.kind === "workspace" &&
        scope.parentId === null &&
        scope.workspacePath !== null,
    )
    .sort((left, right) => left.displayName.localeCompare(right.displayName));
}

function workspaceLabel(scope: ScopeSummary): string {
  if (!scope.workspacePath) {
    return scope.kind;
  }
  const segments = scope.workspacePath.split(/[\\/]/).filter(Boolean);
  return `workspace · ${segments[segments.length - 1] ?? scope.workspacePath}`;
}

function AllocationRow({
  scope,
  allocation,
  unit,
  providerSpendable,
  onEdit,
}: {
  scope: ScopeSummary;
  allocation?: AllocationSnapshot;
  unit: string;
  providerSpendable: number;
  onEdit: () => void;
}) {
  const usableNow = allocation
    ? Math.min(allocation.spendable, providerSpendable)
    : 0;
  const remainingPercent = allocation?.limit
    ? Math.min(
        100,
        Math.round((usableNow / allocation.limit) * 100),
      )
    : 0;
  const decision = allocation?.decision ?? "allow";

  return (
    <article className="allocation-row">
      <div className="scope-identity">
        <span className="scope-icon workspace">
          <Icon name="folder" size={18} />
        </span>
        <div>
          <strong>{scope.displayName}</strong>
          <span title={scope.workspacePath ?? undefined}>
            {workspaceLabel(scope)}
          </span>
        </div>
      </div>

      <div className="allocation-progress">
        {allocation ? (
          <>
            <div className="progress-meta">
              <strong>{formatAmount(usableNow, unit)} left</strong>
              <span>of {formatAmount(allocation.limit, unit)}</span>
            </div>
            <div
              className="progress-track"
              role="progressbar"
              aria-label={`${scope.displayName} quota remaining`}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={remainingPercent}
            >
              <span
                className={`progress-fill ${decision}`}
                style={{ width: `${remainingPercent}%` }}
              />
            </div>
          </>
        ) : (
          <span className="unallocated-label">Not allocated in this window</span>
        )}
      </div>

      <div className="row-actions">
        <button className="button subtle small" type="button" onClick={onEdit}>
          {allocation ? "Adjust" : "Allocate"}
        </button>
      </div>
    </article>
  );
}

export function Dashboard({
  state,
  refreshing,
  onSelectSource,
  onAddSource,
  onAddScope,
  onEditAllocation,
  onRefresh,
  onRemoveSource,
  removingSource,
}: DashboardProps) {
  const [sourceMenu, setSourceMenu] = useState<{
    source: QuotaSourceSummary;
    x: number;
    y: number;
  } | null>(null);

  useEffect(() => {
    if (!sourceMenu) {
      return;
    }
    const close = () => setSourceMenu(null);
    window.addEventListener("pointerdown", close);
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("pointerdown", close);
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
    };
  }, [sourceMenu]);

  const dashboard = state.dashboard;
  const source = state.sources.find(
    (candidate) => candidate.windowId === state.selectedWindowId,
  );

  if (!dashboard || !source) {
    return null;
  }

  const { window: quotaWindow } = dashboard;
  const availablePercent = quotaWindow.capacity
    ? Math.min(
        100,
        Math.round(
          (quotaWindow.providerSpendable / quotaWindow.capacity) * 100,
        ),
      )
    : 0;
  const allocationByScope = new Map(
    dashboard.allocations.map((allocation) => [allocation.scopeId, allocation]),
  );
  const scopes = orderedScopes(state.scopes);
  const forecast = formatForecast(dashboard.forecast, quotaWindow.unit);

  return (
    <div className="app-layout">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark">
            <Icon name="gauge" size={22} />
          </span>
          <div>
            <strong>Agent Quota</strong>
            <span>Manager</span>
          </div>
        </div>

        <div className="sidebar-section">
          <div className="sidebar-label">
            <span>Quota sources</span>
            <button type="button" onClick={onAddSource} aria-label="Add quota source">
              <Icon name="plus" size={16} />
            </button>
          </div>
          <div className="source-list">
            {state.sources.map((item) => (
              <button
                className={`source-item ${
                  item.windowId === source.windowId ? "active" : ""
                }`}
                type="button"
                key={item.windowId}
                onClick={() => onSelectSource(item.windowId)}
                onContextMenu={(event) => {
                  event.preventDefault();
                  setSourceMenu({
                    source: item,
                    x: Math.min(event.clientX, window.innerWidth - 180),
                    y: Math.min(event.clientY, window.innerHeight - 64),
                  });
                }}
              >
                <span className="source-avatar">
                  {item.providerDisplayName.slice(0, 2).toUpperCase()}
                </span>
                <span>
                  <strong>{item.providerDisplayName}</strong>
                  <small>{item.poolDisplayName}</small>
                </span>
                {item.isActive && <i />}
              </button>
            ))}
          </div>
        </div>

        <div className="local-card">
          <Icon name="database" size={20} />
          <div>
            <strong>Local-first</strong>
            <span>Data stays on this device</span>
          </div>
          <Icon name="check" size={16} />
        </div>
      </aside>

      <main className="main-content">
        <header className="topbar">
          <div>
            <h1>{source.providerDisplayName}</h1>
            <p className="topbar-subtitle">{source.poolDisplayName}</p>
          </div>
          <div className="topbar-actions">
            <button
              className="icon-button bordered"
              type="button"
              onClick={onRefresh}
              disabled={refreshing}
              aria-label="Sync latest quota"
              title="Sync latest quota from provider"
            >
              <Icon name="refresh" size={18} className={refreshing ? "spin" : ""} />
            </button>
          </div>
        </header>

        <section className="quota-summary">
          <div className="quota-summary-main">
            <div className="status-line">
              <span className={`status-dot ${source.isActive ? "active" : ""}`} />
              {source.isActive ? "Active window" : "Inactive window"}
              <small>{formatLastSync(source.lastSyncedAt)}</small>
            </div>
            <div className="quota-amount">
              <strong>
                {formatAmount(
                  quotaWindow.providerSpendable,
                  quotaWindow.unit,
                )}
              </strong>
              <span>quota left</span>
            </div>
            <div
              className="provider-progress"
              role="progressbar"
              aria-label="Provider quota remaining"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={availablePercent}
            >
              <span style={{ width: `${availablePercent}%` }} />
            </div>
            <div className="forecast-line">
              <span>
                <Icon name="activity" size={16} />
                Managed pace
              </span>
              <strong>{forecast.label}</strong>
              <small>{forecast.detail}</small>
            </div>
          </div>

          <dl className="quota-facts">
            <div>
              <dt>
                <Icon name="calendar" size={18} />
                Resets
              </dt>
              <dd>{formatReset(quotaWindow.endsAt)}</dd>
              <small>{dateRange(source)}</small>
            </div>
            <div>
              <dt>
                <Icon name="activity" size={18} />
                Unattributed
              </dt>
              <dd>
                {formatAmount(
                  quotaWindow.unattributedUsage,
                  quotaWindow.unit,
                )}
              </dd>
              <small>Not assigned to a workspace</small>
            </div>
          </dl>
        </section>

        <section className="allocations-section">
          <header className="section-header">
            <div>
              <h2>Workspace allocations</h2>
              <span>Quota limits for local folders.</span>
            </div>
            <div className="section-actions">
              <button className="button outline" type="button" onClick={onAddScope}>
                <Icon name="plus" size={17} />
                Add workspace
              </button>
            </div>
          </header>

          <div className="allocation-table-header" aria-hidden="true">
            <span>Workspace</span>
            <span>Remaining</span>
            <span />
          </div>

          <div className="allocation-list">
            {scopes.length > 0 ? (
              scopes.map((scope) => (
                <AllocationRow
                  key={scope.id}
                  scope={scope}
                  allocation={allocationByScope.get(scope.id)}
                  unit={quotaWindow.unit}
                  providerSpendable={quotaWindow.providerSpendable}
                  onEdit={() => onEditAllocation(scope)}
                />
              ))
            ) : (
              <div className="empty-allocations">
                <span>
                  <Icon name="spark" size={24} />
                </span>
                <div>
                  <strong>Allocate quota to your first workspace</strong>
                  <p>Choose a local folder and set its quota limit.</p>
                </div>
                <button className="button dark" type="button" onClick={onAddScope}>
                  Add workspace
                </button>
              </div>
            )}
          </div>
        </section>
      </main>
      {sourceMenu && (
        <div
          className="source-context-menu"
          role="menu"
          aria-label={`${sourceMenu.source.providerDisplayName} source actions`}
          style={{ left: sourceMenu.x, top: sourceMenu.y }}
          onPointerDown={(event) => event.stopPropagation()}
        >
          <button
            type="button"
            role="menuitem"
            disabled={removingSource}
            onClick={() => {
              onRemoveSource(sourceMenu.source);
              setSourceMenu(null);
            }}
          >
            <Icon name="trash" size={16} />
            Delete source
          </button>
        </div>
      )}
    </div>
  );
}
