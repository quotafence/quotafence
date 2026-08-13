import { useState } from "react";
import type {
  CodexProtectionEvent,
  CodexProtectionStatus,
  CodexSyncResult,
  ClaudeStatusLineStatus,
  QuotaSourceSummary,
} from "../types";
import {
  observedCodexHookAt,
  verifiedCodexProtectionAt,
} from "../lib/protection";
import { Icon } from "./Icon";

export type ThemePreference = "system" | "light" | "dark";
type DecisionRange = "hour" | "day" | "week";
type SettingsTab = "general" | "codex" | "claude" | "activity";

const DECISION_RANGES: Array<{
  value: DecisionRange;
  label: string;
  duration: number;
}> = [
  { value: "hour", label: "1h", duration: 60 * 60_000 },
  { value: "day", label: "24h", duration: 24 * 60 * 60_000 },
  { value: "week", label: "7d", duration: 7 * 24 * 60 * 60_000 },
];

type SettingsPanelProps = {
  protection: CodexProtectionStatus | null;
  events: CodexProtectionEvent[];
  sources: QuotaSourceSummary[];
  workspaceCount: number;
  syncResult: CodexSyncResult | null;
  syncIssue: string | null;
  checking: boolean;
  busy: boolean;
  theme: ThemePreference;
  onThemeChange: (theme: ThemePreference) => void;
  onProtection: (enabled: boolean) => void;
  claudeIntegration: ClaudeStatusLineStatus | null;
  claudeBusy: boolean;
  onClaudeIntegration: (enabled: boolean) => void;
  onCheck: () => void;
};

