import { useEffect, useState } from "react";
import type {
  AllocationSnapshot,
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

function formatShare(value: number, total: number): string {
  if (total === 0) {
    return "0%";
  }
  const percentage = (value / total) * 100;
  return `${Number.isInteger(percentage) ? percentage : percentage.toFixed(1)}%`;
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
  providerCapacity,
  providerSpendable,
  onEdit,
}: {
  scope: ScopeSummary;
  allocation?: AllocationSnapshot;
  unit: string;
  providerCapacity: number;
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
              <span>{formatAmount(usableNow, unit)} usable now</span>
              <span>{remainingPercent}% of folder cap</span>
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

      <div className="allocation-limit">
        <strong>{allocation ? formatAmount(allocation.limit, unit) : "—"}</strong>
        <span>
          {allocation
            ? `${formatShare(allocation.limit, providerCapacity)} of total quota`
            : "No limit"}
        </span>
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

        <nav className="nav-list" aria-label="Primary">
          <button className="nav-item active" type="button">
            <Icon name="gauge" size={19} />
            Overview
          </button>
          <button className="nav-item" type="button" onClick={onAddScope}>
            <Icon name="folder" size={19} />
            New allocation
          </button>
        </nav>

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
            <p className="eyebrow">Local subscription budget</p>
            <h1>{source.providerDisplayName} overview</h1>
          </div>
          <div className="topbar-actions">
            <label className="source-select">
              <select
                value={source.windowId}
                onChange={(event) => onSelectSource(event.currentTarget.value)}
                aria-label="Selected quota window"
              >
                {state.sources.map((item) => (
                  <option value={item.windowId} key={item.windowId}>
                    {item.providerDisplayName} · {item.poolDisplayName}
                  </option>
                ))}
              </select>
              <Icon name="chevron-down" size={16} />
            </label>
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
            <button
              className="icon-button bordered danger"
              type="button"
              onClick={() => onRemoveSource(source)}
              disabled={removingSource}
              aria-label="Remove quota source"
              title="Remove quota source"
            >
              <Icon name="trash" size={17} />
            </button>
            <button className="button dark" type="button" onClick={onAddScope}>
              <Icon name="plus" size={18} />
              New allocation
            </button>
          </div>
        </header>

        <section className="overview-grid">
          <article className="quota-hero">
            <div
              className="quota-ring"
              role="progressbar"
              aria-label="Provider quota remaining"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={availablePercent}
              style={{
                background: `conic-gradient(var(--accent) ${availablePercent * 3.6}deg, var(--ring-track) 0deg)`,
              }}
            >
              <div>
                <strong>{availablePercent}%</strong>
                <span>left</span>
              </div>
            </div>
            <div className="quota-hero-copy">
              <div className="status-line">
                <span className={`status-dot ${source.isActive ? "active" : ""}`} />
                {source.isActive ? "Active window" : "Inactive window"}
                <small>{formatLastSync(source.lastSyncedAt)}</small>
              </div>
              <h2>{formatAmount(quotaWindow.providerSpendable, quotaWindow.unit)}</h2>
              <p>available from {formatAmount(quotaWindow.capacity, quotaWindow.unit)}</p>
              <div className="window-range">
                <Icon name="calendar" size={17} />
                {dateRange(source)} · {formatReset(source.endsAt)}
              </div>
            </div>
          </article>

          <div className="metric-grid">
            <article className="metric-card">
              <span className="metric-icon green">
                <Icon name="folder" size={19} />
              </span>
              <p>Allocated</p>
              <strong>{formatAmount(quotaWindow.allocatedToRootScopes, quotaWindow.unit)}</strong>
              <small>
                {formatShare(
                  quotaWindow.allocatedToRootScopes,
                  quotaWindow.capacity,
                )}{" "}
                of total quota · {formatAmount(quotaWindow.unallocated, quotaWindow.unit)}{" "}
                unallocated
              </small>
            </article>
            <article className="metric-card">
              <span className="metric-icon violet">
                <Icon name="activity" size={19} />
              </span>
              <p>Unattributed</p>
              <strong>
                {formatAmount(quotaWindow.unattributedUsage, quotaWindow.unit)}
              </strong>
              <small>not assigned to a workspace</small>
            </article>
            <article className="metric-card">
              <span className="metric-icon amber">
                <Icon name="calendar" size={19} />
              </span>
              <p>Reset</p>
              <strong>{formatReset(quotaWindow.endsAt).split(" ")[0]}</strong>
              <small>{new Date(quotaWindow.endsAt).toLocaleString()}</small>
            </article>
            <article className="metric-card">
              <span className="metric-icon blue">
                <Icon name="shield" size={19} />
              </span>
              <p>Enforcement</p>
              <strong>Policy ready</strong>
              <small>80 / 90 / 100 thresholds</small>
            </article>
          </div>
        </section>

        <section className="allocations-section">
          <header className="section-header">
            <div>
              <p className="eyebrow">Budget map</p>
              <h2>Workspace allocations</h2>
              <span>
                Limits are shares of the full provider window; availability also
                respects the provider quota left now.
              </span>
            </div>
            <div className="section-actions">
              <button className="button outline" type="button" onClick={onAddScope}>
                <Icon name="plus" size={17} />
                Add workspace
              </button>
            </div>
          </header>

          <div className="allocation-table-header" aria-hidden="true">
            <span>Scope</span>
            <span>Available now</span>
            <span>Share of total</span>
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
                  providerCapacity={quotaWindow.capacity}
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
                  <strong>Start allocating your shared allowance</strong>
                  <p>
                    Choose a folder, then give that workspace a clear quota limit.
                  </p>
                </div>
                <button className="button dark" type="button" onClick={onAddScope}>
                  Create first allocation
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
