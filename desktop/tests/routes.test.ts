import { expect, test } from "bun:test";
import { parseRoute, routeHref, storedWorkspace, storeWorkspace } from "../src/routes";

test("hash routes round-trip for both workspaces", () => {
  for (const route of [
    { workspace: "local" as const, page: "usage", tab: "requests" },
    { workspace: "local" as const, page: "settings", tab: "sharing" },
    { workspace: "hosted" as const, page: "keys", tab: null },
  ]) expect(parseRoute(routeHref(route))).toEqual(route);
});

test("unknown pages fall back to the overview of the requested workspace", () => {
  expect(parseRoute("#/local/providers")).toEqual({ workspace: "local", page: "overview", tab: null });
  expect(parseRoute("#/hosted/keys/x")).toEqual({ workspace: "hosted", page: "keys", tab: "x" });
  expect(parseRoute("#/local/keys")).toEqual({ workspace: "local", page: "overview", tab: null });
});

test("an empty hash opens the last workspace", () => {
  expect(parseRoute("", "hosted")).toEqual({ workspace: "hosted", page: "overview", tab: null });
  expect(parseRoute("#", "local")).toEqual({ workspace: "local", page: "overview", tab: null });
});

test("workspace choice keeps the existing spanreed.mode values", () => {
  const values = new Map<string, string>();
  const storage = { getItem: (key: string) => values.get(key) ?? null, setItem: (key: string, value: string) => { values.set(key, value); } };
  expect(storedWorkspace(storage)).toBe("local");
  storeWorkspace("hosted", storage);
  expect(values.get("spanreed.mode")).toBe("remote");
  expect(storedWorkspace(storage)).toBe("hosted");
  storeWorkspace("local", storage);
  expect(values.get("spanreed.mode")).toBe("local");
});
