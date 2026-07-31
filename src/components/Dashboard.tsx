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

function verifiedProtectionAt(
  protection: CodexProtectionStatus | null,
  events: CodexProtectionEvent[],
): number | null {
  if (
    protection?.installed !== true ||
    protection.state !== "configured" ||
    events.length === 0
  ) {
    return null;
  }
  return events[0]?.occurredAt ?? null;
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
  verifiedAt,
  onOpenSettings,
}: {
  source: QuotaSourceSummary;
  protection: CodexProtectionStatus | null;
  verifiedAt: number | null;
  onOpenSettings: () => void;
}) {
  if (source.providerDisplayName.toLowerCase() !== "codex") {
    return null;
  }

  const sourceReady = source.lastSyncedAt !== null;
  const hooksReady = protection?.installed === true;
  const trustReady = hooksReady && verifiedAt !== null;
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
                  ? `Observed ${formatLastSync(verifiedAt)}`
                  : "Enable them, restart Codex, then submit a test prompt"}
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
  protectionActive,
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
  protectionActive: boolean;
  priorityBusy: boolean;
  dragging: boolean;
  dragOver: boolean;
  onDragStart: () => void;
  onDragOver: () => void;
  onDrop: () => void;
  onDragEnd: () => void;
  onEdit: () => void;
}) {
  const usedWithinAllocation = Math.min(
    allocation.attributedUsage,
    allocation.limit,
  );
  const overage = Math.max(
    0,
    allocation.attributedUsage - allocation.limit,
  );
  const usedPercent = allocation.limit
    ? Math.round((usedWithinAllocation / allocation.limit) * 100)
    : 0;
  const usedWidth = Math.min(100, usedPercent);
  const protectedPercent = allocation.limit
    ? Math.min(
        100 - usedWidth,
        Math.round((allocation.protectedNow / allocation.limit) * 100),
      )
    : 0;
  const fundedLabel = protectionActive ? "protected now" : "planned now";

  return (
    <article
      className={`overview-allocation-row ${
        overage > 0 ? "over-allocation" : ""
      } ${dragging ? "dragging" : ""} ${dragOver ? "drag-over" : ""}`}
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
      <div className="allocation-row-identity">
        <strong>{scope.displayName}</strong>
        <span>Priority {allocation.priority + 1}</span>
      </div>
      <div className="allocation-quota">
        <div className="allocation-quota-meta">
          <span>
            <strong>{formatAmount(allocation.protectedNow, unit)}</strong>{" "}
            {fundedLabel}
          </span>
          <span>
            {overage > 0 ? (
              <>
                {formatAmount(usedWithinAllocation, unit)} allocation used ·{" "}
                <strong className="allocation-overage">
                  {formatAmount(overage, unit)} over allocation
                </strong>
              </>
            ) : (
              <>
                {formatAmount(allocation.attributedUsage, unit)} used ·{" "}
                {usedPercent}% of allocation
              </>
            )}
          </span>
        </div>
        <div
          className="allocation-quota-track"
          role="progressbar"
          aria-label={
            overage > 0
              ? `${scope.displayName}: ${formatAmount(
                  allocation.attributedUsage,
                  unit,
                )} total usage, including ${formatAmount(
                  usedWithinAllocation,
                  unit,
                )} from its allocation and ${formatAmount(
                  overage,
                  unit,
                )} over its allocation; ${formatAmount(
                  allocation.protectedNow,
                  unit,
                )} ${fundedLabel}`
              : `${scope.displayName}: ${formatAmount(
                  allocation.attributedUsage,
                  unit,
                )} used and ${formatAmount(
                  allocation.protectedNow,
                  unit,
                )} ${fundedLabel} from a ${formatAmount(
                  allocation.limit,
                  unit,
                )} allocation`
          }
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.min(100, usedWidth + protectedPercent)}
        >
          <span
            className="allocation-used-segment"
            style={{ width: `${usedWidth}%` }}
          />
          <span
            className={
              protectionActive
                ? "allocation-protected-segment"
                : "allocation-planned-segment"
            }
            style={{ width: `${protectedPercent}%` }}
          />
        </div>
      </div>
      <span className="allocation-limit">
        <strong>{formatAmount(allocation.limit, unit)}</strong>
        allocation
      </span>
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
  const codexSource =
    source.providerDisplayName.toLowerCase() === "codex";
  const protectionVerifiedAt = codexSource
    ? verifiedProtectionAt(codexProtection, codexProtectionEvents)
    : null;
  const protectionActive = codexSource && protectionVerifiedAt !== null;
  const plannedCapacityNow = scopes.reduce(
    (total, scope) =>
      total + (allocationByScope.get(scope.id)?.protectedNow ?? 0),
    0,
  );
  const unassignedBufferNow = Math.max(
    0,
    quotaWindow.providerSpendable - plannedCapacityNow,
  );
  const usedAmount = Math.max(
    0,
    quotaWindow.capacity - quotaWindow.providerSpendable,
  );
  const percentOfWindow = (amount: number) =>
    quotaWindow.capacity
      ? Math.min(100, Math.max(0, (amount / quotaWindow.capacity) * 100))
      : 0;
  const usedSlicePercent = percentOfWindow(usedAmount);
  const plannedSlicePercent = percentOfWindow(plannedCapacityNow);
  const plannedSliceEnd = Math.min(
    100,
    usedSlicePercent + plannedSlicePercent,
  );
  const plannedLabel = protectionActive ? "Protected" : "Planned";
  const nextAllocationAtRisk = [...scopes]
    .reverse()
    .find(
      (scope) => (allocationByScope.get(scope.id)?.protectedNow ?? 0) > 0,
    );
  const nextAtRiskPriority = nextAllocationAtRisk
    ? (allocationByScope.get(nextAllocationAtRisk.id)?.priority ?? 0)
    : null;
  const capacityErosionOrder = nextAllocationAtRisk
    ? nextAtRiskPriority !== null && nextAtRiskPriority > 0
      ? `Funding then erodes from ${nextAllocationAtRisk.displayName} toward higher priorities.`
      : `Further usage then reduces ${nextAllocationAtRisk.displayName}'s planned capacity.`
    : "No allocation has funded capacity left.";
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
                verifiedAt={protectionVerifiedAt}
                onOpenSettings={() => onViewChange("settings")}
              />
            </header>

            <div className="overview-content block-dashboard-content">
              {codexSource && scopes.length > 0 && !protectionActive && (
                <section
                  className="dashboard-protection-notice unverified"
                  role="alert"
                >
                  <Icon name="shield" size={20} />
                  <div>
                    <strong>
                      Allocations are a priority plan—not enforced yet
                    </strong>
                    <span>
                      {unassignedBufferNow > 0
                        ? `Unmanaged Codex usage consumes the ${formatAmount(
                            unassignedBufferNow,
                            quotaWindow.unit,
                          )} unassigned capacity still available now. ${capacityErosionOrder}`
                        : nextAllocationAtRisk
                          ? `No unassigned capacity remains. The next unmanaged Codex usage reduces ${nextAllocationAtRisk.displayName} first${
                              nextAtRiskPriority !== null &&
                              nextAtRiskPriority > 0
                                ? ", then moves toward higher priorities."
                                : "."
                            }`
                          : "No allocation has funded capacity left. Codex usage can continue until protection is activated."}
                    </span>
                  </div>
                  <button
                    className="button primary small"
                    type="button"
                    onClick={() => onViewChange("settings")}
                  >
                    Finish protection
                  </button>
                </section>
              )}

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
                      className={`used-progress ${statusTone}`}
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

                <article className="dashboard-block allocation-summary-block">
                  <div className="reset-summary">
                    <span>Resets</span>
                    <strong>
                      {remainingDays} {remainingDays === 1 ? "day" : "days"}
                    </strong>
                    <small>{formatDate(quotaWindow.endsAt)}</small>
                  </div>
                  <div className="allocation-donut-group">
                    <div
                      className="allocation-donut"
                      style={{
                        background: `conic-gradient(${statusTone === "danger" ? "var(--danger)" : "var(--quota-used)"} 0 ${usedSlicePercent}%, ${protectionActive ? "var(--success)" : "var(--warning)"} ${usedSlicePercent}% ${plannedSliceEnd}%, var(--success) ${plannedSliceEnd}% 100%)`,
                      }}
                      role="img"
                      aria-label={`${formatAmount(
                        usedAmount,
                        quotaWindow.unit,
                      )} used, ${formatAmount(
                        plannedCapacityNow,
                        quotaWindow.unit,
                      )} ${plannedLabel.toLowerCase()}, and ${formatAmount(
                        unassignedBufferNow,
                        quotaWindow.unit,
                      )} unassigned in the current quota window`}
                    >
                      <span>
                        <strong>
                          {formatAmount(
                            quotaWindow.providerSpendable,
                            quotaWindow.unit,
                          )}
                        </strong>
                        <small>left</small>
                      </span>
                    </div>
                    <dl className="allocation-legend">
                      <div>
                        <dt>
                          <i className="used" />
                          Used
                        </dt>
                        <dd>
                          {formatAmount(usedAmount, quotaWindow.unit)}
                        </dd>
                      </div>
                      <div>
                        <dt>
                          <i
                            className={`funded ${
                              protectionActive ? "protected" : ""
                            }`}
                          />
                          {plannedLabel}
                        </dt>
                        <dd>
                          {formatAmount(plannedCapacityNow, quotaWindow.unit)}
                        </dd>
                      </div>
                      <div>
                        <dt>
                          <i className="unassigned" />
                          Unassigned
                        </dt>
                        <dd>
                          {formatAmount(
                            unassignedBufferNow,
                            quotaWindow.unit,
                          )}
                        </dd>
                      </div>
                    </dl>
                  </div>
                </article>
              </section>

              <section className="workspace-budget-section dashboard-block">
                <header>
                  <div>
                    <h2>Workspace allocations</h2>
                    <p>
                      {protectionActive
                        ? "Drag to set priority. Higher allocations are protected first."
                        : "Drag to set the funding plan. Lowest priorities lose capacity first."}
                    </p>
                  </div>
                  <button
                    className="button primary"
                    type="button"
                    onClick={onAddScope}
                  >
                    <Icon name="plus" size={17} />
                    Add allocation
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
                        protectionActive={protectionActive}
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
