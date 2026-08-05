import type {
  CodexProtectionEvent,
  CodexProtectionStatus,
} from "../types";

export function verifiedCodexProtectionAt(
  protection: CodexProtectionStatus | null,
  events: CodexProtectionEvent[],
): number | null {
  const observedAt = observedCodexHookAt(protection);
  if (observedAt !== null && protection?.lastHookStatus === "decision") {
    return observedAt;
  }
  if (
    protection?.installed !== true ||
    protection.state !== "configured" ||
    events.length === 0
  ) {
    return null;
  }

  const latestEventAt = events[0]?.occurredAt ?? null;
  if (latestEventAt === null) {
    return null;
  }

  const requiredAfter = protection.verificationRequiredAfter;
  return requiredAfter === null || latestEventAt >= requiredAfter
    ? latestEventAt
    : null;
}

export function observedCodexHookAt(
  protection: CodexProtectionStatus | null,
): number | null {
  if (
    protection?.installed !== true ||
    protection.state !== "configured" ||
    protection.lastHookObservedAt === null
  ) {
    return null;
  }
  const requiredAfter = protection.verificationRequiredAfter;
  return requiredAfter === null || protection.lastHookObservedAt >= requiredAfter
    ? protection.lastHookObservedAt
    : null;
}
