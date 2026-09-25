import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauri } from "@tauri-apps/api/core";
import { Button } from "@fabrials/ui";

export type WindowChromeHost = {
  minimize: () => Promise<void> | void;
  toggleMaximize: () => Promise<void> | void;
  close: () => Promise<void> | void;
};

export async function runWindowChromeAction(
  action: "minimize" | "maximize" | "close",
  host: WindowChromeHost,
): Promise<void> {
  if (action === "minimize") await host.minimize();
  else if (action === "maximize") await host.toggleMaximize();
  else await host.close();
}

export function tauriWindowChromeHost(): WindowChromeHost {
  const window = getCurrentWindow();
  return {
    minimize: () => window.minimize(),
    toggleMaximize: () => window.toggleMaximize(),
    close: () => window.close(),
  };
}

export function WindowChrome({ host }: { host: WindowChromeHost }) {
  return <div className="sr-window-controls" role="group" aria-label="Window">
    <Button variant="ghost" size="icon-sm" aria-label="Minimize" onClick={() => void runWindowChromeAction("minimize", host)}>
      <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><rect x="1" y="5.25" width="10" height="1.5" fill="currentColor"/></svg>
    </Button>
    <Button variant="ghost" size="icon-sm" aria-label="Maximize" onClick={() => void runWindowChromeAction("maximize", host)}>
      <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><rect x="1.5" y="1.5" width="9" height="9" fill="none" stroke="currentColor" strokeWidth="1.5"/></svg>
    </Button>
    <Button variant="ghost" size="icon-sm" className="sr-window-close" aria-label="Close" onClick={() => void runWindowChromeAction("close", host)}>
      <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M2 2 L10 10 M10 2 L2 10" stroke="currentColor" strokeWidth="1.5"/></svg>
    </Button>
  </div>;
}

export function DesktopWindowChrome() {
  if (!isTauri()) return null;
  return <WindowChrome host={tauriWindowChromeHost()} />;
}
