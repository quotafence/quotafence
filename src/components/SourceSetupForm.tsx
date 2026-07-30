import { useMemo, useState, type FormEvent } from "react";
import type { QuotaSourceInput } from "../types";
import { Icon } from "./Icon";

type SourceSetupFormProps = {
  onSubmit: (input: QuotaSourceInput) => Promise<void>;
  submitting: boolean;
  submitLabel?: string;
};

const providerOptions = ["Codex", "Claude Code", "Other"] as const;

function toLocalDateTime(date: Date): string {
  const local = new Date(date.getTime() - date.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 16);
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

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
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
              <span className="provider-option-mark">
                {option === "Codex" ? "CX" : option === "Claude Code" ? "CL" : "+"}
              </span>
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
          <select value={unit} onChange={(event) => setUnit(event.currentTarget.value)}>
            <option value="percent">percent</option>
            <option value="quota_points">quota points</option>
            <option value="requests">requests</option>
            <option value="hours">hours</option>
          </select>
        </label>
      </div>

      <div className="form-note">
        <Icon name="shield" size={18} />
        <p>
          This is a manual local allowance. No provider credentials or source
          code are stored.
        </p>
      </div>

      <button className="button primary wide" type="submit" disabled={submitting}>
        {submitting ? "Creating…" : submitLabel}
        {!submitting && <Icon name="arrow-right" size={18} />}
      </button>
    </form>
  );
}
