"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { useContext, useState, createContext } from "react";
import { File as File$1, ChevronRight, FolderOpen, Folder as Folder$1, Star, GitFork } from "lucide-react";
import { classes } from "./shared.js";
const Depth = createContext(0);
function Files({ className, children, ...props }) {
  return /* @__PURE__ */ jsx("div", { "data-slot": "files", className: classes("fui-files", className), children: /* @__PURE__ */ jsx("ul", { role: "list", ...props, children }) });
}
function Folder({
  name,
  defaultOpen = false,
  note,
  children
}) {
  const depth = useContext(Depth);
  const [open, setOpen] = useState(defaultOpen);
  const Icon = open ? FolderOpen : Folder$1;
  return /* @__PURE__ */ jsxs("li", { className: "fui-files-item", style: { "--fui-files-depth": depth }, children: [
    /* @__PURE__ */ jsxs("button", { type: "button", className: "fui-files-row", "aria-expanded": open, onClick: () => setOpen(!open), children: [
      /* @__PURE__ */ jsx(ChevronRight, { "aria-hidden": true, size: 14, className: "fui-files-chevron" }),
      /* @__PURE__ */ jsx(Icon, { "aria-hidden": true, size: 15, className: "fui-files-icon" }),
      /* @__PURE__ */ jsx("span", { className: "fui-files-name", children: name }),
      note ? /* @__PURE__ */ jsx("span", { className: "fui-files-note", children: note }) : null
    ] }),
    open && children ? /* @__PURE__ */ jsx(Depth.Provider, { value: depth + 1, children: /* @__PURE__ */ jsx("ul", { role: "list", children }) }) : null
  ] });
}
function File({
  name,
  icon,
  note,
  highlighted = false
}) {
  const depth = useContext(Depth);
  return /* @__PURE__ */ jsx("li", { className: "fui-files-item", style: { "--fui-files-depth": depth }, children: /* @__PURE__ */ jsxs("span", { className: "fui-files-row", "data-highlighted": highlighted || void 0, children: [
    /* @__PURE__ */ jsx("span", { className: "fui-files-chevron", "aria-hidden": true }),
    icon ?? /* @__PURE__ */ jsx(File$1, { "aria-hidden": true, size: 15, className: "fui-files-icon" }),
    /* @__PURE__ */ jsx("span", { className: "fui-files-name", children: name }),
    note ? /* @__PURE__ */ jsx("span", { className: "fui-files-note", children: note }) : null
  ] }) });
}
const compact = new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 });
function GitHubMark() {
  return /* @__PURE__ */ jsx("svg", { "aria-hidden": true, viewBox: "0 0 16 16", width: "14", height: "14", fill: "currentColor", children: /* @__PURE__ */ jsx("path", { d: "M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.65 7.65 0 0 1 4 0c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" }) });
}
function RepoInfo({
  owner,
  repo,
  href,
  stars,
  forks,
  description,
  className,
  ...props
}) {
  const known = (value) => typeof value === "number" && Number.isFinite(value) && value >= 0;
  return /* @__PURE__ */ jsxs(
    "a",
    {
      "data-slot": "repo-info",
      href: href ?? `https://github.com/${owner}/${repo}`,
      target: "_blank",
      rel: "noreferrer noopener",
      className: classes("fui-repo-info", className),
      ...props,
      children: [
        /* @__PURE__ */ jsxs("span", { className: "fui-repo-info-name", children: [
          /* @__PURE__ */ jsx(GitHubMark, {}),
          /* @__PURE__ */ jsxs("span", { children: [
            owner,
            "/",
            /* @__PURE__ */ jsx("strong", { children: repo })
          ] })
        ] }),
        description ? /* @__PURE__ */ jsx("span", { className: "fui-repo-info-description", children: description }) : null,
        known(stars) || known(forks) ? /* @__PURE__ */ jsxs("span", { className: "fui-repo-info-stats", children: [
          known(stars) ? /* @__PURE__ */ jsxs("span", { "aria-label": `${stars} stars`, children: [
            /* @__PURE__ */ jsx(Star, { "aria-hidden": true, size: 12 }),
            " ",
            compact.format(stars)
          ] }) : null,
          known(forks) ? /* @__PURE__ */ jsxs("span", { "aria-label": `${forks} forks`, children: [
            /* @__PURE__ */ jsx(GitFork, { "aria-hidden": true, size: 12 }),
            " ",
            compact.format(forks)
          ] }) : null
        ] }) : null
      ]
    }
  );
}
export {
  File,
  Files,
  Folder,
  RepoInfo
};
