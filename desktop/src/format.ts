import type { MetricLine, ProviderOutput } from "@fabrials/ai-ui";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

export function duration(ms: number) {
  const value = Math.abs(ms);
  if (value < HOUR) return `${Math.max(1, Math.round(value / MINUTE))} min`;
  if (value < DAY) {
    const hours = Math.floor(value / HOUR);
    const minutes = Math.round((value % HOUR) / MINUTE);
    return minutes && minutes < 60 ? `${hours} h ${minutes} min` : `${hours} h`;
  }
  const days = Math.floor(value / DAY);
  const hours = Math.round((value % DAY) / HOUR);
  return hours && hours < 24 ? `${days} d ${hours} h` : `${days} d`;
}

export function relativeTime(target: number, now: number) {
  const diff = target - now;
  if (Math.abs(diff) < MINUTE) return diff > 0 ? "in under a minute" : "just now";
  return diff > 0 ? `in ${duration(diff)}` : `${duration(diff)} ago`;
}

/** For timestamps that are already in the past; a clock that ticks once a minute can lag behind them. */
export function ago(ms: number, now: number) {
  return relativeTime(Math.min(ms, now), now);
}

export function absoluteTime(ms: number) {
  return new Date(ms).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

const compact = new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 });
const whole = new Intl.NumberFormat();
const usd = new Intl.NumberFormat(undefined, { style: "currency", currency: "USD", maximumFractionDigits: 2 });
export const formatCount = (value: number) => whole.format(value);
export const formatCompact = (value: number) => compact.format(value);
export const formatUsd = (value: number) => usd.format(value);

const providerNames: Record<string, string> = {
  grok: "SuperGrok", codex: "Codex", nous: "Nous", openai: "OpenAI API", claude: "Claude", copilot: "Copilot",
  "opencode-go": "OpenCode Go", "jetbrains-ai-assistant": "JetBrains AI", zai: "Z.ai",
};
/** Whether the publication timer is installed, without the raw scheduler string. */
export function publicationSchedule(schedule: string | null | undefined): "loading" | "on" | "off" | "unknown" {
  if (schedule == null) return "loading";
  const text = schedule.split(";")[0]?.toLowerCase() ?? "";
  if (text.includes("unsupported") || text.includes("not supported") || text.includes("systemctl missing")) return "unknown";
  if (/:\s*(inactive|deactivating|failed)\b/.test(text) || text.includes("not installed") || text.includes("not loaded") || /:\s*missing\b/.test(text)) return "off";
  if (/:\s*(active|activating)\b/.test(text) || text.includes(": loaded") || (text.includes("windows-task") && !text.includes("missing"))) return "on";
  return "unknown";
}

export function providerName(id: string) {
  return providerNames[id] ?? (id ? id[0].toUpperCase() + id.slice(1) : id);
}

export function lineUtilization(line: MetricLine) {
  if (line.type !== "progress" || !(line.limit > 0)) return null;
  return Math.min(100, Math.max(0, (line.used / line.limit) * 100));
}

export function utilization(output: ProviderOutput) {
  const values = output.lines.map(lineUtilization).filter((value): value is number => value !== null);
  return values.length ? Math.max(...values) : null;
}

export function sortByUtilization(outputs: readonly ProviderOutput[]) {
  return outputs.map((output, index) => ({ output, index, value: utilization(output) }))
    .sort((a, b) => (b.value ?? -1) - (a.value ?? -1) || a.index - b.index)
    .map(({ output }) => output);
}

export type Attention = { key: string; tone: "danger" | "warning"; providerId: string; title: string; detail?: string };

const lineText = (line: MetricLine) => line.type === "text" ? line.value : line.type === "badge" ? line.text : line.label;

export function attentionItems(outputs: readonly ProviderOutput[], now: number, threshold = 80): Attention[] {
  const items: Attention[] = [];
  for (const output of outputs) {
    output.lines.forEach((line, index) => {
      if (line.kind === "error") {
        items.push({ key: `${output.providerId}:error:${index}`, tone: "danger", providerId: output.providerId, title: `Couldn't read ${output.displayName}`, detail: lineText(line) });
        return;
      }
      const value = lineUtilization(line);
      if (value === null || value < threshold || line.type !== "progress") return;
      const resets = line.resetsAt ? Date.parse(line.resetsAt) : NaN;
      items.push({
        key: `${output.providerId}:limit:${index}`, tone: value >= 100 ? "danger" : "warning", providerId: output.providerId,
        title: `${output.displayName} ${line.label.toLowerCase()} limit at ${Math.round(value)}%`,
        detail: Number.isFinite(resets) ? `Resets ${relativeTime(resets, now)}` : undefined,
      });
    });
    const inventory = output.resetInventory;
    if (inventory?.availability === "available" && inventory.freshness === "fresh" && inventory.value) {
      const soon = inventory.value.credits.map((credit) => credit.expiresAtMs)
        .filter((expiry): expiry is number => typeof expiry === "number" && expiry > now && expiry - now <= DAY)
        .sort((a, b) => a - b);
      if (soon.length) items.push({
        key: `${output.providerId}:reset-credits`, tone: "warning", providerId: output.providerId,
        title: `${soon.length} ${output.displayName} reset credit${soon.length === 1 ? " expires" : "s expire"} ${relativeTime(soon[0], now)}`,
      });
    }
  }
  return items.sort((a, b) => (a.tone === b.tone ? 0 : a.tone === "danger" ? -1 : 1));
}
