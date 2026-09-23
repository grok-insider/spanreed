// Synthetic fixture data for browser-only layout review. Never real accounts or credentials.
type Scenario = "populated" | "fresh";

const HOUR = 3_600_000;
const DAY = 24 * HOUR;

function seeded(seed: number) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function createFixtures(scenario: Scenario, now = Date.now()) {
  const iso = (ms: number) => new Date(ms).toISOString();
  const random = seeded(42);
  const fresh = scenario === "fresh";

  const detection = [
    ["claude", "Claude", true], ["codex", "Codex", true], ["grok", "Grok", true], ["copilot", "Copilot", true],
    ["cursor", "Cursor", false], ["amp", "Amp", false], ["antigravity", "Antigravity", false], ["devin", "Devin", false],
    ["factory", "Factory", false], ["jetbrains-ai-assistant", "JetBrains AI", false], ["kimi", "Kimi Code", false],
    ["kiro", "Kiro", false], ["minimax", "MiniMax", false], ["nous", "Nous", false], ["opencode-go", "OpenCode Go", false],
    ["perplexity", "Perplexity", false], ["synthetic", "Synthetic", false], ["zai", "Z.ai", false],
  ].map(([id, name, detected]) => ({ id, name, detected: fresh ? false : detected }));

  const snapshot = fresh ? [] : [
    { providerId: "claude", displayName: "Claude", plan: "Max 5x", lines: [
      { type: "progress", kind: "quota", label: "Session", used: 42, limit: 100, format: { kind: "percent" }, resetsAt: iso(now + 2.2 * HOUR) },
      { type: "progress", kind: "quota", label: "Weekly", used: 67, limit: 100, format: { kind: "percent" }, resetsAt: iso(now + 3.1 * DAY) },
      { type: "progress", kind: "quota", label: "Opus weekly", used: 23, limit: 100, format: { kind: "percent" }, resetsAt: iso(now + 3.1 * DAY) },
      { type: "text", kind: "cost", label: "Last 30 Days", value: "$412.37 · 184.2M tokens" },
      { type: "text", kind: "trend", label: "Expected", value: "Weekly ~88% at reset" },
    ] },
    { providerId: "codex", displayName: "Codex", plan: "Pro", lines: [
      { type: "progress", kind: "quota", label: "5h", used: 18, limit: 100, format: { kind: "percent" }, resetsAt: iso(now + 3.4 * HOUR) },
      { type: "progress", kind: "quota", label: "Weekly", used: 54, limit: 100, format: { kind: "percent" }, resetsAt: iso(now + 5.2 * DAY) },
      { type: "text", kind: "cost", label: "Last 30 Days", value: "$96.10 · 41.8M tokens" },
    ] },
    { providerId: "grok", displayName: "Grok", plan: "SuperGrok Heavy", lines: [
      { type: "progress", kind: "quota", label: "Weekly", used: 91, limit: 100, format: { kind: "percent" }, resetsAt: iso(now + 1.6 * DAY) },
      { type: "badge", kind: "plan", label: "Pay as you go", text: "Off" },
    ], resetInventory: { availability: "available", freshness: "fresh", observedAtMs: now - 5 * 60_000, source: "grok", error: null,
      value: { available: 2, detailsComplete: true, credits: [
        { validFromMs: now - 6 * DAY, expiresAtMs: now + 20 * HOUR },
        { validFromMs: now - 2 * DAY, expiresAtMs: now + 5 * DAY },
      ] } } },
    { providerId: "copilot", displayName: "Copilot", lines: [
      { type: "badge", kind: "error", label: "Error", text: "GitHub CLI session expired. Run `gh auth login` to sign in again." },
    ] },
  ];

  const accounts = fresh ? [] : [
    { id: "grok/personal", provider: "grok", alias: "personal", generation: "g-1", active: true, plan_label: "SuperGrok Heavy" },
    { id: "grok/work", provider: "grok", alias: "work", generation: "g-2", active: false, plan_label: "SuperGrok" },
    { id: "codex/personal", provider: "codex", alias: "personal", generation: "g-3", active: true, plan_label: "Pro" },
    { id: "openai/api", provider: "openai", alias: "api", generation: "g-4", active: true, plan_label: null },
  ];

  const clients = [
    ["opencode", "OpenCode"], ["claude", "Claude Code"], ["codex", "Codex CLI"], ["cursor", "Cursor IDE"], ["gemini", "Gemini CLI"],
    ["amp", "Amp"], ["copilot", "Copilot CLI"], ["goose", "Goose"], ["zed", "Zed Agent"], ["kiro", "Kiro"], ["trae", "Trae"],
    ["warp", "Warp"], ["cline", "Cline"], ["grok", "Grok Build"], ["antigravity", "Antigravity"],
  ].map(([id, name]) => ({ id, name, remote_collection: ["cursor", "trae", "warp"].includes(id), aggregate_only: false }));

  const clientWeights: Record<string, number> = fresh ? {} : { claude: 0.58, codex: 0.24, opencode: 0.12, grok: 0.06 };
  const modelWeights: Record<string, [number, number]> = fresh ? {} : {
    "claude-sonnet-4.5": [0.41, 3.1], "claude-opus-4.5": [0.17, 11.5], "gpt-5.2-codex": [0.24, 1.9], "grok-4.5": [0.12, 1.4], "claude-haiku-4.5": [0.06, 0.6],
  };
  const daily = Array.from({ length: 365 }, (_, index) => {
    const at = now - (364 - index) * DAY;
    const weekday = new Date(at).getDay();
    const base = fresh ? 0 : (weekday === 0 || weekday === 6 ? 2.2e6 : 7.5e6) * (0.5 + random());
    const tokens = Math.round(base);
    return { at, key: iso(at).slice(0, 10), tokens, unknown_token_records: 0, known_usd: Math.round(tokens * 2.3) / 1e6, partial: false, records: Math.round(tokens / 38_000) };
  });

  function usageReport(filter?: { since_ms?: number | null; client?: string | null }) {
    const since = filter?.since_ms ?? now - 31 * DAY;
    const days = daily.filter((day) => day.at >= since);
    const share = filter?.client ? clientWeights[filter.client] ?? 0 : 1;
    const scaled = days.map(({ at: _at, ...day }) => ({ ...day, tokens: Math.round(day.tokens * share), known_usd: Math.round(day.known_usd * share * 100) / 100, records: Math.round(day.records * share) }));
    const tokens = scaled.reduce((sum, day) => sum + day.tokens, 0);
    const usd = scaled.reduce((sum, day) => sum + day.known_usd, 0);
    const records = scaled.reduce((sum, day) => sum + day.records, 0);
    const total = (key: string, weight: number, price?: number) => ({ key, tokens: Math.round(tokens * weight), unknown_token_records: 0, known_usd: Math.round((price ? tokens * weight * price / 1e6 : usd * weight) * 100) / 100, partial: false, records: Math.round(records * weight) });
    const byClient = Object.entries(clientWeights).filter(([id]) => !filter?.client || id === filter.client).map(([id, weight]) => total(id, filter?.client ? 1 : weight));
    return {
      revision: 7,
      sources: clients.map((client) => ({ source: client.id, client: client.id, state: clientWeights[client.id] ? "ready" : client.remote_collection ? "needs_connection" : "no_data", last_success_ms: clientWeights[client.id] ? now - 4 * 60_000 : null, revision: 3, records: clientWeights[client.id] ? Math.round(records * clientWeights[client.id]) : 0, detail: client.remote_collection && !clientWeights[client.id] ? "Connect an account to import usage reports." : null })),
      total: { key: "total", tokens, unknown_token_records: fresh ? 0 : 12, known_usd: Math.round(usd * 100) / 100, partial: !fresh, records },
      daily: scaled,
      period_totals: [],
      clients: byClient,
      models: Object.entries(modelWeights).map(([model, [weight, price]]) => total(model, weight, price)),
      sessions: fresh ? [] : Array.from({ length: 8 }, (_, index) => total(`session-${(0xa3f0 + index * 977).toString(16)}`, 0.04 + index * 0.01)),
      projects: fresh ? [] : [total("~/dev/fabrials/spanreed", 0.34), total("~/dev/fabrials/ai-relay", 0.27), total("~/dev/radiant", 0.21), total("~/dev/notes", 0.08)],
      records: [],
      records_truncated: false,
    };
  }

  const history = fresh ? [] : Array.from({ length: 48 }, (_, index) => {
    const provider = ["claude", "codex", "grok"][index % 3];
    const label = index % 2 ? "Weekly" : provider === "codex" ? "5h" : "Session";
    const used = Math.min(100, Math.round((index * 7 + random() * 12) % 100 * 10) / 10);
    return { ts_ms: now - (48 - index) * 2.5 * HOUR, provider, plan: provider === "claude" ? "Max 5x" : provider === "codex" ? "Pro" : "SuperGrok Heavy", label, used, limit: 100, resets_at: null, window_start_ms: null, limit_window_secs: null, kind: "quota", event: index % 17 === 16 ? "reset" : null };
  });

  const models = ["grok-4.5", "grok-4.5-fast", "grok-code-fast-1", "grok-4.1-mini", "grok-imagine-image", "grok-imagine-video", "grok-2-vision"];
  const hops = fresh ? [] : Array.from({ length: 24 }, (_, index) => {
    const input = Math.round(8_000 + random() * 90_000);
    const output = Math.round(400 + random() * 6_000);
    const kind = index % 11 === 5 ? "tts" : "chat";
    return { ts_ms: now - index * 7 * 60_000, session_id: null, model: kind === "tts" ? "grok-tts" : models[index % 3], input_tokens: kind === "tts" ? 0 : input, output_tokens: kind === "tts" ? 0 : output, cached_input_tokens: Math.round(input * 0.6), reasoning_tokens: 0, total_tokens: kind === "tts" ? 0 : input + output, cost_usd_ticks: 0, request_id: `req_${(0x5e21 + index).toString(16)}`, account_id: index % 4 === 0 ? "grok/work" : "grok/personal", route: "grok", provider: "grok", key_hash: null, kind, duration_ms: Math.round(900 + random() * 14_000), status: index === 7 ? 429 : 200, unit: kind === "tts" ? "chars" : "tokens", quantity: kind === "tts" ? 1840 : null, hosted_tools: null };
  });

  return {
    detection, snapshot, accounts, clients, usageReport, history, hops, models,
    routing() {
      return {
        policies: ["grok", "codex", "nous", "openai"].map((provider) => ({ provider, autosteer: provider === "grok", exhausted_pct: provider === "grok" ? 90 : 100, mode: provider === "grok" ? "autosteer" : "active" })),
        limits: { mode: "autosteer", exhausted_pct: 100, providers: ["grok", "codex", "nous", "openai"].map((provider) => ({
          provider, route: provider === "grok" ? "/v1" : `/${provider}/v1`, autosteer: provider === "grok",
          accounts: accounts.filter((account) => account.provider === provider).map((account) => ({ alias: account.alias, plan: account.plan_label, used_pct: provider === "grok" ? (account.alias === "personal" ? 91 : 34) : provider === "codex" ? 54 : null, resets_at: iso(now + 1.6 * DAY), exhausted: provider === "grok" && account.alias === "personal", role: provider === "grok" ? (account.alias === "work" ? "next" : "—") : account.active ? "active" : "—" })),
        })) },
      };
    },
  };
}
