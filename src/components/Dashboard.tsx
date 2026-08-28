import { useEffect, useRef, useState } from "react";
import type {
  KeyboardEvent as ReactKeyboardEvent,
  MouseEvent as ReactMouseEvent,
  PointerEvent as ReactPointerEvent,
} from "react";
import type {
  AllocationSnapshot,
  CodexProtectionEvent,
  CodexProtectionStatus,
  CodexSyncResult,
  ClaudeStatusLineStatus,
  ClaudeProtectionStatus,
  LocalState,
  QuotaSourceSummary,
  QuotaHistoryPoint,
  ScopeSummary,
} from "../types";
import { Icon } from "./Icon";
import { ProviderLogo } from "./ProviderLogo";
import {
  observedCodexHookAt,
  verifiedCodexProtectionAt,
} from "../lib/protection";
import {
  SettingsPanel,
  type ThemePreference,
} from "./SettingsPanel";

export type DashboardView = "overview" | "settings";

function combinedProviderKey(source: QuotaSourceSummary): string | null {
  switch (source.providerDisplayName.trim().toLowerCase()) {
    case "claude code":
      return "claude-code";
    case "codex":
    case "openai codex":
      return "codex";
    default:
      return null;
  }
}

type DashboardProps = {
  state: LocalState;
  view: DashboardView;
  onViewChange: (view: DashboardView) => void;
  refreshing: boolean;
  onSelectSource: (windowId: string) => void;
  onAddSource: () => void;
  onAddScope: () => void;
  onEditAllocation: (scope: ScopeSummary) => void;
  onRemoveAllocation: (scope: ScopeSummary) => void;
  onRefresh: () => void;
  onRemoveSource: (sources: QuotaSourceSummary[]) => void;
  removingSource: boolean;
  codexProtection: CodexProtectionStatus | null;
  codexProtectionEvents: CodexProtectionEvent[];
  codexSyncResult: CodexSyncResult | null;
  codexSyncIssue: string | null;
  protectionBusy: boolean;
  onProtection: (enabled: boolean) => void;
  claudeIntegration: ClaudeStatusLineStatus | null;
  claudeProtection: ClaudeProtectionStatus | null;
  claudeBusy: boolean;
  onClaudeIntegration: (enabled: boolean) => void;
  onClaudeProtection: (enabled: boolean) => void;
  onClaudeSync: () => void;
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

function startOfLocalDay(timestamp: number): number {
  const date = new Date(timestamp);
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

function UsageHeatmap({
  history,
  capacity,
  unit,
}: {
  history: QuotaHistoryPoint[];
  capacity: number;
  unit: string;
}) {
  const lastDay = startOfLocalDay(Date.now());
  const firstDayDate = new Date(lastDay);
  firstDayDate.setMonth(firstDayDate.getMonth() - 6);
  const firstDay = firstDayDate.getTime();
  const visibleHistory = history
    .filter(
      (point) => point.observedAt >= firstDay && point.observedAt < lastDay + 86_400_000,
    )
    .sort((a, b) => a.observedAt - b.observedAt);
  const usageByDay = new Map<number, number>();
  let previousRemaining = capacity;
  for (const point of visibleHistory) {
    const usageSinceLastCheckpoint = Math.max(
      0,
      previousRemaining - point.remaining,
    );
    const day = startOfLocalDay(point.observedAt);
    usageByDay.set(day, (usageByDay.get(day) ?? 0) + usageSinceLastCheckpoint);
    previousRemaining = point.remaining;
  }

  const days: Array<{ timestamp: number; usage: number } | null> = Array.from(
    { length: new Date(firstDay).getDay() },
    () => null,
  );
  for (let day = firstDay; day <= lastDay; ) {
    days.push({ timestamp: day, usage: usageByDay.get(day) ?? 0 });
    const next = new Date(day);
    next.setDate(next.getDate() + 1);
    day = next.getTime();
  }
  while (days.length % 7 !== 0) {
    days.push(null);
  }
  const weekCount = Math.max(1, days.length / 7);
  const activeDayCount = [...usageByDay.values()].filter(
    (usage) => usage > 0,
  ).length;
  const usageLevel = (usage: number) => {
    const ratio = usage / Math.max(1, capacity);
    if (usage <= 0) return 0;
    if (ratio <= 0.05) return 1;
    if (ratio <= 0.15) return 2;
    if (ratio <= 0.3) return 3;
    return 4;
  };

  return (
    <div className="quota-trend">
      <div className="quota-trend-heading">
        <span>Daily usage</span>
        <small>
          Last 6 months · {activeDayCount}{" "}
          {activeDayCount === 1 ? "active day" : "active days"}
        </small>
      </div>
      <div
        className="quota-heatmap"
        role="img"
        aria-label="Daily quota usage heatmap"
        style={{ gridTemplateColumns: `repeat(${weekCount}, minmax(0, 1fr))` }}
      >
        {days.map((day, index) =>
          day ? (
            <span
              className={`quota-heatmap-day level-${usageLevel(day.usage)}`}
              key={day.timestamp}
              title={`${formatDate(day.timestamp)} · ${formatAmount(day.usage, unit)} used`}
            />
          ) : (
            <span className="quota-heatmap-day empty" key={`empty-${index}`} />
          ),
        )}
      </div>
      <div className="quota-trend-axis">
        <span>{formatDate(firstDay)}</span>
        <span className="quota-heatmap-legend" aria-hidden="true">
          Less
          {[0, 1, 2, 3, 4].map((level) => (
            <i className={`level-${level}`} key={level} />
          ))}
          More
        </span>
        <span>{formatDate(lastDay)}</span>
      </div>
    </div>
  );
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
  observedAt,
  onOpenSettings,
}: {
  source: QuotaSourceSummary;
  protection: CodexProtectionStatus | null;
  observedAt: number | null;
  onOpenSettings: () => void;
}) {
  if (source.providerDisplayName.toLowerCase() !== "codex") {
    return null;
  }

  const sourceReady = source.lastSyncedAt !== null;
  const hooksReady = protection?.installed === true;
  const trustReady = hooksReady && observedAt !== null;
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
            QuotaFence verifies each step locally.
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
                UserPromptSubmit and Stop
              </span>
            </div>
          </li>
          <li className={trustReady ? "complete" : ""}>
            <Icon name={trustReady ? "check" : "shield"} size={16} />
            <div>
              <strong>Protection verified in a new Codex task</strong>
              <span>
                {trustReady
                  ? `Delivered ${formatLastSync(observedAt)}`
                  : "Trust both hooks, then create a new task and submit one prompt"}
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
  onPointerDragStart,
  onPointerDragMove,
  onPointerDragEnd,
  onPointerDragCancel,
  onMoveBy,
  onMenu,
}: {
  scope: ScopeSummary;
  allocation: AllocationSnapshot;
  unit: string;
  protectionActive: boolean;
  priorityBusy: boolean;
  dragging: boolean;
  dragOver: boolean;
  onPointerDragStart: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  onPointerDragMove: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  onPointerDragEnd: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  onPointerDragCancel: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  onMoveBy: (direction: -1 | 1) => void;
  onMenu: (event: ReactMouseEvent<HTMLButtonElement>) => void;
}) {
  const overage = Math.max(
    0,
    allocation.attributedUsage - allocation.limit,
  );
  const remainingPercent = allocation.limit
    ? Math.min(
        100,
        Math.round((allocation.protectedNow / allocation.limit) * 100),
      )
    : 0;
  const remainingLabel = protectionActive ? "protected" : "planned";

  return (
    <article
      data-allocation-scope-id={scope.id}
      className={`overview-allocation-row ${
        overage > 0 ? "over-allocation" : ""
      } ${dragging ? "dragging" : ""} ${dragOver ? "drag-over" : ""}`}
    >
      <div className="allocation-row-identity">
        <strong className="allocation-rank">{allocation.priority + 1}</strong>
        <div>
          <strong>{scope.displayName}</strong>
          <span title={scope.workspacePath ?? undefined}>
            {scope.workspacePath ?? "Folder path unavailable"}
          </span>
        </div>
      </div>
      <div className="allocation-quota">
        <div className="allocation-quota-meta">
          <span>
            <strong>
              {formatAmount(allocation.protectedNow, unit)} left
            </strong>
            {allocation.protectedNow > 0 ? ` · ${remainingLabel}` : null}
          </span>
          <span>
            {formatAmount(allocation.attributedUsage, unit)} tracked used
            {overage > 0 ? (
              <strong className="allocation-overage">
                {` · ${formatAmount(overage, unit)} over allocation`}
              </strong>
            ) : (
              <> · {remainingPercent}% of allocation left</>
            )}
          </span>
        </div>
        <div
          className="allocation-quota-track"
          role="progressbar"
          aria-label={
            overage > 0
              ? `${scope.displayName}: ${formatAmount(
                  allocation.protectedNow,
                  unit,
                )} left and ${formatAmount(
                  overage,
                  unit,
                )} over its allocation`
              : `${scope.displayName}: ${formatAmount(
                  allocation.protectedNow,
                  unit,
                )} left, ${remainingLabel}, from a ${formatAmount(
                  allocation.limit,
                  unit,
                )} allocation`
          }
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={remainingPercent}
        >
          <span
            className={
              protectionActive
                ? "allocation-protected-segment"
                : "allocation-planned-segment"
            }
            style={{ width: `${remainingPercent}%` }}
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
        onClick={onMenu}
        aria-label={`Allocation actions for ${scope.displayName}`}
        title={`Allocation actions for ${scope.displayName}`}
      >
        …
      </button>
      <button
        className="overview-drag-handle"
        type="button"
        disabled={priorityBusy}
        aria-label={`Change priority for ${scope.displayName}. Drag, or use the up and down arrow keys.`}
        aria-pressed={dragging}
        title="Drag to change priority, or use the arrow keys"
        onPointerDown={onPointerDragStart}
        onPointerMove={onPointerDragMove}
        onPointerUp={onPointerDragEnd}
        onPointerCancel={onPointerDragCancel}
        onKeyDown={(event: ReactKeyboardEvent<HTMLButtonElement>) => {
          if (event.key === "ArrowUp") {
            event.preventDefault();
            onMoveBy(-1);
          } else if (event.key === "ArrowDown") {
            event.preventDefault();
            onMoveBy(1);
          }
        }}
      >
        <Icon name="grip" size={18} />
      </button>
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
  onRemoveAllocation,
  onRefresh,
  onRemoveSource,
  removingSource,
  codexProtection,
  codexProtectionEvents,
  codexSyncResult,
  codexSyncIssue,
  protectionBusy,
  onProtection,
  claudeIntegration,
  claudeProtection,
  claudeBusy,
  onClaudeIntegration,
  onClaudeProtection,
  onClaudeSync,
  theme,
  onThemeChange,
  priorityBusy,
  onPriorityOrder,
}: DashboardProps) {
  const [sourceMenu, setSourceMenu] = useState<{
    sources: QuotaSourceSummary[];
    x: number;
    y: number;
  } | null>(null);
  const [allocationMenu, setAllocationMenu] = useState<{
    scope: ScopeSummary;
    x: number;
    y: number;
  } | null>(null);
  const [draggedScopeId, setDraggedScopeId] = useState<string | null>(null);
  const [dragOverScopeId, setDragOverScopeId] = useState<string | null>(null);
  const draggedScopeIdRef = useRef<string | null>(null);
  const dragOverScopeIdRef = useRef<string | null>(null);
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
    if (!sourceMenu && !allocationMenu) {
      return;
    }
    const close = () => {
      setSourceMenu(null);
      setAllocationMenu(null);
    };
    window.addEventListener("pointerdown", close);
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("pointerdown", close);
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
    };
  }, [allocationMenu, sourceMenu]);

  const dashboard = state.dashboard;
  const source = state.sources.find(
    (candidate) => candidate.windowId === state.selectedWindowId,
  );

  if (!dashboard || !source) {
    return null;
  }
  const providerWindows = state.sources.filter(
    (candidate) =>
      candidate.providerDisplayName.toLowerCase() ===
      source.providerDisplayName.toLowerCase(),
  );
  const sidebarSources = state.sources.reduce<
    Array<{ key: string; sources: QuotaSourceSummary[] }>
  >((groups, candidate) => {
    const providerKey = combinedProviderKey(candidate);
    if (providerKey === null) {
      groups.push({ key: candidate.windowId, sources: [candidate] });
      return groups;
    }

    const providerGroup = groups.find((group) => group.key === providerKey);
    if (providerGroup) {
      providerGroup.sources.push(candidate);
    } else {
      groups.push({ key: providerKey, sources: [candidate] });
    }
    return groups;
  }, []);

  const { window: quotaWindow } = dashboard;
  const shortWindow =
    quotaWindow.endsAt - quotaWindow.startsAt < 86_400_000;
  const pacingUnit = shortWindow ? "hour" : "day";
  const pacingArticle = shortWindow ? "an" : "a";
  const pacingUnitMillis = shortWindow ? 3_600_000 : 86_400_000;
  const remainingPacingUnits = Math.max(
    0,
    Math.ceil((quotaWindow.endsAt - Date.now()) / pacingUnitMillis),
  );
  const pacingBudget =
    remainingPacingUnits > 0
      ? Math.floor(quotaWindow.providerSpendable / remainingPacingUnits)
      : 0;
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
  const scopes = orderedScopes(state.scopes, dashboard.allocations);
  const codexSource =
    source.providerDisplayName.toLowerCase() === "codex";
  const claudeSource =
    source.providerDisplayName.toLowerCase() === "claude code";
  const protectionVerifiedAt = codexSource
    ? verifiedCodexProtectionAt(codexProtection, codexProtectionEvents)
    : null;
  const hookObservedAt = codexSource
    ? observedCodexHookAt(codexProtection)
    : null;
  const claudeProtectionActive =
    claudeSource &&
    (claudeProtection?.installed ?? false) &&
    claudeProtection?.lastHookObservedAt !== null;
  const protectionActive =
    (codexSource && protectionVerifiedAt !== null) || claudeProtectionActive;
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
  const attributedUsage = dashboard.allocations.reduce(
    (total, allocation) => total + allocation.attributedUsage,
    0,
  );
  const turnHealth = source.turnHealth;
  const attributionIssue =
    (turnHealth?.contendedCount ?? 0) > 0
      ? `${turnHealth?.contendedCount} overlapping ${turnHealth?.contendedCount === 1 ? "turn is" : "turns are"} being kept unassigned rather than guessed.`
      : (turnHealth?.staleCount ?? 0) > 0
        ? `${turnHealth?.staleCount} unfinished ${turnHealth?.staleCount === 1 ? "turn needs" : "turns need"} conservative recovery after a missing lifecycle event.`
        : quotaWindow.unattributedUsage > 0
          ? "Some provider usage could not be tied safely to one allocated folder."
          : (turnHealth?.pendingCount ?? 0) > 0
            ? `${turnHealth?.pendingCount} ${turnHealth?.pendingCount === 1 ? "turn is" : "turns are"} waiting for a closing checkpoint.`
            : "All observed usage currently has a clear attribution state.";
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
  const projectedPacingUnits =
    showForecast && forecast.projectedDepletionAt !== null
      ? Math.max(
          0,
          Math.ceil(
            (forecast.projectedDepletionAt - Date.now()) / pacingUnitMillis,
          ),
        )
      : 0;
  const statusTone =
    source.isActive && availablePercent < 10
      ? "danger"
      : source.isActive && showForecast
        ? "warning"
        : "neutral";
  const statusMessage =
    !source.isActive
      ? "This allowance window is inactive. Sync to check for a newer window."
      : availablePercent < 10
      ? `Only ${formatAmount(
          quotaWindow.providerSpendable,
          quotaWindow.unit,
        )} left. Reserve it for priority work.`
      : showForecast
        ? `At your current pace you run out in ${projectedPacingUnits} ${
            projectedPacingUnits === 1 ? pacingUnit : `${pacingUnit}s`
          }.`
        : `About ${formatAmount(
            pacingBudget,
            quotaWindow.unit,
          )} ${pacingArticle} ${pacingUnit} keeps you safe.`;
  const clearPriorityDrag = () => {
    draggedScopeIdRef.current = null;
    dragOverScopeIdRef.current = null;
    setDraggedScopeId(null);
    setDragOverScopeId(null);
  };
  const finishPriorityDrag = (targetScopeId: string | null) => {
    const draggedId = draggedScopeIdRef.current;
    if (
      draggedId === null ||
      targetScopeId === null ||
      draggedId === targetScopeId ||
      priorityBusy
    ) {
      clearPriorityDrag();
      return;
    }
    const orderedScopeIds = scopes.map((scope) => scope.id);
    const from = orderedScopeIds.indexOf(draggedId);
    const to = orderedScopeIds.indexOf(targetScopeId);
    if (from >= 0 && to >= 0) {
      const [moved] = orderedScopeIds.splice(from, 1);
      orderedScopeIds.splice(to, 0, moved);
      onPriorityOrder(orderedScopeIds);
    }
    clearPriorityDrag();
  };
  const beginPriorityDrag = (
    scopeId: string,
    event: ReactPointerEvent<HTMLButtonElement>,
  ) => {
    if (priorityBusy || event.button !== 0) {
      return;
    }
    event.currentTarget.setPointerCapture(event.pointerId);
    draggedScopeIdRef.current = scopeId;
    dragOverScopeIdRef.current = scopeId;
    setDraggedScopeId(scopeId);
    setDragOverScopeId(scopeId);
  };
  const updatePriorityDrag = (
    event: ReactPointerEvent<HTMLButtonElement>,
  ) => {
    if (draggedScopeIdRef.current === null) {
      return;
    }
    event.preventDefault();
    const row = document
      .elementFromPoint(event.clientX, event.clientY)
      ?.closest<HTMLElement>("[data-allocation-scope-id]");
    const targetScopeId = row?.dataset.allocationScopeId ?? null;
    if (
      targetScopeId !== null &&
      targetScopeId !== dragOverScopeIdRef.current
    ) {
      dragOverScopeIdRef.current = targetScopeId;
      setDragOverScopeId(targetScopeId);
    }
  };
  const endPriorityDrag = (
    event: ReactPointerEvent<HTMLButtonElement>,
  ) => {
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    finishPriorityDrag(dragOverScopeIdRef.current);
  };
  const cancelPriorityDrag = (
    event: ReactPointerEvent<HTMLButtonElement>,
  ) => {
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    clearPriorityDrag();
  };
  const movePriorityBy = (scopeId: string, direction: -1 | 1) => {
    if (priorityBusy) {
      return;
    }
    const orderedScopeIds = scopes.map((scope) => scope.id);
    const from = orderedScopeIds.indexOf(scopeId);
    const to = from + direction;
    if (from < 0 || to < 0 || to >= orderedScopeIds.length) {
      return;
    }
    [orderedScopeIds[from], orderedScopeIds[to]] = [
      orderedScopeIds[to],
      orderedScopeIds[from],
    ];
    onPriorityOrder(orderedScopeIds);
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
            {sidebarSources.map((group) => {
              const selectedGroupSource = group.sources.find(
                (candidate) => candidate.windowId === source.windowId,
              );
              const item =
                selectedGroupSource ??
                group.sources.find((candidate) => candidate.isActive) ??
                group.sources.find((candidate) =>
                  candidate.poolDisplayName.toLowerCase().includes("weekly"),
                ) ??
                group.sources[0];
              const latestSync = group.sources.reduce<number | null>(
                (latest, candidate) =>
                  candidate.lastSyncedAt !== null &&
                  (latest === null || candidate.lastSyncedAt > latest)
                    ? candidate.lastSyncedAt
                    : latest,
                null,
              );
              const groupedProvider =
                combinedProviderKey(item) !== null && group.sources.length > 1;
              return (
                <button
                  className={`source-item ${selectedGroupSource ? "active" : ""}`}
                  type="button"
                  key={group.key}
                  title={`${formatLastSync(
                    latestSync,
                  )}. Right-click for source actions.`}
                  onClick={() => {
                    onViewChange("overview");
                    onSelectSource(item.windowId);
                  }}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    setSourceMenu({
                      sources: group.sources,
                      x: Math.min(event.clientX, window.innerWidth - 180),
                      y: Math.min(event.clientY, window.innerHeight - 64),
                    });
                  }}
                >
                  <ProviderLogo
                    className="source-avatar"
                    providerName={item.providerDisplayName}
                  />
                  <span>
                    <strong>{item.providerDisplayName}</strong>
                    <small>
                      {groupedProvider
                        ? "5-hour + Weekly"
                        : item.poolDisplayName}
                    </small>
                  </span>
                  {group.sources.some((candidate) => candidate.isActive) && <i />}
                </button>
              );
            })}
            {claudeIntegration?.installed &&
              !state.sources.some(
                (item) =>
                  item.providerDisplayName.toLowerCase() === "claude code",
              ) && (
                <button
                  className="source-item source-item-pending"
                  type="button"
                  title="Claude has not reported subscription quota yet."
                  onClick={() => onViewChange("settings")}
                >
                  <ProviderLogo
                    className="source-avatar"
                    providerName="Claude Code"
                  />
                  <span>
                    <strong>Claude Code</strong>
                    <small>
                      {claudeIntegration.lastObservedAt === null
                        ? "Refresh quota to connect"
                        : "Connected · waiting for quota"}
                    </small>
                  </span>
                  <i />
                </button>
              )}
          </div>
        </section>
      </aside>

      <main ref={mainContentRef} className="main-content">
        {view === "settings" ? (
          <SettingsPanel
            protection={codexProtection}
            events={codexProtectionEvents}
            sources={state.sources}
            workspaceCount={scopes.filter((scope) => scope.workspacePath).length}
            syncResult={codexSyncResult}
            syncIssue={codexSyncIssue}
            checking={refreshing}
            onCheck={onRefresh}
            busy={protectionBusy}
            theme={theme}
            onThemeChange={onThemeChange}
            onProtection={onProtection}
            claudeIntegration={claudeIntegration}
            claudeProtection={claudeProtection}
            claudeBusy={claudeBusy}
            onClaudeIntegration={onClaudeIntegration}
            onClaudeProtection={onClaudeProtection}
            onClaudeSync={onClaudeSync}
          />
        ) : (
          <>
            <header className="topbar block-dashboard-header">
              <div>
                <h1>{source.providerDisplayName}</h1>
                <p className="topbar-subtitle">{source.poolDisplayName}</p>
              </div>
              <div className="topbar-actions">
                <button
                  className="button primary dashboard-sync-button"
                  type="button"
                  onClick={onRefresh}
                  disabled={refreshing}
                  aria-label={refreshing ? "Syncing quota" : "Sync quota now"}
                  title="Sync provider quota and Codex Desktop usage"
                >
                  <Icon
                    className={refreshing ? "spin" : undefined}
                    name="refresh"
                    size={17}
                  />
                  {refreshing ? "Syncing…" : "Sync"}
                </button>
                <SetupDisclosure
                  source={source}
                  protection={codexProtection}
                  observedAt={hookObservedAt}
                  onOpenSettings={() => onViewChange("settings")}
                />
              </div>
            </header>

            {providerWindows.length > 1 && (
              <nav className="provider-window-strip" aria-label="Provider quota windows">
                {providerWindows.map((candidate) => {
                  const left =
                    candidate.providerUsed === null
                      ? null
                      : Math.max(0, candidate.capacity - candidate.providerUsed);
                  return (
                    <button
                      key={candidate.windowId}
                      className={candidate.windowId === source.windowId ? "active" : ""}
                      type="button"
                      onClick={() => onSelectSource(candidate.windowId)}
                    >
                      <span>{candidate.poolDisplayName}</span>
                      <strong>
                        {left === null
                          ? "Waiting for quota"
                          : `${formatAmount(left, candidate.unit)} left`}
                      </strong>
                      <small>
                        {candidate.isActive
                          ? `Resets ${formatDate(candidate.endsAt)}`
                          : "Inactive window"}
                      </small>
                    </button>
                  );
                })}
              </nav>
            )}

            <div className="overview-content block-dashboard-content">
              {codexSource && scopes.length > 0 && !protectionActive && (
                <section
                  className="dashboard-protection-notice unverified"
                  role="alert"
                >
                  <Icon name="shield" size={20} />
                  <div>
                    <strong>
                      {codexProtection?.installed
                        ? hookObservedAt !== null
                          ? "Hook connected, but enforcement is degraded"
                          : "Current Codex tasks are tracking-only"
                        : "Allocations are a priority plan—not enforced yet"}
                    </strong>
                    <span>
                      {codexProtection?.installed
                        ? hookObservedAt !== null
                          ? `${codexProtection.lastHookIssue ?? "The latest prompt did not produce an enforceable quota decision."} QuotaFence will retry automatically. `
                          : "QuotaFence still tracks usage in existing tasks, but it cannot block their prompts. Trust and enable UserPromptSubmit and Stop, then create a new Codex task inside an allocated folder to activate hard protection. "
                        : ""}
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
                    {hookObservedAt !== null ? "View status" : "View setup"}
                  </button>
                </section>
              )}
              {claudeSource && scopes.length > 0 && !claudeProtectionActive && (
                <section
                  className="dashboard-protection-notice unverified"
                  role="alert"
                >
                  <Icon name="shield" size={20} />
                  <div>
                    <strong>Claude allocations are not protected yet</strong>
                    <span>
                      {claudeProtection?.installed
                        ? "Restart Claude and send a test prompt from an allocated folder. Until QuotaFence observes the lifecycle hook, these allocations are a priority plan only."
                        : "Enable Workspace protection in Claude Settings. Without the prompt gate, Claude usage can consume unassigned capacity and lower priorities."}
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
                      aria-label={`${availablePercent}% left`}
                      aria-valuemin={0}
                      aria-valuemax={100}
                      aria-valuenow={availablePercent}
                    >
                      <span style={{ width: `${availablePercent}%` }} />
                    </div>
                    <div className="used-progress-meta">
                      <span>{formatDate(quotaWindow.startsAt)}</span>
                      <strong>{availablePercent}% left</strong>
                      <span>{formatDate(quotaWindow.endsAt)}</span>
                    </div>
                  </div>
                  {shortWindow && (
                    <dl className="short-window-facts">
                      <div>
                        <dt>Used this window</dt>
                        <dd>{formatAmount(usedAmount, quotaWindow.unit)}</dd>
                      </div>
                      <div>
                        <dt>Time remaining</dt>
                        <dd>
                          {remainingPacingUnits}h
                        </dd>
                      </div>
                      <div>
                        <dt>Safe pace</dt>
                        <dd>
                          {formatAmount(pacingBudget, quotaWindow.unit)}/h
                        </dd>
                      </div>
                    </dl>
                  )}
                </article>

                <article className="dashboard-block allocation-summary-block">
                  <div className="reset-summary">
                    <span>{source.isActive ? "Resets" : "Window"}</span>
                    <strong>
                      {source.isActive
                        ? `${remainingPacingUnits} ${
                            remainingPacingUnits === 1
                              ? pacingUnit
                              : `${pacingUnit}s`
                          }`
                        : "Ended"}
                    </strong>
                    <small>{formatDate(quotaWindow.endsAt)}</small>
                  </div>
                  <div className="allocation-donut-group">
                    <div
                      className="allocation-donut"
                      style={{
                        background: `conic-gradient(var(--quiet) 0 ${usedSlicePercent}%, ${protectionActive ? "var(--success)" : "var(--success-muted)"} ${usedSlicePercent}% ${plannedSliceEnd}%, var(--unassigned) ${plannedSliceEnd}% 100%)`,
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
                        onPointerDragStart={(event) =>
                          beginPriorityDrag(scope.id, event)
                        }
                        onPointerDragMove={updatePriorityDrag}
                        onPointerDragEnd={endPriorityDrag}
                        onPointerDragCancel={cancelPriorityDrag}
                        onMoveBy={(direction) =>
                          movePriorityBy(scope.id, direction)
                        }
                        onMenu={(event) => {
                          const rect = event.currentTarget.getBoundingClientRect();
                          setAllocationMenu({
                            scope,
                            x: Math.min(
                              window.innerWidth - 196,
                              Math.max(12, rect.right - 184),
                            ),
                            y: Math.min(window.innerHeight - 92, rect.bottom + 8),
                          });
                        }}
                      />
                    );
                  })}
                </div>
              </section>

              <section className="attribution-diagnostics dashboard-block">
                <div className="attribution-diagnostics-heading">
                  <div>
                    <span className="block-kicker">Attribution</span>
                    <h2>Where provider usage went</h2>
                  </div>
                  <p>{attributionIssue}</p>
                </div>
                <dl>
                  <div>
                    <dt>Tracked to folders</dt>
                    <dd>{formatAmount(attributedUsage, quotaWindow.unit)}</dd>
                    <small>Debited from matching allocations</small>
                  </div>
                  <div>
                    <dt>Unassigned usage</dt>
                    <dd>
                      {formatAmount(
                        quotaWindow.unattributedUsage,
                        quotaWindow.unit,
                      )}
                    </dd>
                    <small>Reduces total quota, not a guessed folder</small>
                  </div>
                  <div>
                    <dt>Open observations</dt>
                    <dd>{turnHealth?.pendingCount ?? 0}</dd>
                    <small>
                      {(turnHealth?.contendedCount ?? 0) > 0
                        ? `${turnHealth?.contendedCount} overlapping`
                        : (turnHealth?.staleCount ?? 0) > 0
                          ? `${turnHealth?.staleCount} stale`
                          : "Awaiting safe reconciliation"}
                    </small>
                  </div>
                </dl>
              </section>

              {!shortWindow && (
                <section className="usage-history-section dashboard-block">
                  <UsageHeatmap
                    history={dashboard.quotaHistory}
                    capacity={quotaWindow.capacity}
                    unit={quotaWindow.unit}
                  />
                </section>
              )}
            </div>
          </>
        )}
      </main>

      {sourceMenu && (
        <div
          className="source-context-menu"
          role="menu"
          aria-label={`${sourceMenu.sources[0].providerDisplayName} source actions`}
          style={{ left: sourceMenu.x, top: sourceMenu.y }}
          onPointerDown={(event) => event.stopPropagation()}
        >
          <button
            type="button"
            role="menuitem"
            disabled={removingSource}
            onClick={() => {
              onRemoveSource(sourceMenu.sources);
              setSourceMenu(null);
            }}
          >
            <Icon name="trash" size={16} />
            Delete source
          </button>
        </div>
      )}
      {allocationMenu && (
        <div
          className="source-context-menu allocation-context-menu"
          role="menu"
          aria-label={`${allocationMenu.scope.displayName} allocation actions`}
          style={{ left: allocationMenu.x, top: allocationMenu.y }}
          onPointerDown={(event) => event.stopPropagation()}
        >
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              onEditAllocation(allocationMenu.scope);
              setAllocationMenu(null);
            }}
          >
            <Icon name="settings" size={16} />
            Adjust allocation
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              onRemoveAllocation(allocationMenu.scope);
              setAllocationMenu(null);
            }}
          >
            <Icon name="trash" size={16} />
            Delete allocation
          </button>
        </div>
      )}
    </div>
  );
}
