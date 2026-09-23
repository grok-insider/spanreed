"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { useId } from "react";
import { CheckCircle2, WifiOff, Clock3, AlertCircle, Inbox, LoaderCircle } from "lucide-react";
import { classes } from "./shared.js";
function PageHeader({
  title,
  description,
  actions,
  eyebrow
}) {
  return /* @__PURE__ */ jsxs("header", { className: "fui-page-header", children: [
    /* @__PURE__ */ jsxs("div", { className: "fui-page-heading", children: [
      eyebrow && /* @__PURE__ */ jsx("p", { className: "fui-eyebrow", children: eyebrow }),
      /* @__PURE__ */ jsx("h1", { children: title }),
      description && /* @__PURE__ */ jsx("p", { className: "fui-description", children: description })
    ] }),
    actions && /* @__PURE__ */ jsx("div", { className: "fui-actions", children: actions })
  ] });
}
function SectionHeader({
  title,
  description,
  actions
}) {
  return /* @__PURE__ */ jsxs("header", { className: "fui-section-header", children: [
    /* @__PURE__ */ jsxs("div", { children: [
      /* @__PURE__ */ jsx("h2", { children: title }),
      description && /* @__PURE__ */ jsx("p", { className: "fui-description", children: description })
    ] }),
    actions && /* @__PURE__ */ jsx("div", { className: "fui-actions", children: actions })
  ] });
}
function CollectionToolbar({
  search,
  filters,
  actions,
  label = "Collection controls"
}) {
  return /* @__PURE__ */ jsxs("div", { className: "fui-collection-toolbar", role: "group", "aria-label": label, children: [
    search && /* @__PURE__ */ jsx("div", { className: "fui-search", children: search }),
    filters && /* @__PURE__ */ jsx("div", { className: "fui-filters", children: filters }),
    actions && /* @__PURE__ */ jsx("div", { className: "fui-actions", children: actions })
  ] });
}
function BulkActions({
  count,
  children,
  label,
  regionLabel = "Selection actions"
}) {
  if (count < 1) return null;
  return /* @__PURE__ */ jsxs("div", { className: "fui-bulk-actions", role: "group", "aria-label": regionLabel, children: [
    /* @__PURE__ */ jsx("span", { role: "status", children: label ?? `${count} selected` }),
    /* @__PURE__ */ jsx("div", { className: "fui-actions", children })
  ] });
}
const stateIcons = {
  loading: LoaderCircle,
  empty: Inbox,
  error: AlertCircle,
  stale: Clock3,
  offline: WifiOff,
  success: CheckCircle2
};
function StatePanel({
  state,
  title,
  description,
  actions,
  headingLevel = 2,
  className,
  icon
}) {
  const Icon = stateIcons[state];
  const Heading = `h${headingLevel}`;
  return /* @__PURE__ */ jsxs(
    "div",
    {
      className: classes("fui-state-panel", className),
      "data-state": state,
      role: state === "error" ? "alert" : "status",
      "aria-busy": state === "loading" || void 0,
      children: [
        icon ? /* @__PURE__ */ jsx("span", { "aria-hidden": true, className: "fui-state-icon", children: icon }) : /* @__PURE__ */ jsx(
          Icon,
          {
            "aria-hidden": true,
            size: 22,
            className: state === "loading" ? "fui-spin" : void 0
          }
        ),
        /* @__PURE__ */ jsxs("div", { children: [
          /* @__PURE__ */ jsx(Heading, { children: title }),
          description && /* @__PURE__ */ jsx("p", { className: "fui-description", children: description })
        ] }),
        actions && /* @__PURE__ */ jsx("div", { className: "fui-actions", children: actions })
      ]
    }
  );
}
function Field({
  label,
  description,
  error,
  children,
  id: providedId
}) {
  const generatedId = useId();
  const id = providedId ?? generatedId;
  const descriptionId = `${id}-description`;
  const errorId = `${id}-error`;
  return /* @__PURE__ */ jsxs("div", { className: "fui-field", children: [
    /* @__PURE__ */ jsx("label", { className: "fui-label", htmlFor: id, children: label }),
    children({
      id,
      "aria-describedby": [description && descriptionId, error && errorId].filter(Boolean).join(" ") || void 0,
      "aria-invalid": error ? true : void 0
    }),
    description && /* @__PURE__ */ jsx("p", { className: "fui-description", id: descriptionId, children: description }),
    error && /* @__PURE__ */ jsx("p", { className: "fui-field-error", id: errorId, role: "alert", children: error })
  ] });
}
function WorkspaceShell({
  navigation,
  header,
  children,
  contentId = "main-content",
  skipLabel = "Skip to content",
  className,
  ...props
}) {
  return /* @__PURE__ */ jsxs("div", { className: classes("fui-workspace", className), ...props, children: [
    /* @__PURE__ */ jsx("a", { className: "fui-skip-link", href: `#${contentId}`, children: skipLabel }),
    navigation,
    /* @__PURE__ */ jsxs("div", { className: "fui-workspace-body", children: [
      header && /* @__PURE__ */ jsx("header", { className: "fui-workspace-header", children: header }),
      /* @__PURE__ */ jsx("main", { className: "fui-workspace-content", id: contentId, tabIndex: -1, children })
    ] })
  ] });
}
export {
  BulkActions,
  CollectionToolbar,
  Field,
  PageHeader,
  SectionHeader,
  StatePanel,
  WorkspaceShell
};
