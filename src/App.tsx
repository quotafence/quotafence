import { useCallback, useEffect, useMemo, useState } from "react";
import "./App.css";
import { AllocationForm } from "./components/AllocationForm";
import { Dashboard } from "./components/Dashboard";
import { Icon } from "./components/Icon";
import { Modal } from "./components/Modal";
import { ScopeForm } from "./components/ScopeForm";
import { SourceSetupForm } from "./components/SourceSetupForm";
import { UsageForm } from "./components/UsageForm";
import {
  createAllocatedScope,
  createQuotaSource,
  getErrorMessage,
  getLocalState,
  recordUsage,
  setAllocation,
} from "./lib/api";
import type {
  LocalState,
  QuotaSourceInput,
  ScopeInput,
  ScopeSummary,
  UsageInput,
} from "./types";

type ModalState =
  | { type: "source" }
  | { type: "scope" }
  | { type: "allocation"; scope: ScopeSummary }
  | { type: "usage"; scopeId?: string }
  | null;

function LoadingScreen() {
  return (
    <main className="loading-screen">
      <div className="loading-mark">
        <Icon name="gauge" size={30} />
      </div>
      <strong>Agent Quota Manager</strong>
      <span>Opening your local ledger…</span>
      <i />
    </main>
  );
}

function WelcomeScreen({
  submitting,
  onSubmit,
}: {
  submitting: boolean;
  onSubmit: (input: QuotaSourceInput) => Promise<void>;
}) {
  return (
    <main className="welcome-screen">
      <section className="welcome-story">
        <div className="brand welcome-brand">
          <span className="brand-mark">
            <Icon name="gauge" size={22} />
          </span>
          <div>
            <strong>Agent Quota</strong>
            <span>Manager</span>
          </div>
        </div>

        <div className="welcome-copy">
          <p className="eyebrow light">Make shared quota intentional</p>
          <h1>
            Give every project
            <br />
            its <em>fair share.</em>
          </h1>
          <p>
            Allocate a coding-agent subscription across projects and tasks,
            then see exactly how much room remains.
          </p>
        </div>

        <div className="welcome-benefits">
          <div>
            <span>
              <Icon name="folder" size={19} />
            </span>
            <p>
              <strong>Project-scoped budgets</strong>
              Keep important work from losing quota to everything else.
            </p>
          </div>
          <div>
            <span>
              <Icon name="shield" size={19} />
            </span>
            <p>
              <strong>Honest local accounting</strong>
              Separate observed, estimated, and unattributed usage.
            </p>
          </div>
          <div>
            <span>
              <Icon name="database" size={19} />
            </span>
            <p>
              <strong>Private by default</strong>
              The ledger stays in SQLite on this device.
            </p>
          </div>
        </div>

        <footer>
          Open source · Apache 2.0
          <span />
          No cloud account required
        </footer>
      </section>

      <section className="welcome-setup">
        <div className="setup-card">
          <div className="step-indicator">
            <span>01</span>
            <i />
            <small>LOCAL SETUP</small>
          </div>
          <header>
            <p className="eyebrow">First quota source</p>
            <h2>Set up your allowance</h2>
            <p>
              Start with the quota your subscription exposes. You can add more
              sources later.
            </p>
          </header>
          <SourceSetupForm onSubmit={onSubmit} submitting={submitting} />
        </div>
      </section>
    </main>
  );
}

