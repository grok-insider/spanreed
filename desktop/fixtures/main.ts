import { mockIPC } from "@tauri-apps/api/mocks";
import { createFixtures } from "./data";

const params = new URLSearchParams(location.search);
const scenario = params.get("scenario") === "fresh" ? "fresh" : "populated";
for (const [key, name] of [["theme", "spanreed.theme"], ["mode", "spanreed.mode"]] as const) {
  const value = params.get(key);
  if (value) localStorage.setItem(name, value);
}
const fixtures = createFixtures(scenario);
const state = {
  accounts: fixtures.accounts.map((account) => ({ ...account })),
  consent: { shareMetrics: false, syncHistory: false },
  proxy: { state: "stopped", bind: "127.0.0.1:18736", error: null as string | null },
  notifications: { resetExpiry: false },
  link: { state: "disconnected" } as Record<string, unknown>,
  syncSources: [] as string[],
  usageSettings: { additional_roots: {} as Record<string, string[]>, disabled_clients: [] as string[] },
  connections: [] as string[],
  logins: 0,
};
const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

mockIPC(async (cmd, payload) => {
  const args = (payload ?? {}) as Record<string, any>;
  await delay(cmd === "snapshot" || cmd === "usage_report" ? 250 : 60);
  switch (cmd) {
    case "snapshot": return fixtures.snapshot;
    case "accounts": return { accounts: state.accounts };
    case "detection": return fixtures.detection;
    case "privacy": return state.consent;
    case "set_privacy": state.consent = args.consent; return null;
    case "check_reset_notifications": return null;
    case "usage_report": return fixtures.usageReport(args.filter);
    case "usage_sources": return { clients: fixtures.clients, settings: state.usageSettings, connections: state.connections };
    case "save_usage_sources": state.usageSettings = args.settings; return null;
    case "connect_usage_source": state.connections = [...new Set([...state.connections, args.connection.client])]; return null;
    case "disconnect_usage_source": state.connections = state.connections.filter((client) => client !== args.client); return null;
    case "routing": return fixtures.routing();
    case "set_routing": return null;
    case "history": return fixtures.history;
    case "hops": return fixtures.hops;
    case "local_proxy_status": return state.proxy;
    case "start_local_proxy": state.proxy = { state: "running", bind: args.bind, error: null }; return state.proxy;
    case "stop_local_proxy": state.proxy = { ...state.proxy, state: "stopped" }; return state.proxy;
    case "models": return { accountId: args.accountId, observedAtMs: Date.now(), models: fixtures.models };
    case "notification_settings": return state.notifications;
    case "set_reset_notifications": state.notifications = { resetExpiry: args.enabled }; return null;
    case "test_reset_notification": return null;
    case "fabrials_status": return state.link;
    case "fabrials_begin": state.link = { state: "pending", id: "link-1", userCode: "WQXR-7KDM", verificationUri: "https://fabrials.com/spanreed/link", expiresAtMs: Date.now() + 600_000, retryAfterSecs: 5 }; return state.link;
    case "fabrials_poll": return state.link;
    case "fabrials_cancel": case "fabrials_disconnect": state.link = { state: "disconnected" }; return null;
    case "fabrials_open": case "open_hosted": case "open_device_login": case "remote_open_authorization": return null;
    case "sync_settings": return { sources: state.syncSources };
    case "save_sync_settings": state.syncSources = args.settings.sources; return null;
    case "sync_status": return { lastSuccessMs: null, uploaded: 0, downloaded: 0, error: null };
    case "sync_now": return "Synchronized 0 observations.";
    case "publication_status": return { lastSharedDay: null, due: true, schedule: "systemd-user (spanreed-share.timer: inactive); last shared: never" };
    case "publish_metrics": case "set_publication_schedule": return "Saved.";
    case "private_history": return { observations: [], before: null, has_more: false };
    case "saved_migrations": return [];
    case "begin_device_login": case "begin_inactive_device_login": case "reauthorize_account":
      state.logins += 1;
      return { id: `login-${state.logins}`, alias: args.alias ?? "personal", provider: args.provider ?? "grok", device: { verificationUri: "https://accounts.x.ai/device", userCode: "HJ4K-92PL", expiresIn: 900, interval: 5 } };
    case "poll_device_login": return { state: "pending", retryAfterSecs: 5 };
    case "cancel_device_login": return null;
    case "add_api_key":
      state.accounts.push({ id: `${args.provider}/${args.alias}`, provider: args.provider, alias: args.alias, generation: "g-new", active: !state.accounts.some((account) => account.provider === args.provider), plan_label: null });
      return null;
    case "activate_account":
      state.accounts = state.accounts.map((account) => account.provider === state.accounts.find((item) => item.id === args.id)?.provider ? { ...account, active: account.id === args.id } : account);
      return null;
    case "remove_account": state.accounts = state.accounts.filter((account) => account.id !== args.id); return null;
    case "replace_api_key": return null;
    case "preview_opencode_configuration": case "preview_opencode_update": case "preview_opencode_remove": case "preview_grok_configuration":
      return { warnings: [], id: "review-1", path: "~/.config/opencode/opencode.json", providerId: `spanreed-${args.alias}`, addition: { npm: "@ai-sdk/openai-compatible", options: { baseURL: `http://${state.proxy.bind}/acct/${args.alias}/v1` } }, client: cmd === "preview_grok_configuration" ? "grok" : "opencode", operation: cmd.endsWith("remove") ? "remove" : cmd.endsWith("update") ? "update" : "create" };
    case "apply_client_configuration": return null;
    case "remote_request":
      if (args.operation === "dashboard") throw "Connect this installation to Fabrials in Settings to open your hosted workspace.";
      throw "Hosted workspace is unavailable in fixtures.";
    default:
      console.warn("Unhandled fixture command", cmd, args);
      throw `Fixture has no response for ${cmd}`;
  }
});

await import("../src/main.tsx");
