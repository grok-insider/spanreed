"use client";
const HOUR = 36e5;
const MAX_OBSERVATION_AGE = 15 * 6e4;
function validTimestamp(value) {
  return typeof value === "number" && Number.isFinite(value) && Number.isFinite(new Date(value).getTime());
}
function resetExpiryLabel(expiry, now) {
  if (!validTimestamp(expiry)) return "Expiry unavailable";
  if (!validTimestamp(now)) return "…";
  const remaining = expiry - now;
  if (remaining <= 0) return "Expired · refresh inventory";
  const minutes = Math.floor(remaining / 6e4);
  if (minutes === 0) return "<1m";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ${minutes % 60}m`;
  return `${Math.floor(hours / 24)}d ${hours % 24}h`;
}
function upcomingResetExpiries(inventory, now) {
  if (!inventory || !validTimestamp(now) || inventory.available <= 0) return [];
  return inventory.credits.filter(
    (credit) => validTimestamp(credit.expiresAtMs) && credit.expiresAtMs > now && (credit.validFromMs === null || validTimestamp(credit.validFromMs) && credit.validFromMs <= now)
  ).map((credit) => credit.expiresAtMs).sort((a, b) => a - b).slice(0, Math.min(3, inventory.available));
}
function resetExpirySummary(observation, now) {
  const observed = observation.observedAtMs;
  const stale = observation.availability !== "available" || !validTimestamp(now) || !validTimestamp(observed) || observation.freshness !== "fresh" || now - observed > MAX_OBSERVATION_AGE || observed - now > 6e4;
  const inventory = observation.value;
  if (stale || observation.availability !== "available" || !inventory || inventory.available <= 0) {
    return {
      stale,
      expiring: 0,
      expired: 0,
      nextExpiry: null
    };
  }
  let expiring = 0;
  let expired = 0;
  let nextExpiry = null;
  for (const credit of inventory.credits) {
    if (!validTimestamp(credit.expiresAtMs)) continue;
    if (credit.validFromMs !== null && (!validTimestamp(credit.validFromMs) || credit.validFromMs > now))
      continue;
    if (credit.expiresAtMs <= now) {
      expired++;
      continue;
    }
    if (credit.expiresAtMs - now <= 24 * HOUR) {
      expiring++;
      nextExpiry = Math.min(nextExpiry ?? Infinity, credit.expiresAtMs);
    }
  }
  return {
    stale,
    expiring: Math.min(expiring, inventory.available),
    expired,
    nextExpiry
  };
}
export {
  resetExpiryLabel,
  resetExpirySummary,
  upcomingResetExpiries,
  validTimestamp
};
