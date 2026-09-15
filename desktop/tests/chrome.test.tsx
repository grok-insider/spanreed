import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { WorkspaceShell } from "@fabrials/ui";
import { FABRIAL_MARK_DARK, FABRIAL_MARK_LIGHT, FabrialBrandMark } from "../src/brand-mark";
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

test("brand mark is the Spanreed fabrial image, not the letter F", () => {
  const html = renderToStaticMarkup(
    <WorkspaceShell
      title="Spanreed"
      navigation={[{ id: "providers", label: "Providers" }]}
      active="providers"
      onNavigate={() => {}}
      brandMark={<FabrialBrandMark />}
      chrome={<WindowChrome host={fakeHost()} />}
      actions={<button type="button">Refresh</button>}
    >
      <p>body</p>
    </WorkspaceShell>,
  );
  expect(html).not.toContain(">F<");
  expect(html).toContain("<img");
  expect(html).toContain(FABRIAL_MARK_DARK);
  expect(html).toContain(FABRIAL_MARK_LIGHT);
  expect(html).toContain("Spanreed");
  expect(html).toContain("data-tauri-drag-region");
  expect(html).toContain("aria-label=\"Minimize\"");
  expect(html).toContain("aria-label=\"Maximize\"");
  expect(html).toContain("aria-label=\"Close\"");
  expect(html).toContain("Refresh");
  expect(html).toContain("Providers");
});

test("window chrome actions invoke minimize, maximize, and close on the host", async () => {
  const host = fakeHost();
  await runWindowChromeAction("minimize", host);
  await runWindowChromeAction("maximize", host);
  await runWindowChromeAction("close", host);
  expect(host.calls).toEqual(["minimize", "maximize", "close"]);
});
