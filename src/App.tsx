import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import "./App.css";
import { AllocationForm } from "./components/AllocationForm";
import { Dashboard } from "./components/Dashboard";
import { Icon } from "./components/Icon";
import { Modal } from "./components/Modal";
import { ScopeForm } from "./components/ScopeForm";
import { SourceSetupForm } from "./components/SourceSetupForm";
import {
  archiveQuotaSource,
  createAllocatedWorkspace,
  createQuotaSource,
  getErrorMessage,
  getCodexProtectionEvents,
  getCodexProtectionStatus,
  getLocalState,
  installCodexProtection,
  resetWorkspacePolicy,
  setAllocation,
  setAllocationPriorityOrder,
  setWorkspacePolicy,
  syncCodexQuota,
  uninstallCodexProtection,
} from "./lib/api";
import type {
  LocalState,
  CodexProtectionEvent,
  CodexProtectionStatus,
  QuotaSourceInput,
  QuotaSourceSummary,
  ScopeSummary,
  WorkspaceInput,
  WorkspaceBudgetInput,
} from "./types";

type ModalState =
  | { type: "source" }
  | { type: "scope" }
  | { type: "allocation"; scope: ScopeSummary }
  | { type: "remove-source"; source: QuotaSourceSummary }
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
            Give every workspace
            <br />
            its <em>fair share.</em>
          </h1>
          <p>
            Allocate a coding-agent subscription across local folders, then
            see exactly how much room remains.
          </p>
        </div>

        <div className="welcome-benefits">
          <div>
            <span>
              <Icon name="folder" size={19} />
            </span>
            <p>
              <strong>Folder-scoped budgets</strong>
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
  const [notice, setNotice] = useState<string | null>(null);
  const [modal, setModal] = useState<ModalState>(null);
  const [codexProtection, setCodexProtection] =
    useState<CodexProtectionStatus | null>(null);
  const [codexProtectionEvents, setCodexProtectionEvents] = useState<
    CodexProtectionEvent[]
  >([]);
  const [protectionBusy, setProtectionBusy] = useState(false);
  const [priorityBusy, setPriorityBusy] = useState(false);
  const initialSyncStarted = useRef(false);

  const loadState = useCallback(async (windowId: string | null = null) => {
    const nextState = await getLocalState(windowId);
    setLocalState(nextState);
    return nextState;
  }, []);

  useEffect(() => {
    async function initialize() {
      try {
        getCodexProtectionStatus()
          .then(setCodexProtection)
          .catch(() =>
            setCodexProtection({
              installed: false,
              hasAqmHooks: false,
              requiresReview: false,
              configPath: "",
              state: "misconfigured",
              issue: "AQM could not inspect the Codex hook configuration.",
            }),
          );
        getCodexProtectionEvents().then(setCodexProtectionEvents).catch(() => {
          setCodexProtectionEvents([]);
        });
        const nextState = await loadState();
        if (initialSyncStarted.current) {
          return;
        }
        initialSyncStarted.current = true;
        const source = nextState.sources.find(
          (candidate) => candidate.windowId === nextState.selectedWindowId,
        );
        if (source?.providerDisplayName.toLowerCase() === "codex") {
          const sync = await syncCodexQuota(source.windowId);
          await loadState(sync.windowId ?? source.windowId);
        }
      } catch (reason) {
        setError(getErrorMessage(reason));
      } finally {
        setInitializing(false);
      }
    }

    void initialize();
  }, [loadState]);

  useEffect(() => {
    const refreshProtection = () => {
      getCodexProtectionStatus().then(setCodexProtection).catch(() => undefined);
      getCodexProtectionEvents()
        .then(setCodexProtectionEvents)
        .catch(() => undefined);
    };
    window.addEventListener("focus", refreshProtection);
    return () => window.removeEventListener("focus", refreshProtection);
  }, []);

  const selectedSource = useMemo(
    () =>
      localState?.sources.find(
        (source) => source.windowId === localState.selectedWindowId,
      ) ?? null,
    [localState],
  );

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
    setNotice(null);
    try {
      const windowId = localState?.selectedWindowId ?? null;
      if (!windowId) {
        setError("No quota window is selected.");
        return;
      }
      if (selectedSource?.providerDisplayName.toLowerCase() !== "codex") {
        await loadState(windowId);
        setNotice("Local quota state refreshed.");
        return;
      }

      const sync = await syncCodexQuota(windowId);
      await Promise.all([
        loadState(sync.windowId ?? windowId),
        getCodexProtectionEvents().then(setCodexProtectionEvents),
      ]);
      if (sync.status !== "synced") {
        setError(
          sync.message ??
            "Codex did not return a new subscription quota checkpoint.",
        );
        return;
      }
      const desktop = sync.desktopTracking;
      if (desktop?.status === "attributed") {
        setNotice(
          `Codex Desktop usage synced. ${desktop.attributedAmount}% was attributed to its workspace.`,
        );
      } else if (desktop?.status === "pending_provider_delta") {
        setNotice(
          "Codex Desktop activity detected. Attribution is waiting for the provider percentage to advance.",
        );
      } else if (desktop?.status === "baseline_established") {
        setNotice(
          "Codex Desktop tracking initialized. Future local activity can be attributed automatically.",
        );
      } else if (desktop?.status === "ambiguous") {
        setNotice(
          "Codex Desktop activity spanned multiple or unmapped folders, so the quota change stayed unattributed.",
        );
      } else if (desktop?.status === "unavailable") {
        setNotice(
          desktop.message ??
            "Codex quota synced, but Desktop activity metadata was unavailable.",
        );
      } else {
        setNotice(
          sync.rolledOver
            ? "Codex quota synced. A new quota window is now active."
            : "Codex quota synced to the latest checkpoint.",
        );
      }
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

  async function handleRemoveSource(source: QuotaSourceSummary) {
    setSubmitting(true);
    setError(null);
    try {
      await archiveQuotaSource(source.poolId);
      await loadState();
      setModal(null);
    } catch (reason) {
      setError(getErrorMessage(reason));
    } finally {
      setSubmitting(false);
    }
  }

  async function handleProtection(enabled: boolean) {
    setProtectionBusy(true);
    setError(null);
    setNotice(null);
    try {
      const status = enabled
        ? await installCodexProtection()
        : await uninstallCodexProtection();
      setCodexProtection(status);
      setNotice(
        enabled
          ? "Protection is configured. In Codex, run /hooks, review and trust the AQM hooks, then restart Codex."
          : "Codex Desktop protection is off. Other Codex hooks were left unchanged.",
      );
    } catch (reason) {
      setError(getErrorMessage(reason));
    } finally {
      setProtectionBusy(false);
    }
  }

  async function handlePriorityOrder(
    windowId: string,
    orderedScopeIds: string[],
  ) {
    setPriorityBusy(true);
    setError(null);
    try {
      await setAllocationPriorityOrder(windowId, orderedScopeIds);
      await loadState(windowId);
    } catch (reason) {
      setError(getErrorMessage(reason));
    } finally {
      setPriorityBusy(false);
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
        onRefresh={handleRefresh}
        onRemoveSource={(source) => setModal({ type: "remove-source", source })}
        removingSource={submitting}
        codexProtection={
          selectedSource?.providerId === "codex"
            ? codexProtection
            : null
        }
        codexProtectionEvents={codexProtectionEvents}
        protectionBusy={protectionBusy}
        onProtection={(enabled) => void handleProtection(enabled)}
        priorityBusy={priorityBusy}
        onPriorityOrder={(orderedScopeIds) => {
          if (localState.selectedWindowId) {
            void handlePriorityOrder(
              localState.selectedWindowId,
              orderedScopeIds,
            );
          }
        }}
      />

      {error && (
        <div className="error-toast" role="alert">
          <span>{error}</span>
          <button type="button" onClick={() => setError(null)} aria-label="Dismiss">
            <Icon name="x" size={17} />
          </button>
        </div>
      )}
      {notice && (
        <div className="error-toast success" role="status">
          <Icon name="check" size={17} />
          <span>{notice}</span>
          <button type="button" onClick={() => setNotice(null)} aria-label="Dismiss">
            <Icon name="x" size={17} />
          </button>
        </div>
      )}

      {modal?.type === "source" && (
        <Modal
          eyebrow="Provider detection"
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

      {modal?.type === "remove-source" && (
        <Modal
          eyebrow="Quota source"
          title="Delete this source?"
          onClose={() => {
            if (!submitting) {
              setModal(null);
            }
          }}
        >
          <div className="delete-source-confirmation">
            <p>
              <strong>{modal.source.providerDisplayName}</strong>
              <span>{modal.source.poolDisplayName}</span>
            </p>
            <p>
              This removes the source from the active list. Its existing usage
              history stays in your local ledger.
            </p>
            <div className="modal-actions">
              <button
                className="button outline"
                type="button"
                disabled={submitting}
                onClick={() => setModal(null)}
              >
                Cancel
              </button>
              <button
                className="button danger-action"
                type="button"
                disabled={submitting}
                onClick={() => void handleRemoveSource(modal.source)}
              >
                <Icon name="trash" size={17} />
                {submitting ? "Deleting…" : "Delete source"}
              </button>
            </div>
          </div>
        </Modal>
      )}

      {modal?.type === "scope" && dashboard && (
        <Modal
          eyebrow="Workspace budget"
          title="Create an allocation"
          onClose={() => setModal(null)}
        >
          <ScopeForm
            unit={unit}
            submitting={submitting}
            onSubmit={(input: WorkspaceInput) =>
              runMutation(() =>
                createAllocatedWorkspace(input, dashboard.window.id, unit),
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
            currentPolicy={
              editingAllocation?.policy ?? {
                warnAtBasisPoints: 8_000,
                confirmAtBasisPoints: 9_000,
                stopAtBasisPoints: 10_000,
                customized: false,
                updatedAt: null,
              }
            }
            unit={unit}
            submitting={submitting}
            onSubmit={(input: WorkspaceBudgetInput) =>
              runMutation(async () => {
                await setAllocation(
                  modal.scope.id,
                  dashboard.window.id,
                  input.amount,
                  unit,
                );
                const currentPolicy = editingAllocation?.policy;
                const policyChanged =
                  !currentPolicy ||
                  currentPolicy.warnAtBasisPoints !== input.warnAtBasisPoints ||
                  currentPolicy.confirmAtBasisPoints !==
                    input.confirmAtBasisPoints ||
                  currentPolicy.stopAtBasisPoints !== input.stopAtBasisPoints;
                if (policyChanged) {
                  await setWorkspacePolicy(modal.scope.id, input);
                }
              })
            }
            onResetPolicy={() =>
              runMutation(async () => {
                await resetWorkspacePolicy(modal.scope.id);
              })
            }
          />
        </Modal>
      )}

    </>
  );
}

export default App;
