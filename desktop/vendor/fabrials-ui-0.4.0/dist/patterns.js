"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { useId } from "react";
import { CheckCircle2, WifiOff, Clock3, AlertCircle, Inbox, LoaderCircle } from "lucide-react";
import { classes } from "./shared.js";
function PageHeader({
  title,
  description,
  actions,
  eyebrow,
  className
}) {
  return /* @__PURE__ */ jsxs("header", { className: classes("fui-page-header", className), children: [
    /* @__PURE__ */ jsxs("div", { className: "fui-page-heading", children: [
      eyebrow && /* @__PURE__ */ jsx("div", { className: "fui-eyebrow", children: eyebrow }),
      /* @__PURE__ */ jsx("h1", { children: title }),
      description && /* @__PURE__ */ jsx("p", { className: "fui-description", children: description })
    ] }),
    actions && /* @__PURE__ */ jsx("div", { className: "fui-actions", children: actions })
  ] });
}
function SectionHeader({
  title,
  description,
  actions,
  className
}) {
  return /* @__PURE__ */ jsxs("header", { className: classes("fui-section-header", className), children: [
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
  icon,
  align = "start"
}) {
  const Icon = stateIcons[state];
  const Heading = `h${headingLevel}`;
  return /* @__PURE__ */ jsxs(
    "div",
    {
      className: classes("fui-state-panel", className),
      "data-state": state,
      "data-align": align,
      role: state === "error" ? "alert" : "status",
      "aria-busy": state === "loading" || void 0,
      children: [
        icon ? /* @__PURE__ */ jsx("span", { "aria-hidden": true, className: "fui-state-icon", children: icon }) : /* @__PURE__ */ jsx(
          Icon,
          {
            "aria-hidden": true,
            size: 20,
            className: classes("fui-state-icon", state === "loading" && "fui-spin")
          }
        ),
        /* @__PURE__ */ jsxs("div", { className: "fui-state-text", children: [
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
function SiteHeader({
  brand,
  navigation,
  actions,
  mobileMenu,
  sticky = true,
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "header",
    {
      "data-slot": "site-header",
      "data-sticky": sticky || void 0,
      className: classes("fui-site-header", className),
      ...props,
      children: /* @__PURE__ */ jsxs("div", { className: "fui-site-header-inner", children: [
        /* @__PURE__ */ jsx("div", { className: "fui-site-brand", children: brand }),
        navigation ? /* @__PURE__ */ jsx("div", { className: "fui-site-nav", children: navigation }) : null,
        /* @__PURE__ */ jsxs("div", { className: "fui-site-actions", children: [
          actions,
          mobileMenu ? /* @__PURE__ */ jsx("div", { className: "fui-site-mobile-menu", children: mobileMenu }) : null
        ] })
      ] })
    }
  );
}
function AuthLayout({
  brand,
  title,
  description,
  children,
  footer,
  aside,
  headingLevel = 1,
  className
}) {
  const Heading = `h${headingLevel}`;
  return /* @__PURE__ */ jsxs(
    "div",
    {
      "data-slot": "auth-layout",
      "data-aside": aside ? "" : void 0,
      className: classes("fui-auth", className),
      children: [
        aside ? /* @__PURE__ */ jsx("aside", { className: "fui-auth-aside", children: aside }) : null,
        /* @__PURE__ */ jsxs("div", { className: "fui-auth-main", children: [
          brand ? /* @__PURE__ */ jsx("div", { className: "fui-auth-brand", children: brand }) : null,
          /* @__PURE__ */ jsxs("section", { className: "fui-auth-card", children: [
            /* @__PURE__ */ jsxs("header", { className: "fui-auth-header", children: [
              /* @__PURE__ */ jsx(Heading, { className: "fui-auth-title", children: title }),
              description ? /* @__PURE__ */ jsx("p", { className: "fui-description", children: description }) : null
            ] }),
            children
          ] }),
          footer ? /* @__PURE__ */ jsx("p", { className: "fui-auth-footer", children: footer }) : null
        ] })
      ]
    }
  );
}
export {
  AuthLayout,
  BulkActions,
  CollectionToolbar,
  Field,
  PageHeader,
  SectionHeader,
  SiteHeader,
  StatePanel,
  WorkspaceShell
};
