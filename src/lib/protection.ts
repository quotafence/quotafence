import type {
  CodexProtectionEvent,
  CodexProtectionStatus,
} from "../types";

export function verifiedCodexProtectionAt(
  protection: CodexProtectionStatus | null,
  events: CodexProtectionEvent[],
): number | null {
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
