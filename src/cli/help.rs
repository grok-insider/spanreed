//! `spanreed help`.

pub(super) fn print() {
    println!(
        "spanreed — Linux AI subscription usage tracker\n\n\
         USAGE:\n\
         \tspanreed list                 Show providers and whether they're detected\n\
         \tspanreed probe [id] [--force] Probe detected providers, or a single id\n\
         \t  (default: rate-limit quotas only)\n\
         \t  --cost --models --cache --trend --plan   Add detail blocks\n\
         \t  --all                                    Full verbose output\n\
         \tspanreed waybar               Waybar custom-module JSON (one shot)\n\
         \tspanreed json                 Raw JSON of detected provider outputs\n\
         \tspanreed serve [--interval S] Local HTTP API on 127.0.0.1:6736\n\
         \tspanreed history [id]         Show recorded rate-limit history (JSONL)\n\
         \tspanreed capture serve        Fabric :18736  /v1 grok  /xai api.x.ai  /acct/ID\n\
         \t                               (honors HTTP(S)_PROXY for upstream egress)\n\
         \t  --watchdog                   Keep capture alive (restart on exit; logs to\n\
         \t                               %%LOCALAPPDATA%%/spanreed/logs/capture.log;\n\
         \t                               Windows: windowless / FreeConsole)\n\
         \tspanreed capture ensure      Start capture+watchdog if ports are down\n\
         \tspanreed capture status      Exit 0 if listening, 1 if DOWN; print log path\n\
         \tspanreed agent serve|open|status|doctor|stop|repair|workspace\n\
         \t                               Local host for desktop.grok.me (Grok Build over ACP;\n\
         \t                               also runs when invoked as `grok-bridge`)\n\
         \tspanreed grok-proxy [--bind HOST:PORT]\n\
         \t                               Alias for `capture serve --grok-cli-bind`\n\
         \tspanreed setup               Install CLI, ledger, optional capture service,\n\
         \t                               and wire Grok Build + OpenCode xAI to the proxy\n\
         \t  --yes / -y                   Non-interactive defaults (service off unless --service)\n\
         \t  --service                    Enable capture user service (with --yes)\n\
         \t  --dry-run --no-wire --from-current-exe\n\
         \tspanreed setup status        Show install / wire / service state\n\
         \t                               (exit 1 if clients wired but proxy DOWN)\n\
         \tspanreed setup uninstall     Unwire clients and disable capture service\n\
         \tspanreed auth copilot         Link Copilot (opt-in; pick gh user or paste)\n\
         \t  --user LOGIN                 Import token for that gh account\n\
         \t  --token-stdin                Read token from stdin\n\
         \tspanreed auth logout copilot  Remove the stored Copilot credential\n\
         \tspanreed update-pricing [out] Fetch LiteLLM prices plus the OpenCode Go\n\
         \t                               channel from models.dev (writes to stdout, or to [out];\n\
         \t                               used to refresh the fabrials-pricing price snapshot)\n\
         \tspanreed share               Upload plan/quota metrics (requires X login)\n\
         \tspanreed share login         Link CLI via device code on fabrials.com\n\
         \tspanreed share logout|status Session management\n\
         \tspanreed usage [--client ID] [--days N] [--refresh]  Read local consumption\n\
         \tspanreed usage sources        List consumption sources and connections\n\
         \tspanreed sync                Synchronize selected private usage sources with Fabrials\n\
         \t                               (SPANREED_API_BASE optional)\n\
         \t                               At most once per day; setup installs\n\
         \t                               evening timer + login/missed-run catch-up\n\
         \tspanreed self-update         Install latest GitHub Release (sha256 verified)\n\
         \t  --check [--json]             Report only (exit 2 if newer)\n\
         \t  --yes --dry-run              Apply without prompt / download-only verify\n\
         \tspanreed tray [--interval S] System tray companion (needs --features tray)\n\
         \tspanreed account …           Identities (add/import/use/login grok)\n\
         \tspanreed plugin list         Drivers (in-process / toml / PATH)\n\n\
         PROVIDERS: `spanreed list` shows every provider and whether it is detected.\n\
         \t           `spanreed probe <id>` fetches one provider.\n\
         \t           (copilot requires `spanreed auth copilot`)"
    );
}
