import type { Capability, EntitlementSnapshot } from "../types";

export function hasCapability(
  entitlements: EntitlementSnapshot,
  capability: Capability,
): boolean {
  return entitlements.capabilities.includes(capability);
}
