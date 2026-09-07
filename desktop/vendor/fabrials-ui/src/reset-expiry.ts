import type { Observation, ResetInventory } from "./contracts";

const HOUR = 3_600_000;
const MAX_OBSERVATION_AGE = 15 * 60_000;
export function validTimestamp(value: number | null | undefined): value is number {
  return typeof value === "number" && Number.isFinite(value) && Number.isFinite(new Date(value).getTime());
}

export function resetExpiryLabel(expiry: number | null | undefined, now: number | null): string {
  if (!validTimestamp(expiry)) return "Expiry unavailable";
  if (!validTimestamp(now)) return "…";
  const remaining = expiry - now;
  if (remaining <= 0) return "Expired · refresh inventory";
  const minutes = Math.floor(remaining / 60_000);
  if (minutes === 0) return "<1m";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ${minutes % 60}m`;
  return `${Math.floor(hours / 24)}d ${hours % 24}h`;
}

export function upcomingResetExpiries(inventory: ResetInventory | null, now: number | null): number[] {
  if (!inventory || !validTimestamp(now) || inventory.available <= 0) return [];
  return inventory.credits
    .filter(credit => validTimestamp(credit.expiresAtMs) && credit.expiresAtMs > now
      && (credit.validFromMs === null || (validTimestamp(credit.validFromMs) && credit.validFromMs <= now)))
    .map(credit => credit.expiresAtMs as number)
    .sort((a, b) => a - b)
    .slice(0, Math.min(3, inventory.available));
}

export function resetExpirySummary(observation: Observation<ResetInventory>, now: number) {
  const observed = observation.observedAtMs;
  const stale = observation.availability !== "available" || !validTimestamp(now) || !validTimestamp(observed) || observation.freshness !== "fresh"
    || now - observed > MAX_OBSERVATION_AGE || observed - now > 60_000;
  const inventory = observation.value;
  if (stale || observation.availability !== "available" || !inventory || inventory.available <= 0) {
    return { stale, expiring: 0, expired: 0, nextExpiry: null as number | null };
  }
  let expiring = 0;
  let expired = 0;
  let nextExpiry: number | null = null;
  for (const credit of inventory.credits) {
    if (!validTimestamp(credit.expiresAtMs)) continue;
    if (credit.validFromMs !== null && (!validTimestamp(credit.validFromMs) || credit.validFromMs > now)) continue;
    if (credit.expiresAtMs <= now) { expired++; continue; }
    if (credit.expiresAtMs - now <= 24 * HOUR) {
      expiring++;
      nextExpiry = Math.min(nextExpiry ?? Infinity, credit.expiresAtMs);
    }
  }
  return { stale, expiring: Math.min(expiring, inventory.available), expired, nextExpiry };
}
