import { useState, type FormEvent } from "react";
import type { ScopeInput, ScopeKind, ScopeSummary } from "../types";
import { Icon } from "./Icon";

type ScopeFormProps = {
  parents: ScopeSummary[];
  unit: string;
  submitting: boolean;
  onSubmit: (input: ScopeInput) => Promise<void>;
};

const kinds: Array<{
  value: Exclude<ScopeKind, "reserve">;
  label: string;
  icon: "folder" | "repository" | "task";
}> = [
  { value: "project", label: "Project", icon: "folder" },
  { value: "repository", label: "Repository", icon: "repository" },
  { value: "task", label: "Task", icon: "task" },
];

export function ScopeForm({
  parents,
  unit,
  submitting,
  onSubmit,
}: ScopeFormProps) {
  const [displayName, setDisplayName] = useState("");
  const [kind, setKind] = useState<Exclude<ScopeKind, "reserve">>("project");
  const [parentId, setParentId] = useState("");
  const [allocation, setAllocation] = useState("");

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    await onSubmit({
      displayName,
      kind,
      parentId: kind === "task" ? parentId || null : null,
      allocation: allocation ? Number(allocation) : 0,
    });
  }

  return (
    <form className="modal-form" onSubmit={handleSubmit}>
      <div className="kind-options">
        {kinds.map((option) => (
          <button
            className={`kind-option ${kind === option.value ? "selected" : ""}`}
            type="button"
            key={option.value}
            onClick={() => {
              setKind(option.value);
              if (option.value !== "task") {
                setParentId("");
              }
            }}
          >
            <Icon name={option.icon} size={19} />
            <span>{option.label}</span>
          </button>
        ))}
      </div>

      <label className="field">
        <span>Name</span>
        <input
          autoFocus
          value={displayName}
          onChange={(event) => setDisplayName(event.currentTarget.value)}
          placeholder={
            kind === "task"
              ? "e.g. Authentication refactor"
              : "e.g. Customer portal"
          }
          required
        />
      </label>

      {kind === "task" && (
        <label className="field">
          <span>Parent allocation</span>
          <select
            value={parentId}
            onChange={(event) => setParentId(event.currentTarget.value)}
            required
          >
            <option value="">Choose a project or repository</option>
            {parents.map((parent) => (
              <option value={parent.id} key={parent.id}>
                {parent.displayName}
              </option>
            ))}
          </select>
          {parents.length === 0 && (
            <small>Create and allocate a project before adding a task.</small>
          )}
        </label>
      )}

      <label className="field">
        <span>Allocation</span>
        <div className="input-with-suffix">
          <input
            type="number"
            min="1"
            step="1"
            value={allocation}
            onChange={(event) => setAllocation(event.currentTarget.value)}
            placeholder="20"
            required
          />
          <span>{unit.split("_").join(" ")}</span>
        </div>
        <small>The operation is saved atomically with the new scope.</small>
      </label>

      <button
        className="button primary wide"
        type="submit"
        disabled={submitting || (kind === "task" && parents.length === 0)}
      >
        {submitting ? "Adding…" : "Add to workspace"}
        {!submitting && <Icon name="arrow-right" size={18} />}
      </button>
    </form>
  );
}
