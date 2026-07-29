export type ScopeKind = "project" | "repository" | "task" | "reserve";

export type EnforcementDecision = "allow" | "warn" | "confirm" | "stop";

export type IpcError = {
  code: string;
  message: string;
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
};

export type ScopeSummary = {
  id: string;
  parentId: string | null;
  kind: ScopeKind;
  displayName: string;
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
  unit: string;
  limit: number;
  attributedUsage: number;
  activeReservations: number;
  remaining: number;
  spendable: number;
  decision: EnforcementDecision;
};

export type QuotaDashboard = {
  window: WindowSummary;
  allocations: AllocationSnapshot[];
};

export type LocalState = {
  sources: QuotaSourceSummary[];
  scopes: ScopeSummary[];
  selectedWindowId: string | null;
  dashboard: QuotaDashboard | null;
};

export type QuotaSourceInput = {
  providerDisplayName: string;
  poolDisplayName: string;
  capacity: number;
  unit: string;
  startsAt: number;
  endsAt: number;
};

export type ScopeInput = {
  displayName: string;
  kind: ScopeKind;
  parentId: string | null;
  allocation: number;
};

export type UsageInput = {
  scopeId: string | null;
  amount: number;
};