function App() {
  const [localState, setLocalState] = useState<LocalState | null>(null);
  const [initializing, setInitializing] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [modal, setModal] = useState<ModalState>(null);

  const loadState = useCallback(async (windowId: string | null = null) => {
    const nextState = await getLocalState(windowId);
    setLocalState(nextState);
    return nextState;
  }, []);

  useEffect(() => {
    loadState()
      .catch((reason) => setError(getErrorMessage(reason)))
      .finally(() => setInitializing(false));
  }, [loadState]);

  const selectedSource = useMemo(
    () =>
      localState?.sources.find(
        (source) => source.windowId === localState.selectedWindowId,
      ) ?? null,
    [localState],
  );

  const allocatedParents = useMemo(() => {
    if (!localState?.dashboard) {
      return [];
    }
    const allocatedIds = new Set(
      localState.dashboard.allocations.map((allocation) => allocation.scopeId),
    );
    return localState.scopes.filter(
      (scope) =>
        !scope.parentId && scope.kind !== "task" && allocatedIds.has(scope.id),
    );
  }, [localState]);

  async function runMutation(operation: () => Promise<void>, close = true) {
    setSubmitting(true);
    setError(null);
    try {
      await operation();
      await loadState(localState?.selectedWindowId ?? null);
      if (close) {
        setModal(null);
      }
    } catch (reason) {
      setError(getErrorMessage(reason));
    } finally {
      setSubmitting(false);
    }
  }

  async function handleCreateSource(input: QuotaSourceInput) {
    setSubmitting(true);
    setError(null);
    try {
      await createQuotaSource(input);
      await loadState();
      setModal(null);
    } catch (reason) {
      setError(getErrorMessage(reason));
    } finally {
      setSubmitting(false);
    }
  }

  async function handleRefresh() {
    setRefreshing(true);
    setError(null);
    try {
      await loadState(localState?.selectedWindowId ?? null);
    } catch (reason) {
      setError(getErrorMessage(reason));
    } finally {
      setRefreshing(false);
    }
  }

  async function handleSelectSource(windowId: string) {
    setRefreshing(true);
    setError(null);
    try {
      await loadState(windowId);
    } catch (reason) {
      setError(getErrorMessage(reason));
    } finally {
      setRefreshing(false);
    }
  }

  if (initializing) {
    return <LoadingScreen />;
  }

  if (!localState) {
    return (
      <main className="fatal-screen">
        <span>
          <Icon name="database" size={26} />
        </span>
        <h1>Could not open the local ledger</h1>
        <p>{error}</p>
        <button
          className="button dark"
          type="button"
          onClick={() => {
            setInitializing(true);
            setError(null);
            loadState()
              .catch((reason) => setError(getErrorMessage(reason)))
              .finally(() => setInitializing(false));
          }}
        >
          Try again
        </button>
      </main>
    );
  }

  if (localState.sources.length === 0) {
    return (
      <>
        <WelcomeScreen submitting={submitting} onSubmit={handleCreateSource} />
        {error && (
          <div className="error-toast" role="alert">
            <span>{error}</span>
            <button type="button" onClick={() => setError(null)} aria-label="Dismiss">
              <Icon name="x" size={17} />
            </button>
          </div>
        )}
      </>
    );
  }

  const dashboard = localState.dashboard;
  const unit = dashboard?.window.unit ?? selectedSource?.unit ?? "quota_points";
  const editingAllocation =
    modal?.type === "allocation"
      ? dashboard?.allocations.find(
          (allocation) => allocation.scopeId === modal.scope.id,
        )
      : undefined;

  return (
    <>
      <Dashboard
        state={localState}
        refreshing={refreshing}
        onSelectSource={handleSelectSource}
        onAddSource={() => setModal({ type: "source" })}
        onAddScope={() => setModal({ type: "scope" })}
        onEditAllocation={(scope) => setModal({ type: "allocation", scope })}
        onRecordUsage={(scopeId) => setModal({ type: "usage", scopeId })}
        onRefresh={handleRefresh}
      />

      {error && (
        <div className="error-toast" role="alert">
          <span>{error}</span>
          <button type="button" onClick={() => setError(null)} aria-label="Dismiss">
            <Icon name="x" size={17} />
          </button>
        </div>
      )}

      {modal?.type === "source" && (
        <Modal
          eyebrow="Manual subscription"
          title="Add a quota source"
          onClose={() => setModal(null)}
        >
          <SourceSetupForm
            onSubmit={handleCreateSource}
            submitting={submitting}
            submitLabel="Add quota source"
          />
        </Modal>
      )}

      {modal?.type === "scope" && dashboard && (
        <Modal
          eyebrow="Project budget"
          title="Create an allocation"
          onClose={() => setModal(null)}
        >
          <ScopeForm
            parents={allocatedParents}
            unit={unit}
            submitting={submitting}
            onSubmit={(input: ScopeInput) =>
              runMutation(() =>
                createAllocatedScope(input, dashboard.window.id, unit),
              )
            }
          />
        </Modal>
      )}

      {modal?.type === "allocation" && dashboard && (
        <Modal
          eyebrow="Quota limit"
          title="Adjust allocation"
          onClose={() => setModal(null)}
        >
          <AllocationForm
            scope={modal.scope}
            currentAmount={editingAllocation?.limit ?? 0}
            unit={unit}
            submitting={submitting}
            onSubmit={(amount) =>
              runMutation(() =>
                setAllocation(modal.scope.id, dashboard.window.id, amount, unit),
              )
            }
          />
        </Modal>
      )}

      {modal?.type === "usage" && dashboard && (
        <Modal
          eyebrow="Local observation"
          title="Record usage"
          onClose={() => setModal(null)}
        >
          <UsageForm
            scopes={localState.scopes.filter((scope) =>
              dashboard.allocations.some(
                (allocation) => allocation.scopeId === scope.id,
              ),
            )}
            initialScopeId={modal.scopeId}
            unit={unit}
            submitting={submitting}
            onSubmit={(input: UsageInput) =>
              runMutation(() =>
                recordUsage(input, dashboard.window.id, unit),
              )
            }
          />
        </Modal>
      )}
    </>
  );
}

export default App;
