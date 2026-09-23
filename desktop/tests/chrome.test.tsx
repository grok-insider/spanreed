import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { Boxes, CircleGauge } from "lucide-react";
import { DesktopShell, settingsItem } from "../src/desktop-shell";
import { WindowChrome, runWindowChromeAction, type WindowChromeHost } from "../src/window-chrome";

function fakeHost(): WindowChromeHost & { calls: string[] } {
  const calls: string[] = [];
  return {
    calls,
    minimize: () => { calls.push("minimize"); },
    toggleMaximize: () => { calls.push("maximize"); },
    close: () => { calls.push("close"); },
  };
}

function render(workspace: "local" | "hosted" = "local", page = "accounts") {
  return renderToStaticMarkup(
    <DesktopShell
      route={{ workspace, page, tab: null }}
      groups={[{ label: "Monitor", items: [{ page: "overview", label: "Overview", icon: CircleGauge }] }, { label: "Set up", items: [{ page: "accounts", label: "Accounts", icon: Boxes }] }]}
      footer={[settingsItem]}
      actions={<button type="button">Refresh</button>}
      chrome={<WindowChrome host={fakeHost()} />}
    >
      <p>body</p>
    </DesktopShell>,
  );
}

test("brand mark is the ruby gem, not the letter F", () => {
  const html = render();
  expect(html).not.toContain(">F<");
  expect(html).toContain("fui-gem");
  expect(html).toContain('data-gem="ruby"');
  expect(html).toContain("Spanreed");
});

test("title bar keeps the drag region, page actions and window controls", () => {
  const html = render();
  expect(html).toContain("data-tauri-drag-region");
  for (const label of ["Minimize", "Maximize", "Close"]) expect(html).toContain(`aria-label="${label}"`);
  expect(html).toContain("Refresh");
});

test("navigation is grouped, linkable and marks the current page", () => {
  const html = render("local", "accounts");
  expect(html).toContain(">Monitor<");
  expect(html).toContain(">Set up<");
  expect(html).toMatch(/href="#\/local\/accounts"[^>]*aria-current="page"|aria-current="page"[^>]*href="#\/local\/accounts"/);
  expect(html).toContain('href="#/local/overview"');
  expect(html).toContain('href="#/local/settings"');
});

test("workspace switcher names places and keeps the page inside the chosen workspace", () => {
  const local = render("local", "overview");
  expect(local).toContain("This computer");
  expect(local).toContain("Hosted relay");
  expect(local).toMatch(/href="#\/local\/overview" aria-current="true"/);
  const hosted = render("hosted", "accounts");
  expect(hosted).toMatch(/href="#\/hosted\/overview" aria-current="true"/);
  expect(hosted).toMatch(/href="#\/hosted\/accounts"[^>]*aria-current="page"/);
});

test("window chrome actions invoke minimize, maximize, and close on the host", async () => {
  const host = fakeHost();
  await runWindowChromeAction("minimize", host);
  await runWindowChromeAction("maximize", host);
  await runWindowChromeAction("close", host);
  expect(host.calls).toEqual(["minimize", "maximize", "close"]);
});
