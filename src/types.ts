export type ScopeKind = "project" | "workspace" | "task" | "reserve";

export type EnforcementDecision =
  | "allow"
  | "warn"
  | "require_confirmation"
  | "stop";

export type PolicySummary = {
  warnAtBasisPoints: number | null;
  confirmAtBasisPoints: number | null;
  stopAtBasisPoints: number | null;
  customized: boolean;
  updatedAt: number | null;
};

export type IpcError = {
  code: string;
  message: string;
};

export type ProviderDetectionStatus =
  | "detected"
  | "not_installed"
  | "not_authenticated"
  | "unavailable";

export type DetectedQuotaWindow = {
  id: string;
  displayName: string;
  kind: string;
  startsAt: number;
  endsAt: number;
  capacity: number;
  used: number;
  remaining: number;
  unit: string;
  durationMinutes: number;
};

export type CodexDetection = {
  status: ProviderDetectionStatus;
  providerId: string;
  providerDisplayName: string;
  planType: string | null;
  windows: DetectedQuotaWindow[];
  message: string | null;
};

export type CodexSyncResult = {
  status: "synced" | "not_applicable" | "unavailable";
  windowId: string | null;
  rolledOver: boolean;
  syncedAt: number | null;
  message: string | null;
  desktopTracking: {
    status:
      | "unavailable"
      | "baseline_established"
      | "no_activity"
      | "pending_provider_delta"
      | "attributed"
      | "ambiguous"
      | "window_rolled_over";
    observedThreads: number;
    pendingTokens: number;
    attributedAmount: number;
    scopeId: string | null;
    message: string | null;
  } | null;
};

export type CodexProtectionStatus = {
  installed: boolean;
  hasAqmHooks: boolean;
  requiresReview: boolean;
  verificationRequiredAfter: number | null;
  configPath: string;
  state: "disabled" | "configured" | "misconfigured";
  issue: string | null;
};

export type CodexProtectionEvent = {
  canonicalPath: string;
  scopeId: string | null;
  workspaceName: string | null;
  outcome: "allowed" | "blocked";
  reason: string;
  occurredAt: number;
};

export type QuotaSourceSummary = {
  providerId: string;
  providerDisplayName: string;
  accountId: string;
  accountDisplayName: string;
  poolId: string;
  poolDisplayName: string;
  windowId: string;
  startsAt: number;
  endsAt: number;
  capacity: number;
  unit: string;
  isActive: boolean;
  providerManaged: boolean;
  lastSyncedAt: number | null;
};

export type ScopeSummary = {
  id: string;
  parentId: string | null;
  kind: ScopeKind;
  displayName: string;
  workspacePath: string | null;
};

export type WindowSummary = {
  id: string;
  poolId: string;
  startsAt: number;
  endsAt: number;
  unit: string;
  capacity: number;
  allocatedToRootScopes: number;
  unallocated: number;
  unattributedUsage: number;
  providerRemaining: number;
  providerSpendable: number;
};

export type AllocationSnapshot = {
  scopeId: string;
  parentId: string | null;
  scopeKind: ScopeKind;
  displayName: string;
  priority: number;
  unit: string;
  limit: number;
  attributedUsage: number;
  activeReservations: number;
  remaining: number;
  spendable: number;
  protectedNow: number;
  decision: EnforcementDecision;
  policy: PolicySummary;
};

export type DepletionForecast = {
  status:
    | "insufficient_data"
    | "no_managed_burn"
    | "survives_to_reset"
    | "depletes_before_reset"
    | "window_ended";
  confidence: "low" | "medium";
  sampleCount: number;
  observationStart: number | null;
  observationEnd: number;
  managedUsage: number;
  attributedManagedUsage: number;
  coverageBasisPoints: number;
  burnRatePerDayMilliunits: number | null;
  projectedDepletionAt: number | null;
  projectedRemainingAtReset: number | null;
};

export type WorkspaceBudgetInput = {
  amount: number;
  warnAtBasisPoints: number | null;
  confirmAtBasisPoints: number | null;
  stopAtBasisPoints: number | null;
};

export type QuotaDashboard = {
  window: WindowSummary;
  allocations: AllocationSnapshot[];
  quotaHistory: QuotaHistoryPoint[];
  forecast: DepletionForecast;
};

export type QuotaHistoryPoint = {
  observedAt: number;
  remaining: number;
};

export type LocalState = {
  sources: QuotaSourceSummary[];
  scopes: ScopeSummary[];
  selectedWindowId: string | null;
  dashboard: QuotaDashboard | null;
};

export type QuotaSourceInput = {
  providerDisplayName: string;
  accountDisplayName?: string;
  poolDisplayName: string;
  capacity: number;
  unit: string;
  startsAt: number;
  endsAt: number;
  providerSnapshot?: {
    adapter: string;
    remoteLimitId: string;
    remoteWindowKind: string;
    used: number;
    observedAt: number;
    resetsAt: number;
  };
};

export type WorkspaceInput = {
  displayName: string;
  allocation: number;
  workspacePath: string;
};
