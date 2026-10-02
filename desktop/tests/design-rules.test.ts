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

test("tab strips scroll themselves instead of wrapping onto a second row", () => {
  for (const name of ["usage.tsx", "settings.tsx", "remote-workspace.tsx"]) {
    const text = sources.find(([file]) => file === name)![1];
    for (const tag of text.match(/<TabsList[^>]*>/g) ?? []) expect(tag, name).toContain("scrollable");
  }
});
