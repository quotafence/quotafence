import { useEffect, useRef, useState } from "react";
import type {
  AllocationSnapshot,
  CodexProtectionEvent,
  CodexProtectionStatus,
  LocalState,
  QuotaSourceSummary,
  ScopeSummary,
} from "../types";
import { Icon } from "./Icon";
import {
  SettingsPanel,
  type ThemePreference,
} from "./SettingsPanel";

export type DashboardView = "overview" | "settings";

type DashboardProps = {
  state: LocalState;
  view: DashboardView;
  onViewChange: (view: DashboardView) => void;
  refreshing: boolean;
  onSelectSource: (windowId: string) => void;
  onAddSource: () => void;
  onAddScope: () => void;
  onEditAllocation: (scope: ScopeSummary) => void;
  onRefresh: () => void;
  onRemoveSource: (source: QuotaSourceSummary) => void;
  removingSource: boolean;
  codexProtection: CodexProtectionStatus | null;
  codexProtectionEvents: CodexProtectionEvent[];
  protectionBusy: boolean;
  onProtection: (enabled: boolean) => void;
  theme: ThemePreference;
  onThemeChange: (theme: ThemePreference) => void;
  priorityBusy: boolean;
  onPriorityOrder: (orderedScopeIds: string[]) => void;
};

function labelUnit(unit: string): string {
  return unit.split("_").join(" ");
}

function formatAmount(value: number, unit: string): string {
  if (unit === "percent") {
    return `${Math.round(value)}%`;
  }
  return `${Math.round(value).toLocaleString()} ${labelUnit(unit)}`;
}

function daysRemaining(endsAt: number): number {
  return Math.max(0, Math.ceil((endsAt - Date.now()) / 86_400_000));
}

function formatDate(timestamp: number): string {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
  }).format(timestamp);
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
  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return `Synced ${hours}h ago`;
  }
  return `Synced ${Math.floor(hours / 24)}d ago`;
}

function orderedScopes(
  scopes: ScopeSummary[],
  allocations: AllocationSnapshot[],
): ScopeSummary[] {
  const priority = new Map(
    allocations.map((allocation) => [allocation.scopeId, allocation.priority]),
  );
  return scopes
    .filter(
      (scope) =>
        scope.kind === "workspace" &&
        scope.parentId === null &&
        scope.workspacePath !== null &&
        priority.has(scope.id),
    )
    .sort(
      (left, right) =>
        (priority.get(left.id) ?? Number.MAX_SAFE_INTEGER) -
        (priority.get(right.id) ?? Number.MAX_SAFE_INTEGER),
    );
}

function SetupDisclosure({
  source,
  protection,
  onOpenSettings,
}: {
  source: QuotaSourceSummary;
  protection: CodexProtectionStatus | null;
  onOpenSettings: () => void;
}) {
  if (source.providerDisplayName.toLowerCase() !== "codex") {
    return null;
  }

  const sourceReady = source.lastSyncedAt !== null;
  const hooksReady = protection?.installed === true;
  const trustReady = hooksReady && protection?.requiresReview === false;
  const completed = [sourceReady, hooksReady, trustReady].filter(Boolean).length;

  if (completed === 3) {
    return null;
  }

  return (
    <details className="setup-disclosure">
      <summary className="setup-chip">
        Setup {completed}/3
      </summary>
      <section className="setup-panel">
        <header>
          <strong>Finish Codex setup</strong>
          <span>
            AQM only marks checks complete when it can verify them locally.
          </span>
        </header>
        <ol>
          <li className={sourceReady ? "complete" : ""}>
            <Icon name={sourceReady ? "check" : "activity"} size={16} />
            <div>
              <strong>Quota source synced</strong>
              <span>{sourceReady ? formatLastSync(source.lastSyncedAt) : "Sync required"}</span>
            </div>
          </li>
          <li className={hooksReady ? "complete" : ""}>
            <Icon name={hooksReady ? "check" : "activity"} size={16} />
            <div>
              <strong>Lifecycle hooks installed</strong>
              <span>
                UserPromptSubmit, Stop, and SessionEnd
              </span>
            </div>
          </li>
          <li className={trustReady ? "complete" : ""}>
            <Icon name={trustReady ? "check" : "shield"} size={16} />
            <div>
              <strong>Hooks trusted and enabled in Codex</strong>
              <span>
                {trustReady
                  ? "Verified"
                  : "Codex does not expose this state to AQM yet"}
              </span>
            </div>
          </li>
        </ol>
        <button
          className="button primary wide"
          type="button"
          onClick={onOpenSettings}
        >
          Fix this
          <Icon name="arrow-right" size={16} />
        </button>
      </section>
    </details>
  );
}

