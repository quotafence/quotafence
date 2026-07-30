import { invoke } from "@tauri-apps/api/core";
import type {
  CodexDetection,
  CodexSyncResult,
  IpcError,
  LocalState,
  QuotaSourceInput,
  WorkspaceInput,
} from "../types";

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
