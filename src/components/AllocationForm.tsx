import { useState, type FormEvent } from "react";
import type {
  PolicySummary,
  ScopeSummary,
  WorkspaceBudgetInput,
} from "../types";

type AllocationFormProps = {
  scope: ScopeSummary;
  currentAmount: number;
  currentPolicy: PolicySummary;
  unit: string;
  submitting: boolean;
  onSubmit: (input: WorkspaceBudgetInput) => Promise<void>;
  onResetPolicy: () => Promise<void>;
};

export function AllocationForm({
  scope,
  currentAmount,
  currentPolicy,
  unit,
  submitting,
  onSubmit,
  onResetPolicy,
}: AllocationFormProps) {
  const [amount, setAmount] = useState(String(currentAmount));
  const [warnAt, setWarnAt] = useState(
    formatBasisPoints(currentPolicy.warnAtBasisPoints),
  );
  const [confirmAt, setConfirmAt] = useState(
    formatBasisPoints(currentPolicy.confirmAtBasisPoints),
  );
  const [stopAt, setStopAt] = useState(
    formatBasisPoints(currentPolicy.stopAtBasisPoints),
  );
  const [validation, setValidation] = useState<string | null>(null);
  const policyFields = [
    { label: "Warn", value: warnAt, setValue: setWarnAt },
    { label: "Confirm", value: confirmAt, setValue: setConfirmAt },
    { label: "Stop", value: stopAt, setValue: setStopAt },
  ];

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    const policy = {
      warnAtBasisPoints: parsePercent(warnAt),
      confirmAtBasisPoints: parsePercent(confirmAt),
      stopAtBasisPoints: parsePercent(stopAt),
    };
    const thresholds = [
      policy.warnAtBasisPoints,
      policy.confirmAtBasisPoints,
      policy.stopAtBasisPoints,
    ].filter((value): value is number => value !== null);
    if (
      thresholds.some((value) => value < 1 || value > 10_000) ||
      thresholds.some((value, index) => index > 0 && thresholds[index - 1] > value)
    ) {
      setValidation(
        "Thresholds must be between 0.01% and 100%, ordered warn ≤ confirm ≤ stop.",
      );
      return;
    }
    setValidation(null);
    await onSubmit({ amount: Number(amount), ...policy });
  }

  return (
    <form className="modal-form" onSubmit={handleSubmit}>
      <div className="selected-scope">
        <span>{scope.kind}</span>
        <strong>{scope.displayName}</strong>
      </div>
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
      <p className="form-help">
        This limit is a share of the full provider window. Actual availability
        is also capped by the provider quota remaining now.
      </p>
      <fieldset className="policy-fields">
        <legend>Managed session policy</legend>
        <p className="form-help">
          Leave a threshold blank to disable it. Stop refuses a new AQM-managed
          launch; it does not terminate unmanaged Codex work.
        </p>
        <div>
          {policyFields.map(({ label, value, setValue }) => (
            <label className="field" key={label}>
              <span>{label} at</span>
              <div className="input-with-suffix">
                <input
                  type="number"
                  min="0.01"
                  max="100"
                  step="0.01"
                  value={value}
                  onChange={(event) => setValue(event.currentTarget.value)}
                />
                <span>percent</span>
              </div>
            </label>
          ))}
        </div>
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
