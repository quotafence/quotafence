import { useState, type FormEvent } from "react";
import type {
  PolicySummary,
  ScopeSummary,
  WorkspaceBudgetInput,
} from "../types";
import { PercentageControl } from "./PercentageControl";

type AllocationFormProps = {
  scope: ScopeSummary;
  currentAmount: number;
  currentPolicy: PolicySummary;
  maxAmount: number;
  unit: string;
  submitting: boolean;
  onSubmit: (input: WorkspaceBudgetInput) => Promise<void>;
  onResetPolicy: () => Promise<void>;
};

export function AllocationForm({
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
  const warnValue = optionalNumber(warnAt);
  const confirmValue = optionalNumber(confirmAt);
  const stopValue = optionalNumber(stopAt);
  const policyFields = [
    {
      label: "Warn at",
      value: warnAt,
      setValue: setWarnAt,
      min: 0.01,
      max: confirmValue ?? stopValue ?? 100,
      helperText: confirmValue !== null
        ? `Warn cannot exceed Confirm (${formatPercent(confirmValue)}%).`
        : "Warn cannot exceed the next enabled threshold.",
    },
    {
      label: "Confirm at",
      value: confirmAt,
      setValue: setConfirmAt,
      min: warnValue ?? 0.01,
      max: stopValue ?? 100,
      helperText: [
        warnValue !== null
          ? `At least Warn (${formatPercent(warnValue)}%)`
          : "No Warn minimum",
        stopValue !== null
          ? `at most Stop (${formatPercent(stopValue)}%)`
          : "no Stop maximum",
      ].join(" · ") + ".",
    },
    {
      label: "Stop at",
      value: stopAt,
      setValue: setStopAt,
      min: confirmValue ?? warnValue ?? 0.01,
      max: 100,
      helperText: confirmValue !== null
        ? `Stop cannot be below Confirm (${formatPercent(confirmValue)}%).`
        : "Stop cannot be below the previous enabled threshold.",
    },
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
      {unit === "percent" ? (
        <PercentageControl
          label="Quota limit"
          value={amount}
          onChange={setAmount}
          min={0}
          max={maxAmount}
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
      <fieldset className="policy-fields">
        <legend>Managed session policy</legend>
        <p className="form-help">
          Leave a threshold blank to disable it. Stop refuses a new managed
          launch; it does not terminate unmanaged Codex work.
        </p>
        <div>
          {policyFields.map(
            ({ label, value, setValue, min, max, helperText }) => (
              <PercentageControl
                key={label}
                label={label}
                value={value}
                onChange={setValue}
                min={min}
                max={max}
                step={0.01}
                sliderMin={0}
                sliderMax={100}
                sliderStep={1}
                helperText={helperText}
                allowEmpty
              />
            ),
          )}
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

function optionalNumber(value: string): number | null {
  const parsed = Number(value);
  return value.trim() === "" || !Number.isFinite(parsed) ? null : parsed;
}

function formatPercent(value: number): string {
  return Number.isInteger(value)
    ? String(value)
    : String(Number(value.toFixed(2)));
}
