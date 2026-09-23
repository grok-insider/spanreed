import { expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const flake = readFileSync(join(root, "../flake.nix"), "utf8");
const workflow = readFileSync(join(root, "../.github/workflows/desktop.yml"), "utf8");
const vendored = Object.entries(pkg.dependencies as Record<string, string>).filter(([name]) => name.startsWith("@fabrials/"));

test("both shared UI packages come from generated vendor distributions", () => {
  expect(vendored.map(([name]) => name).sort()).toEqual(["@fabrials/ai-ui", "@fabrials/ui"]);
  for (const [, spec] of vendored) {
    expect(spec).toMatch(/^file:vendor\/fabrials-(ai-)?ui-\d+\.\d+\.\d+$/);
    expect(existsSync(join(root, spec.slice(5), "fabrials-manifest.json"))).toBe(true);
  }
  expect(pkg.overrides["@fabrials/ui"]).toBe(pkg.dependencies["@fabrials/ui"]);
});

test("the Nix desktop build links the same vendor directories", () => {
  for (const [name, spec] of vendored) expect(flake).toContain(`ln -s ../../${spec.slice(5)} desktop/node_modules/${name}`);
  expect(flake).toContain("rm -rf node_modules/@fabrials\n");
});

test("CI verifies the vendored copies and runs these tests", () => {
  for (const [, spec] of vendored) expect(pkg.scripts["check:vendor"]).toContain(spec.slice(5));
  expect(workflow).toContain("bun run check:vendor");
  expect(workflow).toContain("bun run test");
});
