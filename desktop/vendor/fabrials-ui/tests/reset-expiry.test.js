import { expect, test } from "bun:test";
import { resetExpiryLabel, resetExpirySummary, upcomingResetExpiries } from "../src/reset-expiry";

const now = 1_800_000_000_000;
const hour = 3_600_000;
function observation(credits, overrides = {}) {
  return { availability: "available", freshness: "fresh", observedAtMs: now,
    source: "fixture", error: null,
    value: {available: credits.length, detailsComplete: true, credits}, ...overrides };
}
const credit = (expiresAtMs, validFromMs = null) => ({expiresAtMs, validFromMs});
test("reports only currently valid credits expiring in the next day", () => {
  const summary = resetExpirySummary(observation([
    credit(now + hour), credit(now + 24 * hour), credit(now + 25 * hour),
    credit(now + hour, now + 1000), credit(null), credit(now),
  ]), now);
  expect(summary).toEqual({stale:false,expiring:2,expired:1,nextExpiry:now + hour});
});
test("suppresses alerts for stale, unavailable or untrusted observation times", () => {
  for (const overrides of [{freshness:"stale"}, {availability:"unavailable"},
    {observedAtMs:null}, {observedAtMs:now - 900001}, {observedAtMs:now + 60001}]) {
    expect(resetExpirySummary(observation([credit(now + hour)], overrides), now).expiring).toBe(0);
  }
});
test("does not invent alerts from count-only, zero or invalid date inventories", () => {
  expect(resetExpirySummary(observation([]), now).expiring).toBe(0);
  expect(resetExpirySummary(observation([credit(Infinity), credit(9e18)]), now).expiring).toBe(0);
  const empty = observation([credit(now + hour)]); empty.value.available = 0;
  expect(resetExpirySummary(empty, now).expiring).toBe(0);
});

test("formats imminent expiries without suggesting zero remaining time", () => {
  expect(resetExpiryLabel(now + 30_000, now)).toBe("<1m");
  expect(resetExpiryLabel(now + 17 * 60_000, now)).toBe("17m");
  expect(resetExpiryLabel(now + hour + 5 * 60_000, now)).toBe("1h 5m");
  expect(resetExpiryLabel(now + 49 * hour, now)).toBe("2d 1h");
  expect(resetExpiryLabel(now, now)).toBe("Expired · refresh inventory");
  expect(resetExpiryLabel(null, now)).toBe("Expiry unavailable");
  expect(resetExpiryLabel(now, null)).toBe("…");
});
test("preview orders up to three valid expiries and respects the reported count", () => {
  const inventory = observation([credit(now + 4 * hour), credit(now + hour),
    credit(now + 2 * hour), credit(now + 3 * hour), credit(now),
    credit(now + 500, now + 1000), credit(null)]).value;
  expect(upcomingResetExpiries(inventory, now)).toEqual([now + hour, now + 2 * hour, now + 3 * hour]);
  inventory.available = 1;
  expect(upcomingResetExpiries(inventory, now)).toEqual([now + hour]);
  expect(upcomingResetExpiries(inventory, null)).toEqual([]);
  expect(upcomingResetExpiries(null, now)).toEqual([]);
});
