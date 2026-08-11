import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import "./App.css";
import { AllocationForm } from "./components/AllocationForm";
import { Dashboard, type DashboardView } from "./components/Dashboard";
import { Icon } from "./components/Icon";
import { Modal } from "./components/Modal";
import { ScopeForm } from "./components/ScopeForm";
import type { ThemePreference } from "./components/SettingsPanel";
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
  removeWorkspaceAllocation,
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
  CodexSyncResult,
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
  | { type: "remove-allocation"; scope: ScopeSummary }
  | { type: "remove-source"; source: QuotaSourceSummary }
  | null;

const THEME_STORAGE_KEY = "aqm-theme";
const AUTO_SYNC_INTERVAL_MS = 2 * 60_000;

function storedTheme(): ThemePreference {
  const value = localStorage.getItem(THEME_STORAGE_KEY);
  return value === "light" || value === "dark" || value === "system"
    ? value
    : "system";
}

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
  const [codexSyncResult, setCodexSyncResult] =
    useState<CodexSyncResult | null>(null);
  const [codexSyncIssue, setCodexSyncIssue] = useState<string | null>(null);
  const [priorityBusy, setPriorityBusy] = useState(false);
  const [view, setView] = useState<DashboardView>("overview");
  const [theme, setTheme] = useState<ThemePreference>(storedTheme);
  const initialSyncStarted = useRef(false);
  const syncInFlight = useRef(false);

  const loadState = useCallback(async (windowId: string | null = null) => {
    const nextState = await getLocalState(windowId);
    setLocalState(nextState);
    return nextState;
  }, []);

  useEffect(() => {
    const systemTheme = window.matchMedia("(prefers-color-scheme: dark)");
    const applyTheme = () => {
      const resolved =
        theme === "system" ? (systemTheme.matches ? "dark" : "light") : theme;
      document.documentElement.dataset.theme = resolved;
      document.documentElement.style.colorScheme = resolved;
    };

    localStorage.setItem(THEME_STORAGE_KEY, theme);
    applyTheme();
    if (theme === "system") {
      systemTheme.addEventListener("change", applyTheme);
      return () => systemTheme.removeEventListener("change", applyTheme);
    }
  }, [theme]);

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
              verificationRequiredAfter: null,
              lastHookObservedAt: null,
              lastHookStatus: null,
              lastHookIssue: null,
              configPath: "",
              state: "misconfigured",
              issue: "Agent Quota Manager could not inspect the Codex hook configuration.",
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
        if (
          source?.providerDisplayName.toLowerCase() === "codex" &&
          !syncInFlight.current
        ) {
          syncInFlight.current = true;
          try {
            const sync = await syncCodexQuota(source.windowId);
            setCodexSyncResult(sync);
            setCodexSyncIssue(
              sync.status === "synced"
                ? null
                : sync.message ?? "Codex sync was unavailable.",
            );
            await loadState(sync.windowId ?? source.windowId);
          } catch (reason) {
            setCodexSyncIssue(getErrorMessage(reason));
          } finally {
            syncInFlight.current = false;
          }
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

  useEffect(() => {
    const windowId = localState?.selectedWindowId ?? null;
    if (
      !windowId ||
      selectedSource?.providerDisplayName.toLowerCase() !== "codex"
    ) {
      return;
    }

    let cancelled = false;
    const syncInBackground = async () => {
      if (cancelled || syncInFlight.current) {
        return;
      }
      syncInFlight.current = true;
      try {
        const sync = await syncCodexQuota(windowId);
        setCodexSyncResult(sync);
        setCodexSyncIssue(
          sync.status === "synced"
            ? null
            : sync.message ?? "Codex sync was unavailable.",
        );
        if (cancelled || sync.status !== "synced") {
          return;
        }
        await Promise.all([
          loadState(sync.windowId ?? windowId),
          getCodexProtectionEvents().then(setCodexProtectionEvents),
        ]);
      } catch (reason) {
        setCodexSyncIssue(getErrorMessage(reason));
        // Background refresh stays silent. The visible Sync action reports errors.
      } finally {
        syncInFlight.current = false;
      }
    };
    const syncWhenVisible = () => {
      if (document.visibilityState === "visible") {
        void syncInBackground();
      }
    };
    const intervalId = window.setInterval(
      () => void syncInBackground(),
      AUTO_SYNC_INTERVAL_MS,
    );
    window.addEventListener("focus", syncWhenVisible);
    document.addEventListener("visibilitychange", syncWhenVisible);
    return () => {
      cancelled = true;
      window.clearInterval(intervalId);
      window.removeEventListener("focus", syncWhenVisible);
      document.removeEventListener("visibilitychange", syncWhenVisible);
    };
  }, [
    loadState,
    localState?.selectedWindowId,
    selectedSource?.providerDisplayName,
  ]);

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
    if (syncInFlight.current) {
      return;
    }
    syncInFlight.current = true;
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
      setCodexSyncResult(sync);
      setCodexSyncIssue(
        sync.status === "synced"
          ? null
          : sync.message ?? "Codex sync was unavailable.",
      );
      await Promise.all([
        loadState(sync.windowId ?? windowId),
        getCodexProtectionEvents().then(setCodexProtectionEvents),
        getCodexProtectionStatus().then(setCodexProtection),
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
      setCodexSyncIssue(getErrorMessage(reason));
      setError(getErrorMessage(reason));
    } finally {
      syncInFlight.current = false;
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

  async function handleRemoveAllocation(scope: ScopeSummary) {
    const windowId = localState?.selectedWindowId;
    if (!windowId) {
      setError("No quota window is selected.");
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      await removeWorkspaceAllocation(scope.id, windowId);
      await loadState(windowId);
      setModal(null);
      setNotice(`${scope.displayName} is no longer allocated quota.`);
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
          ? "Trust and enable UserPromptSubmit and Stop in Codex, then send another prompt in your current task. Agent Quota Manager will verify it automatically."
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
        view={view}
        onViewChange={setView}
        refreshing={refreshing}
        onSelectSource={handleSelectSource}
        onAddSource={() => setModal({ type: "source" })}
        onAddScope={() => setModal({ type: "scope" })}
        onEditAllocation={(scope) => setModal({ type: "allocation", scope })}
        onRemoveAllocation={(scope) =>
          setModal({ type: "remove-allocation", scope })
        }
        onRefresh={handleRefresh}
        onRemoveSource={(source) => setModal({ type: "remove-source", source })}
        removingSource={submitting}
        codexProtection={codexProtection}
        codexProtectionEvents={codexProtectionEvents}
        codexSyncResult={codexSyncResult}
        codexSyncIssue={codexSyncIssue}
        protectionBusy={protectionBusy}
        onProtection={(enabled) => void handleProtection(enabled)}
        theme={theme}
        onThemeChange={setTheme}
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

      {modal?.type === "remove-allocation" && (
        <Modal
          eyebrow="Workspace allocation"
          title="Delete this allocation?"
          onClose={() => {
            if (!submitting) {
              setModal(null);
            }
          }}
        >
          <div className="delete-source-confirmation">
            <p>
              <strong>{modal.scope.displayName}</strong>
              <span>{modal.scope.workspacePath}</span>
            </p>
            <p>
              This releases its planned quota and removes the folder binding.
              Existing usage history stays in your local ledger. No files in
              the folder are changed.
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
                onClick={() => void handleRemoveAllocation(modal.scope)}
              >
                <Icon name="trash" size={17} />
                {submitting ? "Deleting…" : "Delete allocation"}
              </button>
            </div>
          </div>
        </Modal>
      )}

      {modal?.type === "scope" && dashboard && (
        <Modal
          eyebrow="Workspace allocation"
          title="Create an allocation"
          onClose={() => setModal(null)}
        >
          <ScopeForm
            unit={unit}
            maxAllocation={dashboard.window.unallocated}
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
            maxAmount={
              dashboard.window.unallocated + (editingAllocation?.limit ?? 0)
            }
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