function formatRelativeTime(timestamp: number): string {
  const seconds = Math.max(0, Math.round((Date.now() - timestamp) / 1_000));
  if (seconds < 60) {
    return "just now";
  }
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m ago`;
  }
  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return `${hours}h ago`;
  }
  return `${Math.floor(hours / 24)}d ago`;
}

function folderName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

function statusLabel(
  protection: CodexProtectionStatus,
  observedAt: number | null,
  verifiedAt: number | null,
): string {
  if (verifiedAt !== null) {
    return "Active";
  }
  if (observedAt !== null) {
    return "Connected";
  }
  switch (protection.state) {
    case "configured":
      return "Verify in Codex";
    case "misconfigured":
      return "Needs attention";
    case "disabled":
      return "Off";
  }
}

export function SettingsPanel({
  protection,
  events,
  sources,
  workspaceCount,
  syncResult,
  syncIssue,
  checking,
  busy,
  theme,
  onThemeChange,
  onProtection,
  claudeIntegration,
  claudeBusy,
  onClaudeIntegration,
  onCheck,
}: SettingsPanelProps) {
  const [decisionRange, setDecisionRange] = useState<DecisionRange>("day");
  const [settingsTab, setSettingsTab] = useState<SettingsTab>(() =>
    claudeIntegration?.installed &&
    !sources.some(
      (candidate) => candidate.providerDisplayName.toLowerCase() === "claude code",
    )
      ? "claude"
      : "general",
  );
  const verifiedAt = verifiedCodexProtectionAt(protection, events);
  const observedAt = observedCodexHookAt(protection);
  const desktopTracking = syncResult?.desktopTracking ?? null;
  const codexSource = sources.find(
    (candidate) => candidate.providerDisplayName.toLowerCase() === "codex",
  );
  const turnHealth = codexSource?.turnHealth ?? null;
  const desktopHealthy =
    desktopTracking !== null && desktopTracking.status !== "unavailable";
  const integrationHealthy =
    syncIssue === null &&
    codexSource?.lastSyncedAt != null &&
    workspaceCount > 0 &&
    desktopHealthy &&
    verifiedAt !== null;
  const integrationNeedsAttention =
    syncIssue !== null ||
    workspaceCount === 0 ||
    desktopTracking?.status === "unavailable" ||
    (turnHealth?.staleCount ?? 0) > 0 ||
    (turnHealth?.contendedCount ?? 0) > 0 ||
    protection?.state === "misconfigured";
  const selectedDecisionRange = DECISION_RANGES.find(
    (range) => range.value === decisionRange,
  )!;
  const decisionCutoff = Date.now() - selectedDecisionRange.duration;
  const visibleEvents = events.filter(
    (event) => event.occurredAt >= decisionCutoff,
  );
  const allowedDecisionCount = visibleEvents.filter(
    (event) => event.outcome === "allowed",
  ).length;
  const blockedDecisionCount = visibleEvents.length - allowedDecisionCount;
  const claudeSources = sources.filter(
    (candidate) => candidate.providerDisplayName.toLowerCase() === "claude code",
  );
  const latestClaudeSync = claudeSources.reduce<number | null>(
    (latest, candidate) =>
      candidate.lastSyncedAt !== null &&
      (latest === null || candidate.lastSyncedAt > latest)
        ? candidate.lastSyncedAt
        : latest,
    null,
  );

  return (
    <>
      <header className="topbar settings-topbar">
        <div>
          <h1>Settings</h1>
          <p className="topbar-subtitle">
            Local integrations, enforcement, and privacy.
          </p>
        </div>
      </header>

      <nav className="settings-tabs" aria-label="Settings sections">
        {(
          [
            ["general", "General"],
            ["codex", "Codex"],
            ["claude", "Claude"],
            ["activity", "Activity"],
          ] as const
        ).map(([value, label]) => (
          <button
            key={value}
            type="button"
            className={settingsTab === value ? "active" : ""}
            aria-current={settingsTab === value ? "page" : undefined}
            onClick={() => setSettingsTab(value)}
          >
            {label}
          </button>
        ))}
      </nav>

      <section className="settings-card" hidden={settingsTab !== "general"}>
        <header className="settings-card-header">
          <span className="settings-card-icon">
            <Icon name="sun" size={20} />
          </span>
          <div>
            <h2>Appearance</h2>
            <p>Choose a theme or follow your system automatically.</p>
          </div>
        </header>

        <div
          className="theme-options"
          role="radiogroup"
          aria-label="Application theme"
        >
          {(
            [
              ["system", "monitor", "System"],
              ["light", "sun", "Light"],
              ["dark", "moon", "Dark"],
            ] as const
          ).map(([value, icon, label]) => (
            <button
              key={value}
              className={theme === value ? "active" : ""}
              type="button"
              role="radio"
              aria-checked={theme === value}
              onClick={() => onThemeChange(value)}
            >
              <Icon name={icon} size={18} />
              <span>{label}</span>
              {theme === value && <Icon name="check" size={15} />}
            </button>
          ))}
        </div>
      </section>

      <section className="settings-card" hidden={settingsTab !== "claude"}>
        <header className="settings-card-header">
          <span className="settings-card-icon">
            <Icon name="activity" size={20} />
          </span>
          <div>
            <h2>Claude Code tracking</h2>
            <p>Observe subscription quota through Claude's local status line.</p>
          </div>
          {claudeIntegration && (
            <span
              className={`settings-status ${
                latestClaudeSync !== null
                  ? "active"
                  : claudeIntegration.state === "conflict" ||
                      claudeIntegration.state === "misconfigured"
                    ? "misconfigured"
                    : claudeIntegration.state
              }`}
            >
              {claudeIntegration.lastQuotaObservedAt !== null
                ? "Active"
                : claudeIntegration.lastObservedAt !== null
                  ? "Connected"
                : claudeIntegration.state === "configured"
                  ? "Restart required"
                  : claudeIntegration.state === "conflict"
                    ? "Status line conflict"
                    : claudeIntegration.state === "misconfigured"
                      ? "Needs attention"
                      : "Off"}
            </span>
          )}
        </header>

        {claudeIntegration ? (
          <>
            <div className="settings-control-row">
              <div>
                <strong>Subscription window observer</strong>
                <p>
                  {claudeIntegration.lastQuotaObservedAt !== null
                    ? `${claudeSources.length} Claude quota ${claudeSources.length === 1 ? "window" : "windows"} observed · last update ${formatRelativeTime(claudeIntegration.lastQuotaObservedAt)}.`
                    : claudeIntegration.lastObservedAt !== null
                      ? `Claude Code is connected · last heartbeat ${formatRelativeTime(claudeIntegration.lastObservedAt)}. Send a prompt from a subscribed account to receive quota.`
                    : claudeIntegration.issue ??
                      (claudeIntegration.installed
                        ? "Quit and reopen Claude Desktop or the CLI, then start a new Code task. AQM has not received a heartbeat yet."
                        : "Install the observer to add Claude's 5-hour and weekly subscription windows automatically.")}
                </p>
              </div>
              <button
                className={`protection-toggle ${claudeIntegration.installed ? "enabled" : ""}`}
                type="button"
                disabled={claudeBusy || claudeIntegration.state === "conflict"}
                onClick={() => onClaudeIntegration(!claudeIntegration.installed)}
                role="switch"
                aria-checked={claudeIntegration.installed}
              >
                <i />
                {claudeBusy
                  ? "Updating…"
                  : claudeIntegration.installed
                    ? "Installed"
                    : "Off"}
              </button>
            </div>
            {claudeIntegration.state === "conflict" && (
              <div className="settings-callout warning" role="alert">
                <Icon name="activity" size={18} />
                <div>
                  <strong>Your existing Claude status line was not changed</strong>
                  <p>
                    Claude supports one status-line command. Remove or combine the
                    existing command manually before enabling AQM.
                  </p>
                </div>
              </div>
            )}
            <div className="settings-path">
              <span>Claude configuration</span>
              <code>{claudeIntegration.configPath || "Unavailable"}</code>
            </div>
          </>
        ) : (
          <div className="settings-loading">Inspecting Claude Code settings…</div>
        )}
      </section>

      <section
        className="settings-card integration-health-card"
        hidden={settingsTab !== "codex"}
      >
        <header className="settings-card-header">
          <span className="settings-card-icon">
            <Icon name="activity" size={20} />
          </span>
          <div>
            <h2>Codex integration health</h2>
            <p>
              Verify quota sync, Desktop attribution, mappings, and protection.
            </p>
          </div>
          <div className="integration-health-actions">
            <span
              className={`settings-status ${
                integrationHealthy
                  ? "active"
                  : integrationNeedsAttention
                    ? "misconfigured"
                    : "configured"
              }`}
            >
              {integrationHealthy
                ? "Ready"
                : integrationNeedsAttention
                  ? "Needs attention"
                  : "Passive only"}
            </span>
            <button
              className="button primary small"
              type="button"
              disabled={checking}
              onClick={onCheck}
            >
              <Icon
                className={checking ? "spin" : undefined}
                name="refresh"
                size={15}
              />
              {checking ? "Checking…" : "Check now"}
            </button>
          </div>
        </header>

        <div className="integration-health-list">
          <IntegrationHealthRow
            label="Provider checkpoint"
            tone={
              syncIssue
                ? "warning"
                : codexSource?.lastSyncedAt
                  ? "healthy"
                  : "muted"
            }
            value={
              syncIssue ??
              (codexSource?.lastSyncedAt
                ? `Synced ${formatRelativeTime(codexSource.lastSyncedAt)}`
                : "Not checked yet")
            }
          />
          <IntegrationHealthRow
            label="Desktop attribution"
            tone={
              desktopTracking?.status === "unavailable"
                ? "warning"
                : desktopHealthy
                  ? "healthy"
                  : "muted"
            }
            value={desktopTrackingLabel(desktopTracking)}
          />
          <IntegrationHealthRow
            label="Turn recovery"
            tone={
              (turnHealth?.staleCount ?? 0) > 0 ||
              (turnHealth?.contendedCount ?? 0) > 0
                ? "warning"
                : (turnHealth?.pendingCount ?? 0) > 0
                  ? "muted"
                  : "healthy"
            }
            value={turnRecoveryLabel(turnHealth)}
          />
          <IntegrationHealthRow
            label="Workspace mappings"
            tone={workspaceCount > 0 ? "healthy" : "warning"}
            value={
              workspaceCount > 0
                ? `${workspaceCount} allocated ${workspaceCount === 1 ? "folder" : "folders"}`
                : "No allocated folders"
            }
          />
          <IntegrationHealthRow
            label="Prompt gate"
            tone={
              verifiedAt !== null
                ? "healthy"
                : protection?.state === "misconfigured"
                  ? "warning"
                  : "muted"
            }
            value={promptGateHealthLabel(protection, observedAt, verifiedAt)}
          />
        </div>
      </section>

      <section className="settings-card" hidden={settingsTab !== "codex"}>
        <header className="settings-card-header">
          <span className="settings-card-icon">
            <Icon name="shield" size={20} />
          </span>
          <div>
            <h2>Codex Desktop protection</h2>
            <p>Check every new Codex prompt against workspace capacity.</p>
          </div>
          {protection && (
            <span
              className={`settings-status ${
                verifiedAt !== null ? "active" : protection.state
              }`}
            >
              {statusLabel(protection, observedAt, verifiedAt)}
            </span>
          )}
        </header>

        {protection ? (
          <>
            <div className="settings-control-row">
              <div>
                <strong>Workspace prompt gate</strong>
                <p>
                  {protection.state === "configured"
                    ? verifiedAt !== null
                      ? `Agent Quota Manager observed a Codex prompt decision ${formatRelativeTime(
                          verifiedAt,
                        )}.`
                      : observedAt !== null
                        ? `Codex delivered a prompt hook ${formatRelativeTime(
                            observedAt,
                          )}, but the latest check did not produce an enforceable quota decision.`
                        : "Agent Quota Manager is installed, but Codex has not delivered a prompt hook yet."
                    : protection.state === "misconfigured"
                      ? protection.issue
                      : "Passive usage tracking stays available, but prompts are not blocked."}
                </p>
              </div>
              <div className="protection-actions">
                {protection.state === "misconfigured" &&
                  protection.hasAqmHooks && (
                    <button
                      className="button subtle small"
                      type="button"
                      disabled={busy}
                      onClick={() => onProtection(true)}
                    >
                      Repair
                    </button>
                  )}
                <button
                  className={`protection-toggle ${
                    protection.installed ? "enabled" : ""
                  }`}
                  type="button"
                  disabled={busy}
                  onClick={() =>
                    onProtection(
                      protection.state === "misconfigured"
                        ? !protection.hasAqmHooks
                        : !protection.installed,
                    )
                  }
                  role="switch"
                  aria-checked={protection.installed}
                  aria-label={
                    protection.installed || protection.hasAqmHooks
                      ? "Turn off Codex Desktop protection"
                      : "Turn on Codex Desktop protection"
                  }
                >
                  <i />
                  {busy
                    ? "Updating…"
                    : protection.installed
                      ? "Installed"
                      : protection.hasAqmHooks
                        ? "Turn off"
                        : "Off"}
                </button>
              </div>
            </div>

            {protection.installed && observedAt === null && (
              <div className="settings-callout warning" role="alert">
                <Icon name="activity" size={18} />
                <div>
                  <strong>Finish protection</strong>
                  <p>Keep using this task.</p>
                  <ol>
                    <li>
                      In <b>Codex Settings → Hooks</b>, trust and enable{" "}
                      <code>UserPromptSubmit</code> and <code>Stop</code>.
                    </li>
                    <li>
                      Send the next prompt here, then refresh Agent Quota Manager.
                    </li>
                  </ol>
                </div>
              </div>
            )}

            {observedAt !== null && verifiedAt === null && (
              <div className="settings-callout warning" role="alert">
                <Icon name="activity" size={18} />
                <div>
                  <strong>Hook connected, enforcement is degraded</strong>
                  <p>
                    Codex reached Agent Quota Manager {formatRelativeTime(observedAt)}.
                    The latest prompt was not given an enforceable quota decision
                    {protection.lastHookIssue
                      ? `: ${protection.lastHookIssue}`
                      : "."}
                  </p>
                  <p>
                    Agent Quota Manager will retry automatically. Until a decision
                    succeeds, allocations are planned rather than enforced.
                  </p>
                </div>
              </div>
            )}

            {verifiedAt !== null && (
              <div className="settings-callout" role="status">
                <Icon name="check" size={18} />
                <div>
                  <strong>Protection observed in Codex Desktop</strong>
                  <p>
                    The prompt gate last returned a decision{" "}
                    {formatRelativeTime(verifiedAt)}.
                  </p>
                </div>
              </div>
            )}

            <div className="settings-path">
              <span>Hook configuration</span>
              <code>{protection.configPath || "Unavailable"}</code>
            </div>
          </>
        ) : (
          <div className="settings-loading">Inspecting local hook configuration…</div>
        )}
      </section>

      <section
        className="settings-decisions-section"
        hidden={settingsTab !== "activity"}
      >
        <header className="settings-decisions-header">
          <div>
            <h2>Recent protection decisions</h2>
            <p>Up to 100 latest prompts admitted or blocked by the workspace gate.</p>
          </div>
          <div className="settings-decisions-controls">
            <div className="decision-range" aria-label="Decision time range">
              {DECISION_RANGES.map((range) => (
                <button
                  className={decisionRange === range.value ? "active" : ""}
                  key={range.value}
                  type="button"
                  aria-pressed={decisionRange === range.value}
                  onClick={() => setDecisionRange(range.value)}
                >
                  {range.label}
                </button>
              ))}
            </div>
            <div className="decision-counts" aria-label="Filtered decision totals">
              <span className="allowed">{allowedDecisionCount} allowed</span>
              <span className="blocked">{blockedDecisionCount} blocked</span>
            </div>
          </div>
        </header>

        {visibleEvents.length > 0 ? (
          <div className="settings-events">
            {visibleEvents.map((event) => (
              <article
                className={event.outcome}
                key={`${event.occurredAt}-${event.canonicalPath}-${event.outcome}`}
              >
                <span className={`decision-badge ${event.outcome}`}>
                  {event.outcome === "allowed" ? "Allowed" : "Blocked"}
                </span>
                <div>
                  <strong>
                    {event.workspaceName ?? folderName(event.canonicalPath)}
                  </strong>
                  <span>{event.reason}</span>
                </div>
                <small>{formatRelativeTime(event.occurredAt)}</small>
              </article>
            ))}
          </div>
        ) : (
          <div className="settings-empty">
            No protection decisions in the last {selectedDecisionRange.label}.
          </div>
        )}
      </section>

      <footer
        className="settings-privacy-note"
        hidden={settingsTab !== "general"}
      >
        <Icon name="database" size={17} />
        <p>
          <strong>Local-first.</strong> Quota state and up to 100 recent decisions
          stay on this device. Prompts, responses, source code, and provider
          credentials are not stored.
        </p>
      </footer>
    </>
  );
}

type IntegrationHealthRowProps = {
  label: string;
  value: string;
  tone: "healthy" | "warning" | "muted";
};

function IntegrationHealthRow({
  label,
  value,
  tone,
}: IntegrationHealthRowProps) {
  return (
    <div className="integration-health-row">
      <i className={tone} aria-hidden="true" />
      <strong>{label}</strong>
      <span>{value}</span>
    </div>
  );
}

function desktopTrackingLabel(
  tracking: CodexSyncResult["desktopTracking"],
): string {
  if (!tracking) {
    return "Run a check to inspect Desktop metadata";
  }
  if (tracking.message) {
    return tracking.message;
  }
  switch (tracking.status) {
    case "baseline_established":
      return `Baseline ready · ${tracking.observedThreads} observed ${tracking.observedThreads === 1 ? "task" : "tasks"}`;
    case "no_activity":
      return `Ready · ${tracking.observedThreads} observed ${tracking.observedThreads === 1 ? "task" : "tasks"}`;
    case "pending_provider_delta":
      return "Activity found · waiting for provider quota movement";
    case "attributed":
      return `${tracking.attributedAmount}% attributed on the latest check`;
    case "ambiguous":
      return "Scanner ready · latest activity remained unassigned";
    case "window_rolled_over":
      return "Ready · baseline moved to the new quota window";
    case "unavailable":
      return "Codex Desktop metadata is unavailable";
  }
}

function promptGateHealthLabel(
  protection: CodexProtectionStatus | null,
  observedAt: number | null,
  verifiedAt: number | null,
): string {
  if (!protection) {
    return "Inspecting hook configuration";
  }
  if (verifiedAt !== null) {
    return `Active · decision observed ${formatRelativeTime(verifiedAt)}`;
  }
  if (protection.state === "misconfigured") {
    return protection.issue ?? "Hook configuration needs repair";
  }
  if (observedAt !== null) {
    return "Connected · latest hook was not enforceable";
  }
  if (protection.installed) {
    return "Installed · trust hooks and send a test prompt";
  }
  return "Off · passive tracking only";
}

function turnRecoveryLabel(
  health: QuotaSourceSummary["turnHealth"],
): string {
  if (!health || health.pendingCount === 0) {
    return "No pending Codex turns";
  }
  if (health.staleCount > 0) {
    return `${health.staleCount} stale ${health.staleCount === 1 ? "turn" : "turns"}; the next prompt recovers them conservatively`;
  }
  if (health.contendedCount > 0) {
    return `${health.pendingCount} pending; overlapping usage will remain unassigned`;
  }
  const age = health.oldestStartedAt
    ? ` since ${formatRelativeTime(health.oldestStartedAt)}`
    : "";
  return `${health.pendingCount} turn ${health.pendingCount === 1 ? "is" : "are"} awaiting reconciliation${age}`;
}
