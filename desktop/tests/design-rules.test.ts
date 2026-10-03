import { expect, test } from "bun:test";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const src = join(dirname(fileURLToPath(import.meta.url)), "../src");
const sources = readdirSync(src).filter((name) => /\.tsx?$/.test(name)).map((name) => [name, readFileSync(join(src, name), "utf8")] as const);
const css = readFileSync(join(src, "desktop.css"), "utf8");

test("collapsible sections use the shared Disclosure, not a styled native details", () => {
  for (const [name, text] of sources) expect(text, name).not.toMatch(/<(details|summary)[\s>]/);
  expect(css).not.toContain(".sr-disclosure");
});

test("no link or button text ends in an arrow glyph", () => {
  for (const [name, text] of sources) {
    expect(text, name).not.toMatch(/\bArrow(Right|UpRight|Left)\b/);
    expect(text, name).not.toMatch(/>[^<>{}]*[→↗]\s*</);
  }
});

test("the page column anchors to the left gutter instead of centring", () => {
  const main = css.match(/\.sr-main \{[^}]*\}/)?.[0] ?? "";
  expect(main).toContain("overflow-y: auto");
  expect(main).not.toMatch(/margin-inline:\s*auto/);
});

test("page rows keep their content height so cards are never clipped in a short window", () => {
  const main = css.match(/\.sr-main \{[^}]*\}/)?.[0] ?? "";
  expect(main).toContain("grid-auto-rows: max-content");
});

test("tab strips scroll themselves instead of wrapping onto a second row", () => {
  for (const name of ["usage.tsx", "settings.tsx", "remote-workspace.tsx"]) {
    const text = sources.find(([file]) => file === name)![1];
    for (const tag of text.match(/<TabsList[^>]*>/g) ?? []) expect(tag, name).toContain("scrollable");
  }
});

test("the workspace switcher still marks the current place in forced colours", () => {
  const block = css.match(/@media \(forced-colors: active\) \{[^}]*\.sr-switcher-option\[aria-current\][^}]*\}[^}]*\}/)?.[0] ?? "";
  expect(block).toContain("outline");
});

test("the header is one row: its actions never wrap, and below 480 px the refresh label and the updated note give way", () => {
  const actions = css.match(/\.sr-header-actions \{[^}]*\}/)?.[0] ?? "";
  expect(actions).toContain("flex-wrap: nowrap");
  expect(actions).not.toContain("flex-wrap: wrap");
  const narrow = css.match(/@media \(max-width: 480px\) \{[^}]*\}/)?.[0] ?? "";
  expect(narrow).toContain(".sr-refresh-label");
  expect(narrow).toContain(".sr-updated");
  // the button keeps a name when its label is hidden
  expect(sources.find(([name]) => name === "app.tsx")?.[1]).toMatch(/aria-label=\{data\.loading \? "Refreshing" : "Refresh"\}/);
});

