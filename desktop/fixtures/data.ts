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

  // The hosted relay (ai.fabrials.com) as a Fabrials-linked computer sees it. Synthetic, like everything here.
  const hostedOutput = (providerId: string, displayName: string, plan: string, used: number, resetsInDays: number) => ({
    providerId, displayName, plan,
    lines: [{ type: "progress", kind: "quota", label: "Weekly", used, limit: 100, format: { kind: "percent" }, resetsAt: iso(now + resetsInDays * DAY) }],
  });
  const hostedAccounts = [
    { usage_output: hostedOutput("grok", "Grok", "SuperGrok Heavy", 64, 2.4), reset_inventory: null, balance: null, id: "grok/relay", provider: "grok", alias: "relay", label: "relay", active: true, plan: "SuperGrok Heavy", used_pct: 64, resets_at: iso(now + 2.4 * DAY), has_secret: true, quota_detail: null, catalog: null, email: null, auth_type: "oauth", needs_reauth: false },
    { usage_output: hostedOutput("codex", "Codex", "Pro", 31, 4.1), reset_inventory: null, balance: null, id: "codex/shared", provider: "codex", alias: "shared", label: "shared", active: true, plan: "Pro", used_pct: 31, resets_at: iso(now + 4.1 * DAY), has_secret: true, quota_detail: null, catalog: null, email: null, auth_type: "oauth", needs_reauth: false },
    { usage_output: null, reset_inventory: null, balance: { availability: "available", freshness: "fresh", observedAtMs: now - 12 * 60_000, source: "openai", value: { remainingUsd: 38.42, subscriptionRemainingUsd: null, subscriptionLimitUsd: null, purchasedRemainingUsd: 38.42, paidAccess: true }, error: null }, id: "openai/api", provider: "openai", alias: "api", label: "api", active: true, plan: null, used_pct: null, resets_at: null, has_secret: true, quota_detail: null, catalog: null, email: null, auth_type: "api_key", needs_reauth: false },
    { usage_output: null, reset_inventory: null, balance: null, id: "nous/old", provider: "nous", alias: "old", label: "old", active: false, plan: null, used_pct: 12, resets_at: iso(now + 1 * DAY), has_secret: true, quota_detail: null, catalog: null, email: null, auth_type: "oauth", needs_reauth: true },
  ];
  const hostedTotals = (scale: number) => ({ requests: Math.round(412 * scale), input_tokens: Math.round(9_800_000 * scale), output_tokens: Math.round(610_000 * scale), cached_input_tokens: Math.round(6_100_000 * scale), usd: Math.round(18.4 * scale * 100) / 100 });
  const hostedDashboard = {
    generated_at_ms: now, usage_truncated: false,
    session: { owner: "u_fixture", username: "ash" },
    relay: { id: "relay-1", version: "0.8.0", mode: "vps", capabilities: ["accounts", "keys"], endpoints: [{ key: "default", label: "ai.fabrials.com", http_url: "https://ai.fabrials.com", websocket_url: null, reachability: "public", available: true, is_default: true }] },
    summary: {
      days: 7, today: hostedTotals(0.2), window: hostedTotals(1),
      accounts: hostedAccounts.map((account, index) => ({ id: account.id, alias: account.alias, provider: account.provider, plan: account.plan, used_pct: account.used_pct, active: account.active, requests: 180 - index * 40, tokens: 4_200_000 - index * 900_000, usd: 8.2 - index * 2 })),
      models: [{ model: "grok-4.5", requests: 220, tokens: 5_100_000, usd: 9.1 }, { model: "gpt-5.2-codex", requests: 140, tokens: 3_300_000, usd: 7.4 }, { model: "grok-4.5-fast", requests: 52, tokens: 1_000_000, usd: 1.9 }],
      recent: Array.from({ length: 12 }, (_, index) => ({ ts_ms: now - index * 11 * 60_000, account_id: index % 3 ? "grok/relay" : "codex/shared", model: index % 3 ? "grok-4.5" : "gpt-5.2-codex", kind: "chat", input_tokens: Math.round(6_000 + random() * 70_000), output_tokens: Math.round(300 + random() * 5_000), duration_ms: Math.round(800 + random() * 9_000), status: index === 4 ? 429 : 200, usd: Math.round(random() * 0.9 * 10_000) / 10_000 })),
    },
    accounts: hostedAccounts,
    synchronized_accounts: [
      { linked_account_id: "grok/relay", device: "desk", source: "grok", observed_at_ms: now - 8 * 60_000, output: hostedOutput("grok", "Grok", "SuperGrok Heavy", 61, 2.4), local_usage: null },
      { linked_account_id: null, device: "laptop", source: "claude", observed_at_ms: now - 3 * HOUR, output: hostedOutput("claude", "Claude", "Max 5x", 42, 3.1), local_usage: null },
    ],
    keys: [
      { key_hash: "kh_1", prefix: "sk-relay-4f2a", enabled: true, spent_usd: 6.12, policy: { pool_policy: false, name: "desk tools", budget_usd: 25, budget_period: "month", allow_providers: ["grok", "codex"], allow_accounts: [], allow_models: [], allow_routes: [], allow_kinds: [] } },
      { key_hash: "kh_2", prefix: "sk-relay-9b10", enabled: false, spent_usd: 0, policy: { pool_policy: false, name: "ci", budget_usd: null, budget_period: "month", allow_providers: [], allow_accounts: [], allow_models: ["grok-4.5-fast"], allow_routes: [], allow_kinds: ["chat"] } },
    ],
    steering: ["grok", "codex"].map((provider) => ({ provider, route: provider === "grok" ? "/v1" : `/${provider}/v1`, enabled: provider === "grok", mode: provider === "grok" ? "autosteer" : "active", exhausted_pct: 90, queue: hostedAccounts.filter((account) => account.provider === provider).map((account) => ({ id: account.id, alias: account.alias, plan: account.plan, used_pct: account.used_pct, resets_at: account.resets_at, active: account.active, exhausted: false, score: 0.8, factors: "plan weight, headroom" })) })),
    catalog: [{ provider: "grok", model: "grok-4.5", account_alias: "relay", probed_at_ms: now - HOUR }, { provider: "codex", model: "gpt-5.2-codex", account_alias: "shared", probed_at_ms: now - HOUR }],
    connectors: [], fab_pool: null,
  };
  const hostedConsumption = {
    snapshots: [{
      device: "desk", source: "claude", revision: 3, observed_at_ms: now - 20 * 60_000, partial: false,
      days: Array.from({ length: 14 }, (_, index) => ({ date: iso(now - (13 - index) * DAY).slice(0, 10), tokens: Math.round(2e6 + random() * 6e6), estimated_usd: Math.round(random() * 3000) / 100 })),
      period_totals: { tokens: 62_000_000, known_usd: 188.4, partial: false },
      models: [{ model: "claude-sonnet-4.5", requests: 640, tokens: 41_000_000, estimated_usd: 120.5 }, { model: "claude-opus-4.5", requests: 90, tokens: 21_000_000, estimated_usd: 67.9 }],
    }],
  };

  return {
    detection, snapshot, accounts, clients, usageReport, history, hops, models, hostedDashboard, hostedConsumption,
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
