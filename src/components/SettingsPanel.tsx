import type {
  CodexProtectionEvent,
  CodexProtectionStatus,
} from "../types";
import { Icon } from "./Icon";

export type ThemePreference = "system" | "light" | "dark";

type SettingsPanelProps = {
  protection: CodexProtectionStatus | null;
  events: CodexProtectionEvent[];
  busy: boolean;
  theme: ThemePreference;
  onThemeChange: (theme: ThemePreference) => void;
  onProtection: (enabled: boolean) => void;
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

function statusLabel(protection: CodexProtectionStatus): string {
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
  busy,
  theme,
  onThemeChange,
  onProtection,
}: SettingsPanelProps) {
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

      <section className="settings-card">
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

      <section className="settings-card">
        <header className="settings-card-header">
          <span className="settings-card-icon">
            <Icon name="shield" size={20} />
          </span>
          <div>
            <h2>Codex Desktop protection</h2>
            <p>Check every new Codex prompt against workspace capacity.</p>
          </div>
          {protection && (
            <span className={`settings-status ${protection.state}`}>
              {statusLabel(protection)}
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
                    ? "AQM hook files are installed, but Codex trust and enablement still require your review."
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

            {protection.installed && (
              <div className="settings-callout warning" role="alert">
                <Icon name="activity" size={18} />
                <div>
                  <strong>Protection is not active until you finish this</strong>
                  <p>
                    Codex can still run prompts without AQM protection until
                    all three hooks are trusted and switched on.
                  </p>
                  <ol>
                    <li>
                      Open <b>Settings → Hooks → User config</b>.
                    </li>
                    <li>
                      Review, trust, and switch on the AQM entries under{" "}
                      <code>UserPromptSubmit</code>, <code>Stop</code>, and{" "}
                      <code>SessionEnd</code>.
                    </li>
                    <li>Restart Codex, then start a new task.</li>
                  </ol>
                  <p>
                    AQM cannot currently verify Codex's trust state, so this
                    warning remains visible. Codex is the source of truth.
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

      <section className="settings-card">
        <header className="settings-card-header">
          <span className="settings-card-icon">
            <Icon name="activity" size={20} />
          </span>
          <div>
            <h2>Recent protection decisions</h2>
            <p>Latest prompts admitted or blocked by the workspace gate.</p>
          </div>
        </header>

        {events.length > 0 ? (
          <div className="settings-events">
            {events.map((event) => (
              <article
                key={`${event.occurredAt}-${event.canonicalPath}-${event.outcome}`}
              >
                <i className={event.outcome} />
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
            No prompt decisions recorded yet. Activity appears after protection
            is trusted and Codex starts a new prompt.
          </div>
        )}
      </section>

      <section className="settings-card settings-privacy">
        <span className="settings-card-icon">
          <Icon name="database" size={20} />
        </span>
        <div>
          <h2>Local data</h2>
          <p>
            AQM keeps quota state and up to 100 recent protection decisions on
            this device. Prompt text, responses, transcripts, source code, and
            provider credentials are not stored.
          </p>
        </div>
      </section>
    </>
  );
}
