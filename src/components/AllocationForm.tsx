import { useState, type FormEvent } from "react";
import type {
  PolicySummary,
  ScopeSummary,
  WorkspaceBudgetInput,
  AllocationSnapshot,
} from "../types";
import { PercentageControl } from "./PercentageControl";
import { PolicyThresholdControl } from "./PolicyThresholdControl";

type AllocationFormProps = {
  allocations?: AllocationSnapshot[];
  scope: ScopeSummary;
  currentAmount: number;
  currentPolicy: PolicySummary;
  maxAmount: number;
  unit: string;
  submitting: boolean;
  onSubmit: (input: WorkspaceBudgetInput & { reallocateFromScopeId: string | null }) => Promise<void>;
  onResetPolicy: () => Promise<void>;
};

export function AllocationForm({
  allocations = [],
  scope,
  currentAmount,
  currentPolicy,
  maxAmount,
  unit,
  submitting,
  onSubmit,
  onResetPolicy,
}: AllocationFormProps) {
  const [amount, setAmount] = useState(String(currentAmount));
  const [donorId, setDonorId] = useState("");
  const donors = allocations.filter((item) => item.scopeId !== scope.id && item.limit > 0);
  const donor = donors.find((item) => item.scopeId === donorId);
  const effectiveMax = maxAmount + (donor?.limit ?? 0);
  const transfer = Math.max(0, Number(amount) - maxAmount);
  const [warnAt, setWarnAt] = useState(
    formatBasisPoints(currentPolicy.warnAtBasisPoints),
  );
  const [stopAt, setStopAt] = useState(
    formatBasisPoints(currentPolicy.stopAtBasisPoints),
  );
  const [validation, setValidation] = useState<string | null>(null);

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (!Number.isFinite(Number(amount)) || amount.trim() === "" || Number(amount) < 0 || Number(amount) > effectiveMax) {
      setValidation("Choose a valid budget and a project with enough quota to transfer.");
      return;
    }
    const policy = {
      warnAtBasisPoints: parsePercent(warnAt),
      confirmAtBasisPoints: null,
      stopAtBasisPoints: parsePercent(stopAt),
    };
    const thresholds = [
      policy.warnAtBasisPoints,
      policy.stopAtBasisPoints,
    ].filter((value): value is number => value !== null);
    if (
      thresholds.some((value) => value < 1 || value > 10_000) ||
      thresholds.some((value, index) => index > 0 && thresholds[index - 1] > value)
    ) {
      setValidation(
        "Thresholds must be between 0.01% and 100%, ordered warn ≤ stop.",
      );
      return;
    }
    setValidation(null);
    await onSubmit({ amount: Number(amount), ...policy, reallocateFromScopeId: transfer > 0 ? donorId : null });
  }

  return (
    <form className="modal-form" onSubmit={handleSubmit}>
      <div className="selected-scope">
        <span>{scope.kind}</span>
        <strong>{scope.displayName}</strong>
      </div>
      {unit === "percent" && donors.length > 0 && (
        <label className="field">
          <span>Increase budget using</span>
          <select value={donorId} onChange={(event) => {
            const id = event.target.value;
            setDonorId(id);
            const limit = maxAmount + (donors.find((item) => item.scopeId === id)?.limit ?? 0);
            if (Number(amount) > limit) setAmount(String(limit));
          }}>
            <option value="">Unassigned quota ({Math.max(0, maxAmount - currentAmount)}% available)</option>
            {donors.map((item) => <option key={item.scopeId} value={item.scopeId}>{item.displayName} · {item.limit}% allocated</option>)}
          </select>
        </label>
      )}
      {unit === "percent" ? (
        <PercentageControl
          label="Quota limit"
          value={amount}
          onChange={setAmount}
          min={0}
          max={effectiveMax}
          step={1}
          autoFocus
        />
      ) : (
        <label className="field">
          <span>Quota limit</span>
          <div className="input-with-suffix">
            <input
              autoFocus
              type="number"
              min="0"
              step="1"
              value={amount}
              onChange={(event) => setAmount(event.currentTarget.value)}
              required
            />
            <span>{unit.split("_").join(" ")}</span>
          </div>
        </label>
      )}
      <p className="form-help">
        This limit is a share of the full provider window. Actual availability
        is also capped by the provider quota remaining now.
      </p>
      {donor && transfer > 0 && (
        <div className="form-note warm" role="status">
          <strong>{scope.displayName}: {currentAmount}% → {amount}%</strong><br />
          {donor.displayName}: {donor.limit}% → {donor.limit - transfer}%<br />
          Moves {transfer}% of weekly quota when you save. Existing usage is kept.
        </div>
      )}
      <fieldset className="policy-fields">
        <legend>Managed session policy</legend>
        <p className="form-help">
          Warn continues with a notice. Stop refuses the next managed launch or
          protected prompt. Clear a threshold to disable it.
        </p>
        <PolicyThresholdControl
          warnAt={warnAt}
          stopAt={stopAt}
          onWarnChange={setWarnAt}
          onStopChange={setStopAt}
        />
      </fieldset>
      {validation && <p className="form-error">{validation}</p>}
      <div className="modal-actions">
        {currentPolicy.customized && (
          <button
            className="button outline"
            type="button"
            disabled={submitting}
            onClick={() => void onResetPolicy()}
          >
            Use defaults
          </button>
        )}
        <button className="button primary" type="submit" disabled={submitting}>
          {submitting ? "Saving…" : "Save budget"}
        </button>
      </div>
    </form>
  );
}

function formatBasisPoints(value: number | null): string {
  return value === null ? "" : String(value / 100);
}

function parsePercent(value: string): number | null {
  const trimmed = value.trim();
  return trimmed === "" ? null : Math.round(Number(trimmed) * 100);
}
