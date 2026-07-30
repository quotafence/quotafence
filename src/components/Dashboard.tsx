import type {
  AllocationSnapshot,
  LocalState,
  QuotaSourceSummary,
  ScopeKind,
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
  onRecordUsage: (scopeId?: string) => void;
  onRefresh: () => void;
  onRemoveSource: () => void;
  removingSource: boolean;
};

const kindIcons: Record<
  ScopeKind,
  "folder" | "repository" | "task" | "shield"
> = {
  project: "folder",
  repository: "repository",
  task: "task",
  reserve: "shield",
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

function orderedScopes(scopes: ScopeSummary[]): ScopeSummary[] {
  const byParent = new Map<string | null, ScopeSummary[]>();
  for (const scope of scopes) {
    const siblings = byParent.get(scope.parentId) ?? [];
    siblings.push(scope);
    byParent.set(scope.parentId, siblings);
  }
  for (const siblings of byParent.values()) {
    siblings.sort((left, right) => left.displayName.localeCompare(right.displayName));
  }

  const ordered: ScopeSummary[] = [];
  const visited = new Set<string>();
  function append(parentId: string | null) {
    for (const scope of byParent.get(parentId) ?? []) {
      if (visited.has(scope.id)) {
        continue;
      }
      visited.add(scope.id);
      ordered.push(scope);
      append(scope.id);
    }
  }
  append(null);
  for (const scope of scopes) {
    if (!visited.has(scope.id)) {
      ordered.push(scope);
    }
  }
  return ordered;
}

function AllocationRow({
  scope,
  allocation,
  unit,
  onEdit,
  onRecord,
}: {
  scope: ScopeSummary;
  allocation?: AllocationSnapshot;
  unit: string;
  onEdit: () => void;
  onRecord: () => void;
}) {
  const committed = allocation
    ? allocation.attributedUsage + allocation.activeReservations
    : 0;
  const progress = allocation?.limit
    ? Math.min(100, Math.round((committed / allocation.limit) * 100))
    : 0;
  const decision = allocation?.decision ?? "allow";

  return (
    <article className={`allocation-row ${scope.parentId ? "child" : ""}`}>
      <div className="scope-identity">
        <span className={`scope-icon ${scope.kind}`}>
          <Icon name={kindIcons[scope.kind]} size={18} />
        </span>
        <div>
          <strong>{scope.displayName}</strong>
          <span>
            {scope.kind}
            {scope.parentId ? " · nested allocation" : ""}
          </span>
        </div>
      </div>

      <div className="allocation-progress">
        {allocation ? (
          <>
            <div className="progress-meta">
              <span>{formatAmount(committed, unit)} committed</span>
              <span>{progress}%</span>
            </div>
            <div className="progress-track">
              <span
                className={`progress-fill ${decision}`}
                style={{ width: `${progress}%` }}
              />
            </div>
          </>
        ) : (
          <span className="unallocated-label">Not allocated in this window</span>
        )}
      </div>

      <div className="allocation-limit">
        <strong>{allocation ? formatAmount(allocation.limit, unit) : "—"}</strong>
        <span>{allocation ? `${formatAmount(allocation.remaining, unit)} left` : "No limit"}</span>
      </div>

      <div className="row-actions">
        <button className="text-button" type="button" onClick={onRecord}>
          Add usage
        </button>
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
  onRecordUsage,
  onRefresh,
  onRemoveSource,
  removingSource,
}: DashboardProps) {
  const dashboard = state.dashboard;
  const source = state.sources.find(
    (candidate) => candidate.windowId === state.selectedWindowId,
  );

  if (!dashboard || !source) {
    return null;
  }

  const { window } = dashboard;
  const used = Math.max(0, window.capacity - window.providerRemaining);
  const usedPercent = window.capacity
    ? Math.min(100, Math.round((used / window.capacity) * 100))
    : 0;
  const allocationByScope = new Map(
    dashboard.allocations.map((allocation) => [allocation.scopeId, allocation]),
  );
  const scopes = orderedScopes(state.scopes);
  const allocatedParents = state.scopes.filter(
    (scope) =>
      !scope.parentId &&
      allocationByScope.has(scope.id) &&
      scope.kind !== "task",
  );

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
          <button className="nav-item" type="button" onClick={() => onRecordUsage()}>
            <Icon name="activity" size={19} />
            Record usage
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
              aria-label="Refresh"
            >
              <Icon name="refresh" size={18} className={refreshing ? "spin" : ""} />
            </button>
            <button
              className="icon-button bordered danger"
              type="button"
              onClick={onRemoveSource}
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
              style={{
                background: `conic-gradient(var(--accent) ${usedPercent * 3.6}deg, var(--ring-track) 0deg)`,
              }}
            >
              <div>
                <strong>{usedPercent}%</strong>
                <span>used</span>
              </div>
            </div>
            <div className="quota-hero-copy">
              <div className="status-line">
                <span className={`status-dot ${source.isActive ? "active" : ""}`} />
                {source.isActive ? "Active window" : "Inactive window"}
                <small>{formatLastSync(source.lastSyncedAt)}</small>
              </div>
              <h2>{formatAmount(window.providerSpendable, window.unit)}</h2>
              <p>available from {formatAmount(window.capacity, window.unit)}</p>
              <div className="window-range">
                <Icon name="calendar" size={17} />
                {dateRange(source)} · {formatReset(source.endsAt)}
              </div>
            </div>
            <button className="button lime" type="button" onClick={() => onRecordUsage()}>
              <Icon name="activity" size={17} />
              Record usage
            </button>
          </article>

          <div className="metric-grid">
            <article className="metric-card">
              <span className="metric-icon green">
                <Icon name="folder" size={19} />
              </span>
              <p>Allocated</p>
              <strong>{formatAmount(window.allocatedToRootScopes, window.unit)}</strong>
              <small>{formatAmount(window.unallocated, window.unit)} unallocated</small>
            </article>
            <article className="metric-card">
              <span className="metric-icon violet">
                <Icon name="activity" size={19} />
              </span>
              <p>Observed usage</p>
              <strong>{formatAmount(used, window.unit)}</strong>
              <small>
                {formatAmount(window.unattributedUsage, window.unit)} unattributed
              </small>
            </article>
            <article className="metric-card">
              <span className="metric-icon amber">
                <Icon name="calendar" size={19} />
              </span>
              <p>Reset</p>
              <strong>{formatReset(window.endsAt).split(" ")[0]}</strong>
              <small>{new Date(window.endsAt).toLocaleString()}</small>
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
              <h2>Project allocations</h2>
              <span>
                Usage in a task also debits every parent allocation.
              </span>
            </div>
            <div className="section-actions">
              <button className="button subtle" type="button" onClick={() => onRecordUsage()}>
                Add usage
              </button>
              <button className="button outline" type="button" onClick={onAddScope}>
                <Icon name="plus" size={17} />
                Add project
              </button>
            </div>
          </header>

          <div className="allocation-table-header" aria-hidden="true">
            <span>Scope</span>
            <span>Consumption</span>
            <span>Limit</span>
            <span />
          </div>

          <div className="allocation-list">
            {scopes.length > 0 ? (
              scopes.map((scope) => (
                <AllocationRow
                  key={scope.id}
                  scope={scope}
                  allocation={allocationByScope.get(scope.id)}
                  unit={window.unit}
                  onEdit={() => onEditAllocation(scope)}
                  onRecord={() => onRecordUsage(scope.id)}
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
                    Add a project or repository, then give it a clear quota limit.
                  </p>
                </div>
                <button className="button dark" type="button" onClick={onAddScope}>
                  Create first allocation
                </button>
              </div>
            )}
          </div>

          {allocatedParents.length > 0 && (
            <footer className="allocation-footer">
              <Icon name="shield" size={17} />
              Tasks can be nested under {allocatedParents.length} allocated{" "}
              {allocatedParents.length === 1 ? "parent" : "parents"}.
            </footer>
          )}
        </section>
      </main>
    </div>
  );
}
