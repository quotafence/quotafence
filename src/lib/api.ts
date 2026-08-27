import { invoke } from "@tauri-apps/api/core";
import type {
  CodexDetection,
  CodexDesktopConfirmation,
  CodexProtectionEvent,
  CodexProtectionStatus,
  CodexSyncResult,
  ClaudeStatusLineStatus,
  ClaudeProtectionStatus,
  ClaudeSyncResult,
  IpcError,
  EntitlementSnapshot,
  LocalState,
  PolicySummary,
  QuotaSourceInput,
  WorkspaceInput,
} from "../types";

export async function getEntitlements(): Promise<EntitlementSnapshot> {
  return invoke<EntitlementSnapshot>("get_entitlements");
}

function createId(prefix: string): string {
  const suffix =
    globalThis.crypto?.randomUUID?.() ??
    `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  return `${prefix}-${suffix}`;
}

export async function getLocalState(
  selectedWindowId: string | null = null,
): Promise<LocalState> {
  return invoke<LocalState>("get_local_state", {
    request: {
      selectedWindowId,
      at: Date.now(),
    },
  });
}

export async function detectCodexQuota(): Promise<CodexDetection> {
  return invoke<CodexDetection>("detect_codex_quota");
}

export async function syncCodexQuota(
  windowId: string,
): Promise<CodexSyncResult> {
  return invoke<CodexSyncResult>("sync_codex_quota", { windowId });
}

export async function getCodexProtectionStatus(): Promise<CodexProtectionStatus> {
  return invoke<CodexProtectionStatus>("get_codex_protection_status");
}

export async function installCodexProtection(): Promise<CodexProtectionStatus> {
  return invoke<CodexProtectionStatus>("install_codex_protection");
}

export async function uninstallCodexProtection(): Promise<CodexProtectionStatus> {
  return invoke<CodexProtectionStatus>("uninstall_codex_protection");
}

export async function getClaudeIntegrationStatus(): Promise<ClaudeStatusLineStatus> {
  return invoke<ClaudeStatusLineStatus>("get_claude_integration_status");
}

export async function installClaudeIntegration(): Promise<ClaudeStatusLineStatus> {
  return invoke<ClaudeStatusLineStatus>("install_claude_integration");
}

export async function uninstallClaudeIntegration(): Promise<ClaudeStatusLineStatus> {
  return invoke<ClaudeStatusLineStatus>("uninstall_claude_integration");
}

export async function getClaudeProtectionStatus(): Promise<ClaudeProtectionStatus> {
  return invoke<ClaudeProtectionStatus>("get_claude_protection_status");
}

export async function installClaudeProtection(): Promise<ClaudeProtectionStatus> {
  return invoke<ClaudeProtectionStatus>("install_claude_protection");
}

export async function uninstallClaudeProtection(): Promise<ClaudeProtectionStatus> {
  return invoke<ClaudeProtectionStatus>("uninstall_claude_protection");
}

export async function syncClaudeQuota(): Promise<ClaudeSyncResult> {
  return invoke<ClaudeSyncResult>("sync_claude_quota");
}

export async function getCodexProtectionEvents(
  limit = 100,
): Promise<CodexProtectionEvent[]> {
  return invoke<CodexProtectionEvent[]>("get_codex_protection_events", {
    request: { limit },
  });
}

export async function getPendingCodexConfirmations(): Promise<CodexDesktopConfirmation[]> {
  return invoke<CodexDesktopConfirmation[]>("get_pending_codex_confirmations");
}

export async function resolveCodexConfirmation(
  id: string,
  approved: boolean,
): Promise<boolean> {
  return invoke<boolean>("resolve_codex_confirmation", { id, approved });
}

export async function createQuotaSource(
  input: QuotaSourceInput,
): Promise<void> {
  const sourceId = createId("source");

  await invoke("create_quota_source", {
    request: {
      providerId: `${sourceId}-provider`,
      providerDisplayName: input.providerDisplayName,
      accountId: `${sourceId}-account`,
      accountDisplayName: input.accountDisplayName ?? "Subscription",
      poolId: `${sourceId}-pool`,
      poolDisplayName: input.poolDisplayName,
      windowId: `${sourceId}-window`,
      startsAt: input.startsAt,
      endsAt: input.endsAt,
      capacity: input.capacity,
      unit: input.unit,
      providerSnapshot: input.providerSnapshot ?? null,
    },
  });
}

export async function archiveQuotaSource(poolId: string): Promise<void> {
  await invoke("archive_quota_source", {
    request: {
      poolId,
      archivedAt: Date.now(),
    },
  });
}

export async function setAllocation(
  scopeId: string,
  windowId: string,
  amount: number,
  unit: string,
): Promise<void> {
  await invoke("set_allocation", {
    request: {
      scopeId,
      windowId,
      amount,
      unit,
    },
  });
}

export async function removeWorkspaceAllocation(
  scopeId: string,
  windowId: string,
): Promise<void> {
  await invoke("remove_workspace_allocation", {
    request: {
      scopeId,
      windowId,
      removedAt: Date.now(),
    },
  });
}

export async function setAllocationPriorityOrder(
  windowId: string,
  orderedScopeIds: string[],
): Promise<void> {
  await invoke("set_allocation_priority_order", {
    request: {
      windowId,
      orderedScopeIds,
    },
  });
}

export async function setWorkspacePolicy(
  scopeId: string,
  policy: {
    warnAtBasisPoints: number | null;
    confirmAtBasisPoints: number | null;
    stopAtBasisPoints: number | null;
  },
): Promise<PolicySummary> {
  return invoke<PolicySummary>("set_workspace_policy", {
    request: {
      scopeId,
      ...policy,
      updatedAt: Date.now(),
    },
  });
}

export async function resetWorkspacePolicy(
  scopeId: string,
): Promise<PolicySummary> {
  return invoke<PolicySummary>("reset_workspace_policy", {
    request: { scopeId },
  });
}

export async function createAllocatedWorkspace(
  input: WorkspaceInput,
  windowId: string,
  unit: string,
): Promise<void> {
  await invoke("create_allocated_workspace", {
    request: {
      id: createId("workspace"),
      displayName: input.displayName,
      canonicalPath: input.workspacePath,
      windowId,
      amount: input.allocation,
      unit,
      boundAt: Date.now(),
    },
  });
}

export function getErrorMessage(error: unknown): string {
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof (error as IpcError).message === "string"
  ) {
    return (error as IpcError).message;
  }

  if (error instanceof Error) {
    return error.message;
  }

  return typeof error === "string"
    ? error
    : "Something went wrong. Please try again.";
}
