export type Workspace = "local" | "hosted";
export type Route = { workspace: Workspace; page: string; tab: string | null };

export const workspacePages: Record<Workspace, readonly string[]> = {
  local: ["overview", "usage", "accounts", "routing", "connect", "settings"],
  hosted: ["overview", "usage", "accounts", "routing", "keys", "connect", "settings"],
};

const MODE_KEY = "spanreed.mode";

export function storedWorkspace(storage: Pick<Storage, "getItem"> | undefined = globalThis.localStorage): Workspace {
  return storage?.getItem(MODE_KEY) === "remote" ? "hosted" : "local";
}

export function storeWorkspace(workspace: Workspace, storage: Pick<Storage, "setItem"> | undefined = globalThis.localStorage) {
  storage?.setItem(MODE_KEY, workspace === "hosted" ? "remote" : "local");
}

export function parseRoute(hash: string, fallback: Workspace = "local"): Route {
  const [first, page, tab] = hash.replace(/^#?\/?/, "").split("/");
  const workspace: Workspace = first === "hosted" || first === "local" ? first : fallback;
  if (!page || !workspacePages[workspace].includes(page)) return { workspace, page: "overview", tab: null };
  return { workspace, page, tab: tab ? decodeURIComponent(tab) : null };
}

export function routeHref(route: { workspace: Workspace; page: string; tab?: string | null }) {
  return `#/${route.workspace}/${route.page}${route.tab ? `/${encodeURIComponent(route.tab)}` : ""}`;
}
