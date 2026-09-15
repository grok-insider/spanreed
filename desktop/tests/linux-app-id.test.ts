import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const flake = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../../flake.nix"), "utf8");

test("Linux desktop identity matches the hicolor icon name", () => {
  expect(flake).toContain('Icon=com.fabrials.spanreed');
  expect(flake).toContain('StartupWMClass=com.fabrials.spanreed');
  expect(flake).toContain('apps/com.fabrials.spanreed.png');
  expect(flake).not.toContain("StartupWMClass=spanreed-desktop");
});
