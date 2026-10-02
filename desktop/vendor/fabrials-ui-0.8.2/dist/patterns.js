"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { useId, useContext, createContext } from "react";
import { CheckCircle2, WifiOff, Clock3, AlertCircle, Inbox, LoaderCircle } from "lucide-react";
import { classes } from "./shared.js";
function PageHeader({
  title,
  description,
  actions,
  eyebrow,
  headingLevel = 1,
  headingRef,
  headingProps,
  className
}) {
  const Heading = `h${headingLevel}`;
  return /* @__PURE__ */ jsxs("header", { className: classes("fui-page-header", className), children: [
    /* @__PURE__ */ jsxs("div", { className: "fui-page-heading", children: [
      eyebrow && /* @__PURE__ */ jsx("div", { className: "fui-eyebrow", children: eyebrow }),
      /* @__PURE__ */ jsx(Heading, { ...headingProps, ref: headingRef, className: "fui-page-title", children: title }),
      description && /* @__PURE__ */ jsx("p", { className: "fui-description", children: description })
    ] }),
    actions && /* @__PURE__ */ jsx("div", { className: "fui-actions", children: actions })
  ] });
}
function SectionHeader({
  title,
  description,
  actions,
  aside,
  headingLevel = 2,
  headingRef,
  headingProps,
  className
}) {
  const Heading = `h${headingLevel}`;
  return /* @__PURE__ */ jsxs("header", { className: classes("fui-section-header", className), children: [
    /* @__PURE__ */ jsxs("div", { children: [
      aside || aside === 0 ? /* @__PURE__ */ jsxs("div", { className: "fui-section-heading-row", children: [
        /* @__PURE__ */ jsx(Heading, { ...headingProps, ref: headingRef, className: "fui-section-title", children: title }),
        /* @__PURE__ */ jsx("span", { className: "fui-section-aside", children: aside })
      ] }) : /* @__PURE__ */ jsx(Heading, { ...headingProps, ref: headingRef, className: "fui-section-title", children: title }),
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
  regionLabel = "Selection actions",
  keepMounted = false
}) {
  const empty = count < 1;
  if (empty && !keepMounted) return null;
  return /* @__PURE__ */ jsxs(
    "div",
    {
      className: "fui-bulk-actions",
      role: "group",
      "aria-label": regionLabel,
      "data-empty": empty || void 0,
      children: [
        /* @__PURE__ */ jsx("span", { role: "status", children: label ?? `${count} selected` }),
        /* @__PURE__ */ jsx("div", { className: "fui-actions", hidden: empty, children })
      ]
    }
  );
}
const BulkContext = createContext(null);
function useBulk(part) {
  const state = useContext(BulkContext);
  if (!state) throw new Error(`${part} must be inside BulkActionsRoot.`);
  return state;
}
function BulkActionsRoot({
  count,
  regionLabel = "Selection actions",
  keepMounted = false,
  className,
  children,
  ...props
}) {
  const empty = count < 1;
  return /* @__PURE__ */ jsx(BulkContext.Provider, { value: { count, empty, keepMounted, regionLabel }, children: /* @__PURE__ */ jsx("div", { ...props, className: classes("fui-bulk-actions-root", className), "data-empty": empty || void 0, children }) });
}
function BulkActionsStatus({ className, children, ...props }) {
  const { count, empty, keepMounted } = useBulk("BulkActionsStatus");
  if (empty && !keepMounted) return null;
  return /* @__PURE__ */ jsx("span", { role: "status", ...props, className: classes("fui-bulk-actions-status", className), "data-empty": empty || void 0, children: children ?? `${count} selected` });
}
function BulkActionsContent({ className, children, hidden = false, ...props }) {
  const { empty, keepMounted, regionLabel } = useBulk("BulkActionsContent");
  if (empty && !keepMounted) return null;
  return /* @__PURE__ */ jsx("div", { role: "group", "aria-label": regionLabel, ...props, className: classes("fui-bulk-actions-content", className), hidden: empty || hidden, children });
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
  align = "start",
  size = "md",
  variant = "panel",
  fill = false,
  ...props
}) {
  const Icon = stateIcons[state];
  const Heading = `h${headingLevel}`;
  return /* @__PURE__ */ jsxs(
    "div",
    {
      role: state === "error" ? "alert" : "status",
      "aria-busy": state === "loading" || void 0,
      ...props,
      className: classes("fui-state-panel", className),
      "data-state": state,
      "data-align": align,
      "data-size": size === "sm" ? "sm" : void 0,
      "data-variant": variant === "inline" ? "inline" : void 0,
      "data-fill": fill || void 0,
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
  actions,
  align = "auto",
  headingLevel = 1,
  className
}) {
  const Heading = `h${headingLevel}`;
  return /* @__PURE__ */ jsxs(
    "div",
    {
      "data-slot": "auth-layout",
      "data-aside": aside ? "" : void 0,
      "data-align": align === "auto" ? void 0 : align,
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
        ] }),
        actions ? /* @__PURE__ */ jsx("div", { className: "fui-auth-actions", children: actions }) : null
      ]
    }
  );
}
export {
  AuthLayout,
  BulkActions,
  BulkActionsContent,
  BulkActionsRoot,
  BulkActionsStatus,
  CollectionToolbar,
  Field,
  PageHeader,
  SectionHeader,
  SiteHeader,
  StatePanel,
  WorkspaceShell
};
