import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
} from "react";
import { detectCodexQuota, getErrorMessage } from "../lib/api";
import type {
  CodexDetection,
  DetectedQuotaWindow,
  QuotaSourceInput,
} from "../types";
import { Icon } from "./Icon";
import { ProviderLogo } from "./ProviderLogo";

type SourceSetupFormProps = {
  onSubmit: (input: QuotaSourceInput) => Promise<void>;
  submitting: boolean;
  submitLabel?: string;
};

const providerOptions = ["Codex", "Other"] as const;

function toLocalDateTime(date: Date): string {
  const local = new Date(date.getTime() - date.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 16);
}

function formatReset(timestamp: number): string {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(timestamp));
}

function formatPlan(planType: string | null): string {
  if (!planType) {
    return "ChatGPT subscription";
  }

  const labels: Record<string, string> = {
    free: "Free",
    go: "Go",
    plus: "Plus",
    pro: "Pro",
    prolite: "Pro Lite",
    team: "Team",
    self_serve_business_usage_based: "Business",
    business: "Business",
    enterprise_cbp_usage_based: "Enterprise",
    enterprise: "Enterprise",
    edu: "Edu",
    unknown: "ChatGPT",
  };

  return `${labels[planType] ?? planType} subscription`;
}

export function SourceSetupForm({
  onSubmit,
  submitting,
  submitLabel = "Create local workspace",
}: SourceSetupFormProps) {
  const defaultReset = useMemo(
    () => toLocalDateTime(new Date(Date.now() + 7 * 24 * 60 * 60 * 1_000)),
    [],
  );
  const [provider, setProvider] =
    useState<(typeof providerOptions)[number]>("Codex");
  const [customProvider, setCustomProvider] = useState("");
  const [poolName, setPoolName] = useState("Weekly allowance");
  const [capacity, setCapacity] = useState("100");
  const [unit, setUnit] = useState("percent");
  const [resetAt, setResetAt] = useState(defaultReset);
  const [manual, setManual] = useState(false);
  const [detecting, setDetecting] = useState(true);
  const [detection, setDetection] = useState<CodexDetection | null>(null);
  const [detectionError, setDetectionError] = useState<string | null>(null);
  const [selectedWindowId, setSelectedWindowId] = useState<string | null>(null);
  const initialDetectionStarted = useRef(false);

  const runDetection = useCallback(async () => {
    setDetecting(true);
    setDetectionError(null);

    try {
      const result = await detectCodexQuota();
      setDetection(result);
      setSelectedWindowId(result.windows[0]?.id ?? null);
    } catch (reason) {
      setDetection(null);
      setDetectionError(getErrorMessage(reason));
    } finally {
      setDetecting(false);
    }
  }, []);

  useEffect(() => {
    if (initialDetectionStarted.current) {
      return;
    }
    initialDetectionStarted.current = true;
    void runDetection();
  }, [runDetection]);

  const selectedWindow =
    detection?.windows.find((window) => window.id === selectedWindowId) ?? null;

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();

    if (!manual && detection?.status === "detected" && selectedWindow) {
      await onSubmit({
        providerDisplayName: detection.providerDisplayName,
        accountDisplayName: formatPlan(detection.planType),
        poolDisplayName: selectedWindow.displayName,
        capacity: selectedWindow.capacity,
        unit: selectedWindow.unit,
        startsAt: selectedWindow.startsAt,
        endsAt: selectedWindow.endsAt,
        providerSnapshot: {
          adapter: "codex_app_server",
          remoteLimitId: detection.providerId,
          remoteWindowKind: selectedWindow.kind,
          used: selectedWindow.used,
          observedAt: Date.now(),
          resetsAt: selectedWindow.endsAt,
        },
      });
      return;
    }

    const providerDisplayName =
      provider === "Other" ? customProvider.trim() : provider;
    const endsAt = new Date(resetAt).getTime();

    if (!providerDisplayName || !Number.isFinite(endsAt)) {
      return;
    }

    await onSubmit({
      providerDisplayName,
      poolDisplayName: poolName,
      capacity: Number(capacity),
      unit,
      startsAt: Date.now(),
      endsAt,
    });
  }

  return (
    <form className="setup-form" onSubmit={handleSubmit}>
      {!manual && (
        <CodexDetectionPanel
          detection={detection}
          detectionError={detectionError}
          detecting={detecting}
          selectedWindowId={selectedWindowId}
          onSelectWindow={setSelectedWindowId}
          onRetry={() => void runDetection()}
        />
      )}

      {manual && (
        <>
          <fieldset className="field-group">
            <legend>Subscription</legend>
            <div className="provider-options" role="radiogroup">
              {providerOptions.map((option) => (
                <button
                  className={`provider-option ${provider === option ? "selected" : ""}`}
                  type="button"
                  role="radio"
                  aria-checked={provider === option}
                  key={option}
                  onClick={() => setProvider(option)}
                >
                  <ProviderLogo
                    className="provider-option-mark"
                    providerName={option}
                    fallback={option === "Other" ? "+" : undefined}
                  />
                  {option}
                  {provider === option && <Icon name="check" size={16} />}
                </button>
              ))}
            </div>
          </fieldset>

          {provider === "Other" && (
            <label className="field">
              <span>Provider name</span>
              <input
                autoFocus
                value={customProvider}
                onChange={(event) => setCustomProvider(event.currentTarget.value)}
                placeholder="e.g. Cursor"
                required
              />
            </label>
          )}

          <div className="form-grid">
            <label className="field">
              <span>Quota window</span>
              <input
                value={poolName}
                onChange={(event) => setPoolName(event.currentTarget.value)}
                placeholder="Weekly allowance"
                required
              />
            </label>
            <label className="field">
              <span>Resets at</span>
              <input
                type="datetime-local"
                value={resetAt}
                min={toLocalDateTime(new Date())}
                onChange={(event) => setResetAt(event.currentTarget.value)}
                required
              />
            </label>
          </div>

          <div className="form-grid quota-fields">
            <label className="field">
              <span>Total allowance</span>
              <input
                type="number"
                min="1"
                step="1"
                value={capacity}
                onChange={(event) => setCapacity(event.currentTarget.value)}
                required
              />
            </label>
            <label className="field">
              <span>Unit</span>
              <select
                value={unit}
                onChange={(event) => setUnit(event.currentTarget.value)}
              >
                <option value="percent">percent</option>
                <option value="quota_points">quota points</option>
                <option value="requests">requests</option>
                <option value="hours">hours</option>
              </select>
            </label>
          </div>

          <div className="form-note warm">
            <Icon name="shield" size={18} />
            <p>
              Manual allowances are local estimates. No provider credentials or
              source code are stored.
            </p>
          </div>
        </>
      )}

      {!manual && detection?.status === "detected" && selectedWindow && (
        <div className="form-note">
          <Icon name="shield" size={18} />
          <p>
            Read from the official Codex App Server. Only quota metadata is
            imported; credentials, prompts, and source code stay untouched.
          </p>
        </div>
      )}

      <button
        className="button primary wide"
        type="submit"
        disabled={
          submitting ||
          (!manual &&
            (detecting ||
              detection?.status !== "detected" ||
              selectedWindow === null))
        }
      >
        {submitting ? "Creating…" : submitLabel}
        {!submitting && <Icon name="arrow-right" size={18} />}
      </button>

      <button
        className="manual-toggle"
        type="button"
        onClick={() => setManual((current) => !current)}
      >
        {manual ? "Back to automatic detection" : "Set up manually instead"}
      </button>
    </form>
  );
}

