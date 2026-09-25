import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

test("Tauri window has decorations off and chrome window permissions", () => {
  const conf = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"));
  const window = conf.app.windows.find((item: { label: string }) => item.label === "main");
  expect(window.decorations).toBe(false);
  const capabilities = JSON.parse(readFileSync(join(root, "src-tauri/capabilities/default.json"), "utf8"));
  for (const permission of [
    "core:window:allow-set-theme",
    "core:window:allow-minimize",
    "core:window:allow-toggle-maximize",
    "core:window:allow-close",
    "core:window:allow-start-dragging",
  ]) {
    expect(capabilities.permissions).toContain(permission);
  }
});
