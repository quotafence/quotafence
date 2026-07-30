import { invoke } from "@tauri-apps/api/core";
import type {
  CodexDetection,
  CodexSyncResult,
  IpcError,
  LocalState,
  QuotaSourceInput,
  ScopeInput,
  UsageInput,
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

export async function createScope(input: ScopeInput): Promise<string> {
  const scopeId = createId(input.kind);

  await invoke("create_scope", {
    request: {
      id: scopeId,
      parentId: input.parentId,
      kind: input.kind,
      displayName: input.displayName,
    },
  });

  return scopeId;
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

export async function createAllocatedScope(
  input: ScopeInput,
  windowId: string,
  unit: string,
): Promise<void> {
  await invoke("create_allocated_scope", {
    request: {
      id: createId(input.kind),
      parentId: input.parentId,
      kind: input.kind,
      displayName: input.displayName,
      windowId,
      amount: input.allocation,
      unit,
    },
  });
}

export async function recordUsage(
  input: UsageInput,
  windowId: string,
  unit: string,
): Promise<void> {
  await invoke("record_usage", {
    request: {
      id: createId("usage"),
      windowId,
      scopeId: input.scopeId,
      amount: input.amount,
      unit,
      observedAt: Date.now(),
      source: "local_measured",
      confidence: "observed",
      reservationId: null,
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
