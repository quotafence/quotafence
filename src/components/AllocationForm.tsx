import { useState, type FormEvent } from "react";
import type { ScopeSummary } from "../types";

type AllocationFormProps = {
  scope: ScopeSummary;
  currentAmount: number;
  unit: string;
  submitting: boolean;
  onSubmit: (amount: number) => Promise<void>;
};

export function AllocationForm({
  scope,
  currentAmount,
  unit,
  submitting,
  onSubmit,
}: AllocationFormProps) {
  const [amount, setAmount] = useState(String(currentAmount));

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    await onSubmit(Number(amount));
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
      <button className="button primary wide" type="submit" disabled={submitting}>
        {submitting ? "Saving…" : "Save allocation"}
      </button>
    </form>
  );
}
