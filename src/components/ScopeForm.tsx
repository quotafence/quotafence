import { useMemo, useState, type FormEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { AllocationSnapshot, WorkspaceInput } from "../types";
import { Icon } from "./Icon";
import { PercentageControl } from "./PercentageControl";

type ScopeFormProps = {
  unit: string;
  submitting: boolean;
  maxAllocation: number;
  allocations: AllocationSnapshot[];
  onSubmit: (input: WorkspaceInput) => Promise<void>;
};

export function ScopeForm({
  unit,
  submitting,
  maxAllocation,
  allocations,
  onSubmit,
}: ScopeFormProps) {
  const [displayName, setDisplayName] = useState("");
  const [allocation, setAllocation] = useState("");
  const [workspacePath, setWorkspacePath] = useState("");
  const [reallocateFromScopeId, setReallocateFromScopeId] = useState("");
  const selectedDonor = useMemo(
    () =>
      allocations.find(
        (candidate) => candidate.scopeId === reallocateFromScopeId,
      ) ?? null,
    [allocations, reallocateFromScopeId],
  );
  const effectiveMaxAllocation =
    maxAllocation + (selectedDonor?.limit ?? 0);
  const selectedAllocation = allocation ? Number(allocation) : 0;
  const transferAmount = Math.max(0, selectedAllocation - maxAllocation);

  async function chooseFolder() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Choose workspace folder",
    });
    if (!selected) {
      return;
    }

    setWorkspacePath(selected);
    if (!displayName.trim()) {
      const segments = selected.split(/[\\/]/).filter(Boolean);
      setDisplayName(segments[segments.length - 1] ?? "Workspace");
    }
  }

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    await onSubmit({
      displayName,
      allocation: allocation ? Number(allocation) : 0,
      workspacePath,
      reallocateFromScopeId: reallocateFromScopeId || null,
    });
  }

  return (
    <form className="modal-form" onSubmit={handleSubmit}>
      <div className="field">
        <span>Folder</span>
        <button
          className={`folder-picker ${workspacePath ? "selected" : ""}`}
          type="button"
          onClick={() => void chooseFolder()}
        >
          <Icon name="folder" size={19} />
          <span>
            <strong>
              {workspacePath ? "Workspace folder selected" : "Choose folder"}
            </strong>
            <small>{workspacePath || "Git and GitHub are not required."}</small>
          </span>
        </button>
      </div>

      <label className="field">
        <span>Name</span>
        <input
          autoFocus
          value={displayName}
          onChange={(event) => setDisplayName(event.currentTarget.value)}
          placeholder="Defaults to the selected folder name"
          required
        />
      </label>

      {allocations.length > 0 && (
        <label className="field reallocation-source-field">
          <span>Fund this weekly allocation from</span>
          <div className="select-with-icon">
            <Icon name="arrow-right" size={17} />
            <select
              value={reallocateFromScopeId}
              onChange={(event) => {
                const nextScopeId = event.currentTarget.value;
                setReallocateFromScopeId(nextScopeId);
                const nextDonor = allocations.find(
                  (candidate) => candidate.scopeId === nextScopeId,
                );
                const nextMax = maxAllocation + (nextDonor?.limit ?? 0);
                if (allocation && Number(allocation) > nextMax) {
                  setAllocation(String(nextMax));
                }
              }}
            >
              <option value="">
                Unassigned quota ({maxAllocation}% available)
              </option>
              {allocations.map((candidate) => (
                <option value={candidate.scopeId} key={candidate.scopeId}>
                  Take from {candidate.displayName} (up to {candidate.limit}%)
                </option>
              ))}
            </select>
            <Icon className="select-chevron" name="chevron-down" size={16} />
          </div>
          <small>
            {selectedDonor
              ? `Only the amount beyond the ${maxAllocation}% unassigned quota will be moved from ${selectedDonor.displayName}.`
              : maxAllocation > 0
                ? "Uses currently unassigned quota. Choose a workspace if you need more."
                : "All quota is allocated. Choose which workspace should give up quota."}
          </small>
        </label>
      )}

      {effectiveMaxAllocation > 0 && unit === "percent" ? (
        <PercentageControl
          label="Weekly allocation"
          value={allocation}
          onChange={setAllocation}
          min={1}
          max={effectiveMaxAllocation}
          step={1}
          helperText={
            selectedDonor
              ? transferAmount > 0
                ? `${selectedAllocation}% selected · move ${transferAmount}% from ${selectedDonor.displayName}, leaving it ${selectedDonor.limit - transferAmount}%`
                : `${selectedAllocation}% selected · fully covered by unassigned quota; ${selectedDonor.displayName} stays unchanged`
              : undefined
          }
        />
      ) : effectiveMaxAllocation > 0 ? (
        <label className="field">
          <span>Weekly allocation</span>
          <div className="input-with-suffix">
            <input
              type="number"
              min="1"
              max={effectiveMaxAllocation}
              step="1"
              value={allocation}
              onChange={(event) => setAllocation(event.currentTarget.value)}
              required
            />
            <span>{unit.split("_").join(" ")}</span>
          </div>
        </label>
      ) : (
        <div className="form-note warm reallocation-required" role="status">
          <Icon name="folder" size={17} />
          <p>Choose a workspace above to make quota available.</p>
        </div>
      )}
      <p className="form-help">
        This is the folder's maximum share of the weekly allowance. The 5-hour
        allowance remains a provider safety limit and is not allocated separately.
      </p>

      <button
        className="button primary wide"
        type="submit"
        disabled={
          submitting ||
          !workspacePath ||
          effectiveMaxAllocation < 1 ||
          !allocation ||
          Number(allocation) < 1 ||
          Number(allocation) > effectiveMaxAllocation
        }
      >
        {submitting ? "Adding…" : "Add allocation"}
        {!submitting && <Icon name="arrow-right" size={18} />}
      </button>
    </form>
  );
}
