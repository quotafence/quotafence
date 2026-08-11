import { useState, type FormEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { WorkspaceInput } from "../types";
import { Icon } from "./Icon";
import { PercentageControl } from "./PercentageControl";

type ScopeFormProps = {
  unit: string;
  submitting: boolean;
  maxAllocation: number;
  onSubmit: (input: WorkspaceInput) => Promise<void>;
};

export function ScopeForm({
  unit,
  submitting,
  maxAllocation,
  onSubmit,
}: ScopeFormProps) {
  const [displayName, setDisplayName] = useState("");
  const [allocation, setAllocation] = useState("");
  const [workspacePath, setWorkspacePath] = useState("");

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

      {unit === "percent" ? (
        <PercentageControl
          label="Allocation from total quota"
          value={allocation}
          onChange={setAllocation}
          min={1}
          max={maxAllocation}
          step={1}
        />
      ) : (
        <label className="field">
          <span>Allocation from total quota</span>
          <div className="input-with-suffix">
            <input
              type="number"
              min="1"
              step="1"
              value={allocation}
              onChange={(event) => setAllocation(event.currentTarget.value)}
              required
            />
            <span>{unit.split("_").join(" ")}</span>
          </div>
        </label>
      )}
      <p className="form-help">
        This is the folder's maximum share of the full provider window, not a
        share of the quota currently remaining.
      </p>

      <button
        className="button primary wide"
        type="submit"
        disabled={submitting || !workspacePath}
      >
        {submitting ? "Adding…" : "Add allocation"}
        {!submitting && <Icon name="arrow-right" size={18} />}
      </button>
    </form>
  );
}