function AllocationRow({
  scope,
  allocation,
  unit,
  priorityBusy,
  dragging,
  dragOver,
  onDragStart,
  onDragOver,
  onDrop,
  onDragEnd,
  onEdit,
}: {
  scope: ScopeSummary;
  allocation: AllocationSnapshot;
  unit: string;
  priorityBusy: boolean;
  dragging: boolean;
  dragOver: boolean;
  onDragStart: () => void;
  onDragOver: () => void;
  onDrop: () => void;
  onDragEnd: () => void;
  onEdit: () => void;
}) {
  return (
    <article
      className={`overview-allocation-row ${dragging ? "dragging" : ""} ${
        dragOver ? "drag-over" : ""
      }`}
      draggable={!priorityBusy}
      onDragStart={(event) => {
        event.dataTransfer.effectAllowed = "move";
        event.dataTransfer.setData("text/plain", scope.id);
        onDragStart();
      }}
      onDragOver={(event) => {
        event.preventDefault();
        event.dataTransfer.dropEffect = "move";
        onDragOver();
      }}
      onDrop={(event) => {
        event.preventDefault();
        onDrop();
      }}
      onDragEnd={onDragEnd}
    >
      <strong>{scope.displayName}</strong>
      <span>{formatAmount(allocation.limit, unit)}</span>
      <button
        className="row-menu-button"
        type="button"
        onClick={onEdit}
        aria-label={`Adjust ${scope.displayName}`}
        title={`Adjust ${scope.displayName}`}
      >
        …
      </button>
      <span className="overview-drag-handle" title="Drag to change priority">
        <Icon name="grip" size={18} />
      </span>
    </article>
  );
}