function CodexDetectionPanel({
  detection,
  detectionError,
  detecting,
  selectedWindowId,
  onSelectWindow,
  onRetry,
}: {
  detection: CodexDetection | null;
  detectionError: string | null;
  detecting: boolean;
  selectedWindowId: string | null;
  onSelectWindow: (id: string) => void;
  onRetry: () => void;
}) {
  if (detecting) {
    return (
      <div className="detection-panel detecting" aria-live="polite">
        <span className="detection-mark">
          <Icon className="spin" name="refresh" size={18} />
        </span>
        <div>
          <strong>Detecting Codex…</strong>
          <p>Reading subscription rate-limit metadata from this device.</p>
        </div>
      </div>
    );
  }

  if (!detection || detection.status !== "detected") {
    return (
      <div className="detection-panel unavailable" role="status">
        <span className="detection-mark">
          <Icon name="activity" size={19} />
        </span>
        <div>
          <strong>Codex quota not detected</strong>
          <p>
            {detectionError ??
              detection?.message ??
              "Automatic detection is unavailable right now."}
          </p>
          <button type="button" onClick={onRetry}>
            <Icon name="refresh" size={13} />
            Retry detection
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="detected-provider">
      <header>
        <ProviderLogo
          className="detected-provider-mark"
          providerName="Codex"
        />
        <div>
          <strong>Codex detected</strong>
          <p>{formatPlan(detection.planType)}</p>
        </div>
        <span className="connected-badge">
          <i />
          Connected
        </span>
      </header>

      <fieldset>
        <legend>Choose a quota window</legend>
        <div className="detected-windows" role="radiogroup">
          {detection.windows.map((window) => (
            <DetectedWindowOption
              key={window.id}
              window={window}
              selected={window.id === selectedWindowId}
              onSelect={() => onSelectWindow(window.id)}
            />
          ))}
        </div>
      </fieldset>
    </div>
  );
}

function DetectedWindowOption({
  window,
  selected,
  onSelect,
}: {
  window: DetectedQuotaWindow;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      className={`detected-window ${selected ? "selected" : ""}`}
      type="button"
      role="radio"
      aria-checked={selected}
      onClick={onSelect}
    >
      <span className="detected-window-heading">
        <strong>{window.displayName}</strong>
        {selected && (
          <span>
            <Icon name="check" size={12} />
          </span>
        )}
      </span>
      <span className="detected-window-reset">
        Resets {formatReset(window.endsAt)}
      </span>
      <span className="detected-window-meter">
        <i style={{ width: `${window.used}%` }} />
      </span>
      <span className="detected-window-usage">
        <b>{window.used}% used</b>
        <span>{window.remaining}% remaining</span>
      </span>
    </button>
  );
}
