import { useState, type FormEvent } from "react";
import type { ScopeSummary, UsageInput } from "../types";
import { Icon } from "./Icon";

type UsageFormProps = {
  scopes: ScopeSummary[];
  initialScopeId?: string;
  unit: string;
  submitting: boolean;
  onSubmit: (input: UsageInput) => Promise<void>;
};

export function UsageForm({
  scopes,
  initialScopeId,
  unit,
  submitting,
  onSubmit,
}: UsageFormProps) {
  const [scopeId, setScopeId] = useState(initialScopeId ?? "");
  const [amount, setAmount] = useState("");

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    await onSubmit({
      scopeId: scopeId || null,
      amount: Number(amount),
    });
  }

  return (
    <form className="modal-form" onSubmit={handleSubmit}>
      <div className="form-note warm">
        <Icon name="activity" size={18} />
        <p>
          Manual usage is marked as locally observed. Provider-confirmed
          reconciliation arrives with an adapter.
        </p>
      </div>
      <label className="field">
        <span>Attribute to</span>
        <select value={scopeId} onChange={(event) => setScopeId(event.currentTarget.value)}>
          <option value="">Unattributed provider usage</option>
          {scopes.map((scope) => (
            <option value={scope.id} key={scope.id}>
              {scope.displayName} · {scope.kind}
            </option>
          ))}
        </select>
      </label>
      <label className="field">
        <span>Amount used</span>
        <div className="input-with-suffix">
          <input
            autoFocus
            type="number"
            min="1"
            step="1"
            value={amount}
            onChange={(event) => setAmount(event.currentTarget.value)}
            placeholder="5"
            required
          />
          <span>{unit.split("_").join(" ")}</span>
        </div>
      </label>
      <button className="button primary wide" type="submit" disabled={submitting}>
        {submitting ? "Recording…" : "Record usage"}
      </button>
    </form>
  );
}