export function Dashboard({
  state,
  view,
  onViewChange,
  refreshing,
  onSelectSource,
  onAddSource,
  onAddScope,
  onEditAllocation,
  onRefresh,
  onRemoveSource,
  removingSource,
  codexProtection,
  codexProtectionEvents,
  protectionBusy,
  onProtection,
  theme,
  onThemeChange,
  priorityBusy,
  onPriorityOrder,
}: DashboardProps) {
  const [sourceMenu, setSourceMenu] = useState<{
    source: QuotaSourceSummary;
    x: number;
    y: number;
  } | null>(null);
  const [draggedScopeId, setDraggedScopeId] = useState<string | null>(null);
  const [dragOverScopeId, setDragOverScopeId] = useState<string | null>(null);
  const mainContentRef = useRef<HTMLElement>(null);

  useEffect(() => {
    mainContentRef.current?.scrollTo({ top: 0 });
  }, [view, state.selectedWindowId]);

  useEffect(() => {
    const handleRefreshShortcut = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "r") {
        event.preventDefault();
        if (!refreshing) {
          onRefresh();
        }
      }
    };
    window.addEventListener("keydown", handleRefreshShortcut);
    return () => window.removeEventListener("keydown", handleRefreshShortcut);
  }, [onRefresh, refreshing]);

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
  const remainingDays = daysRemaining(quotaWindow.endsAt);
  const dailyBudget =
    remainingDays > 0
      ? Math.floor(quotaWindow.providerSpendable / remainingDays)
      : 0;
  const availablePercent = quotaWindow.capacity
    ? Math.min(
        100,
        Math.round(
          (quotaWindow.providerSpendable / quotaWindow.capacity) * 100,
        ),
      )
    : 0;
  const usedPercent = Math.max(0, 100 - availablePercent);
  const allocationByScope = new Map(
    dashboard.allocations.map((allocation) => [allocation.scopeId, allocation]),
  );
  const scopes = orderedScopes(state.scopes, dashboard.allocations);
  const forecast = dashboard.forecast;
  const showForecast =
    forecast.sampleCount >= 5 &&
    forecast.status === "depletes_before_reset" &&
    forecast.projectedDepletionAt !== null;
  const projectedDays =
    showForecast && forecast.projectedDepletionAt !== null
      ? Math.max(
          0,
          Math.ceil((forecast.projectedDepletionAt - Date.now()) / 86_400_000),
        )
      : 0;
  const statusTone =
    availablePercent < 10 ? "danger" : showForecast ? "warning" : "neutral";
  const statusMessage =
    availablePercent < 10
      ? `Only ${formatAmount(
          quotaWindow.providerSpendable,
          quotaWindow.unit,
        )} left. Reserve it for priority work.`
      : showForecast
        ? `At your current pace you run out in ${projectedDays} ${
            projectedDays === 1 ? "day" : "days"
          }.`
        : `About ${formatAmount(
            dailyBudget,
            quotaWindow.unit,
          )} a day keeps you safe.`;
  const finishPriorityDrag = (targetScopeId: string) => {
    if (
      draggedScopeId === null ||
      draggedScopeId === targetScopeId ||
      priorityBusy
    ) {
      setDraggedScopeId(null);
      setDragOverScopeId(null);
      return;
    }
    const orderedScopeIds = scopes.map((scope) => scope.id);
    const from = orderedScopeIds.indexOf(draggedScopeId);
    const to = orderedScopeIds.indexOf(targetScopeId);
    if (from >= 0 && to >= 0) {
      const [moved] = orderedScopeIds.splice(from, 1);
      orderedScopeIds.splice(to, 0, moved);
      onPriorityOrder(orderedScopeIds);
    }
    setDraggedScopeId(null);
    setDragOverScopeId(null);
  };

  return (
    <div className="app-layout overview-redesign">
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

        <nav className="sidebar-navigation" aria-label="Application">
          <button
            className={view === "overview" ? "active" : ""}
            type="button"
            onClick={() => onViewChange("overview")}
          >
            <Icon name="gauge" size={18} />
            <span>Overview</span>
          </button>
          <button
            className={view === "settings" ? "active" : ""}
            type="button"
            onClick={() => onViewChange("settings")}
          >
            <Icon name="settings" size={18} />
            <span>Settings</span>
          </button>
        </nav>

        <section className="sidebar-section">
          <div className="sidebar-label">
            <span>Quota sources</span>
            <button
              type="button"
              onClick={onAddSource}
              aria-label="Add quota source"
            >
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
                title={`${formatLastSync(
                  item.lastSyncedAt,
                )}. Right-click for source actions.`}
                onClick={() => {
                  onViewChange("overview");
                  onSelectSource(item.windowId);
                }}
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
        </section>
      </aside>

      <main ref={mainContentRef} className="main-content">
        {view === "settings" ? (
          <SettingsPanel
            protection={codexProtection}
            events={codexProtectionEvents}
            busy={protectionBusy}
            theme={theme}
            onThemeChange={onThemeChange}
            onProtection={onProtection}
          />
        ) : (
          <>
            <header className="topbar block-dashboard-header">
              <div>
                <h1>{source.providerDisplayName}</h1>
                <p className="topbar-subtitle">{source.poolDisplayName}</p>
              </div>
              <SetupDisclosure
                source={source}
                protection={codexProtection}
                onOpenSettings={() => onViewChange("settings")}
              />
            </header>

            <div className="overview-content block-dashboard-content">
              <section className="overview-block-grid">
                <article className="dashboard-block quota-dashboard-block">
                  <div className="block-kicker">
                    <span className={`status-dot ${source.isActive ? "active" : ""}`} />
                    {source.isActive ? "Active window" : "Inactive window"}
                    <small>{formatLastSync(source.lastSyncedAt)}</small>
                  </div>
                  <div className={`quota-story ${statusTone}`}>
                    <h2>
                      {formatAmount(
                        quotaWindow.providerSpendable,
                        quotaWindow.unit,
                      )}{" "}
                      left
                    </h2>
                    <p>{statusMessage}</p>
                  </div>
                  <div className="used-progress-section">
                    <div
                      className="used-progress"
                      role="progressbar"
                      aria-label={`${usedPercent}% used`}
                      aria-valuemin={0}
                      aria-valuemax={100}
                      aria-valuenow={usedPercent}
                    >
                      <span style={{ width: `${usedPercent}%` }} />
                    </div>
                    <div className="used-progress-meta">
                      <span>{formatDate(quotaWindow.startsAt)}</span>
                      <strong>{usedPercent}% used</strong>
                      <span>{formatDate(quotaWindow.endsAt)}</span>
                    </div>
                  </div>
                </article>

                <article className="dashboard-block window-facts-block">
                  <div>
                    <span>Resets</span>
                    <strong>
                      {remainingDays} {remainingDays === 1 ? "day" : "days"}
                    </strong>
                    <small>{formatDate(quotaWindow.endsAt)}</small>
                  </div>
                  <div>
                    <span>Reserved</span>
                    <strong>
                      {formatAmount(
                        quotaWindow.allocatedToRootScopes,
                        quotaWindow.unit,
                      )}
                    </strong>
                    <small>Across {scopes.length} workspaces</small>
                  </div>
                  <div>
                    <span>Unassigned</span>
                    <strong>
                      {formatAmount(
                        quotaWindow.unallocated,
                        quotaWindow.unit,
                      )}
                    </strong>
                    <small>Available for new workspace budgets</small>
                  </div>
                </article>
              </section>

              <section className="workspace-budget-section dashboard-block">
              <header>
                <div>
                  <h2>Workspace budgets</h2>
                  <p>
                    Drag to set priority. Higher workspaces are reserved first.
                  </p>
                </div>
                <button
                  className="button primary"
                  type="button"
                  onClick={onAddScope}
                >
                  <Icon name="plus" size={17} />
                  Add workspace
                </button>
              </header>

              <div className="overview-allocation-list">
                {scopes.map((scope) => {
                  const allocation = allocationByScope.get(scope.id);
                  if (!allocation) {
                    return null;
                  }
                  return (
                    <AllocationRow
                      key={scope.id}
                      scope={scope}
                      allocation={allocation}
                      unit={quotaWindow.unit}
                      priorityBusy={priorityBusy}
                      dragging={draggedScopeId === scope.id}
                      dragOver={
                        dragOverScopeId === scope.id &&
                        draggedScopeId !== scope.id
                      }
                      onDragStart={() => setDraggedScopeId(scope.id)}
                      onDragOver={() => setDragOverScopeId(scope.id)}
                      onDrop={() => finishPriorityDrag(scope.id)}
                      onDragEnd={() => {
                        setDraggedScopeId(null);
                        setDragOverScopeId(null);
                      }}
                      onEdit={() => onEditAllocation(scope)}
                    />
                  );
                })}
                <div className="overview-allocation-row unassigned-row">
                  <strong>Unassigned</strong>
                  <span>
                    {formatAmount(
                      quotaWindow.unallocated,
                      quotaWindow.unit,
                    )}
                  </span>
                  <span />
                  <span />
                </div>
              </div>
            </section>
            </div>
          </>
        )}
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
