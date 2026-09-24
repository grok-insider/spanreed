import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Boxes, ChartColumn, CircleGauge, Plug, RefreshCw, Route as RouteIcon } from "lucide-react";
import { Button, StatusDot } from "@fabrials/ui";
import { useClock } from "@fabrials/ai-ui";
import { DesktopShell, settingsItem, type NavGroup } from "./desktop-shell";
import { LocalDataProvider, useLocalData } from "./local-data";
import { parseRoute, routeHref, storedWorkspace, storeWorkspace, type Route } from "./routes";
import { useThemePreference, type ThemePreference } from "./theme";
import { ago, attentionItems } from "./format";
import { ErrorAlert } from "./feedback";
import { OverviewPage } from "./overview";
import { UsagePage } from "./usage";
import { AccountsPage } from "./accounts";
import { RoutingPage } from "./routing";
import { ConnectPage } from "./connect";
import { SettingsPage } from "./settings";
import { HostedWorkspace } from "./remote-workspace";

function useHashRoute() {
  const [route, setRoute] = React.useState<Route>(() => parseRoute(location.hash, storedWorkspace()));
  React.useEffect(() => {
    const sync = () => setRoute(parseRoute(location.hash, storedWorkspace()));
    if (!location.hash) history.replaceState(null, "", routeHref(parseRoute("", storedWorkspace())));
    addEventListener("hashchange", sync);
    return () => removeEventListener("hashchange", sync);
  }, []);
  React.useEffect(() => { storeWorkspace(route.workspace); }, [route.workspace]);
  const first = React.useRef(true);
  React.useEffect(() => {
    if (first.current) { first.current = false; return; }
    document.querySelector(".sr-main")?.scrollTo(0, 0);
    document.getElementById("main-content")?.focus({ preventScroll: true });
  }, [route.workspace, route.page]);
  return route;
}

function LocalWorkspace({ route, theme, onThemeChange, appError }: { route: Route; theme: ThemePreference; onThemeChange: (theme: ThemePreference) => void; appError: string | null }) {
  const data = useLocalData();
  const now = useClock() ?? Date.now();
  const attention = attentionItems(data.outputs, now).length;
  const running = data.proxy?.state === "running";
  const groups: NavGroup[] = [
    { label: "Monitor", items: [
      { page: "overview", label: "Overview", icon: CircleGauge, badge: attention ? <span className="sr-nav-count"><span aria-hidden>{attention}</span><span className="fui-sr-only">, {attention} need attention</span></span> : undefined },
      { page: "usage", label: "Usage", icon: ChartColumn },
    ] },
    { label: "Set up", items: [
      { page: "accounts", label: "Accounts", icon: Boxes },
      { page: "routing", label: "Routing", icon: RouteIcon },
      { page: "connect", label: "Connect", icon: Plug, badge: running ? <span className="sr-nav-dot"><span className="fui-sr-only">, proxy running</span></span> : undefined },
    ] },
  ];
  const status = <StatusDot tone={running ? "success" : "neutral"} label={running ? `Proxy on ${data.proxy!.bind}` : "Proxy off"} />;
  const actions = <>
    {data.updatedAt && <span className="sr-updated">Updated {ago(data.updatedAt, now)}</span>}
    <Button variant="outline" size="sm" disabled={data.loading} onClick={() => void data.refresh()}>
      <RefreshCw aria-hidden size={14} className={data.loading ? "fui-spin" : undefined} />{data.loading ? "Refreshing…" : "Refresh"}
    </Button>
  </>;
  return <DesktopShell route={route} groups={groups} footer={[settingsItem]} status={status} actions={actions} theme={theme} onThemeChange={onThemeChange}>
    <ErrorAlert title="Couldn't update the window theme" error={appError} />
    {route.page === "overview" && <OverviewPage />}
    {route.page === "usage" && <UsagePage tab={route.tab} />}
    {route.page === "accounts" && <AccountsPage tab={route.tab} />}
    {route.page === "routing" && <RoutingPage />}
    {route.page === "connect" && <ConnectPage />}
    {route.page === "settings" && <SettingsPage tab={route.tab} theme={theme} onThemeChange={onThemeChange} />}
  </DesktopShell>;
}

export function App() {
  const [appError, setAppError] = React.useState<string | null>(null);
  const [theme, setTheme] = useThemePreference(setAppError);
  const route = useHashRoute();
  React.useEffect(() => {
    let active = true;
    const pull = () => {
      void invoke<string | null>("take_desktop_route").then((href) => {
        if (active && href && location.hash !== href) location.hash = href;
      }).catch(() => undefined);
    };
    pull();
    const timer = window.setInterval(pull, 1000);
    return () => { active = false; window.clearInterval(timer); };
  }, []);
  if (route.workspace === "hosted") return <HostedWorkspace route={route} theme={theme} onThemeChange={setTheme} />;
  return <LocalDataProvider><LocalWorkspace route={route} theme={theme} onThemeChange={setTheme} appError={appError} /></LocalDataProvider>;
}
