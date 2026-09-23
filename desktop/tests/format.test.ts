import { expect, test } from "bun:test";
import type { ProviderOutput } from "@fabrials/ai-ui";
import { ago, attentionItems, duration, relativeTime, sortByUtilization, utilization } from "../src/format";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const now = Date.UTC(2026, 8, 23, 12, 0);

test("relative times read like a person would say them", () => {
  expect(relativeTime(now + 2 * HOUR + 18 * MINUTE, now)).toBe("in 2 h 18 min");
  expect(relativeTime(now + 26 * HOUR, now)).toBe("in 1 d 2 h");
  expect(relativeTime(now - 5 * MINUTE, now)).toBe("5 min ago");
  expect(relativeTime(now + 20_000, now)).toBe("in under a minute");
  expect(relativeTime(now, now)).toBe("just now");
  expect(duration(3 * HOUR)).toBe("3 h");
});

test("past events never read as future when the clock lags behind", () => {
  expect(ago(now + 30_000, now)).toBe("just now");
  expect(ago(now - 2 * HOUR, now)).toBe("2 h ago");
});

const provider = (id: string, lines: ProviderOutput["lines"], extra: Partial<ProviderOutput> = {}): ProviderOutput => ({ providerId: id, displayName: id[0].toUpperCase() + id.slice(1), lines, ...extra });
const percent = (label: string, used: number, resetsAt?: string) => ({ type: "progress" as const, kind: "quota" as const, label, used, limit: 100, format: { kind: "percent" as const }, resetsAt });

test("providers are ordered by the limit closest to running out", () => {
  const outputs = [
    provider("codex", [percent("Weekly", 54)]),
    provider("copilot", [{ type: "badge", kind: "error", label: "Error", text: "Sign in again" }]),
    provider("grok", [percent("Weekly", 91)]),
    provider("claude", [percent("Session", 42), percent("Weekly", 67)]),
  ];
  expect(sortByUtilization(outputs).map((output) => output.providerId)).toEqual(["grok", "claude", "codex", "copilot"]);
  expect(utilization(outputs[3])).toBe(67);
});

test("needs attention lists errors first, then high limits and expiring reset credits", () => {
  const items = attentionItems([
    provider("grok", [percent("Weekly", 91, new Date(now + 38 * HOUR).toISOString())], {
      resetInventory: { availability: "available", freshness: "fresh", observedAtMs: now, source: "grok", error: null, value: { available: 2, detailsComplete: true, credits: [{ validFromMs: null, expiresAtMs: now + 19 * HOUR }, { validFromMs: null, expiresAtMs: now + 5 * 24 * HOUR }] } },
    }),
    provider("claude", [percent("Session", 42)]),
    provider("copilot", [{ type: "badge", kind: "error", label: "Error", text: "Run `gh auth login` to sign in again." }]),
  ], now);
  expect(items.map((item) => [item.tone, item.title])).toEqual([
    ["danger", "Couldn't read Copilot"],
    ["warning", "Grok weekly limit at 91%"],
    ["warning", "1 Grok reset credit expires in 19 h"],
  ]);
  expect(items[0].detail).toBe("Run `gh auth login` to sign in again.");
  expect(items[1].detail).toBe("Resets in 1 d 14 h");
});

test("stale reset credit readings are not reported as expiring", () => {
  const items = attentionItems([provider("grok", [], {
    resetInventory: { availability: "available", freshness: "stale", observedAtMs: now - 5 * HOUR, source: "grok", error: null, value: { available: 1, detailsComplete: true, credits: [{ validFromMs: null, expiresAtMs: now + HOUR }] } },
  })], now);
  expect(items).toEqual([]);
});
