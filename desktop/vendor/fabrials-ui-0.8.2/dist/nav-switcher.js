"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { useMemo, useRef, useContext, createContext } from "react";
import { Check, ChevronsUpDown } from "lucide-react";
import { Popover, PopoverContent, PopoverTrigger } from "./popover.js";
import { SidebarMenuButton } from "./sidebar.js";
import { classes } from "./shared.js";
const LayoutContext = createContext({ layout: "block", contain: true });
function NavSwitcher({ layout = "block", contain = true, ...props }) {
  const setup = useMemo(() => ({ layout, contain }), [layout, contain]);
  return /* @__PURE__ */ jsx(LayoutContext.Provider, { value: setup, children: /* @__PURE__ */ jsx(Popover, { ...props }) });
}
function NavSwitcherTrigger({ mark, title, description, tag, label, className, ...props }) {
  const { layout, contain } = useContext(LayoutContext);
  return /* @__PURE__ */ jsxs(PopoverTrigger, { "data-layout": layout, "data-contain": contain ? void 0 : "false", className: classes("fui-nav-switcher-trigger", className), ...props, children: [
    mark ? /* @__PURE__ */ jsx("span", { className: "fui-nav-switcher-mark", "aria-hidden": "true", children: mark }) : null,
    /* @__PURE__ */ jsxs("span", { className: "fui-nav-switcher-text", children: [
      label ? /* @__PURE__ */ jsxs("span", { className: "fui-sr-only", children: [
        label,
        ": "
      ] }) : null,
      /* @__PURE__ */ jsx("span", { className: "fui-nav-switcher-title", dir: "auto", children: title }),
      description ? /* @__PURE__ */ jsx("span", { className: "fui-nav-switcher-detail", dir: "auto", children: description }) : null
    ] }),
    tag ? /* @__PURE__ */ jsx("span", { className: "fui-nav-switcher-tag", children: tag }) : null,
    /* @__PURE__ */ jsx(ChevronsUpDown, { className: "fui-nav-switcher-chevron", "aria-hidden": "true" })
  ] });
}
function NavSwitcherContent({ label, listLabel, children, footer, className, ref, initialFocus, ...props }) {
  const popup = useRef(null);
  const setRef = (node) => {
    popup.current = node;
    if (typeof ref === "function") ref(node);
    else if (ref) ref.current = node;
  };
  return /* @__PURE__ */ jsxs(
    PopoverContent,
    {
      align: "start",
      "aria-label": label,
      className: classes("fui-nav-switcher-popover", className),
      ref: setRef,
      initialFocus: initialFocus ?? (() => popup.current?.querySelector("[aria-current]") ?? true),
      ...props,
      children: [
        /* @__PURE__ */ jsx("nav", { "aria-label": listLabel ?? label, className: "fui-nav-switcher-list", children: /* @__PURE__ */ jsx("ul", { children }) }),
        footer
      ]
    }
  );
}
function NavSwitcherItem({ current = false, mark, description, tag, children, ...props }) {
  return /* @__PURE__ */ jsx("li", { "data-slot": "nav-switcher-item", className: "fui-nav-switcher-entry", children: /* @__PURE__ */ jsxs(SidebarMenuButton, { isActive: current, size: "touch", ...props, children: [
    mark ? /* @__PURE__ */ jsx("span", { className: "fui-nav-switcher-mark", "aria-hidden": "true", children: mark }) : null,
    /* @__PURE__ */ jsxs("span", { className: "fui-nav-switcher-text", children: [
      /* @__PURE__ */ jsx("span", { className: "fui-nav-switcher-name", dir: "auto", children }),
      description ? /* @__PURE__ */ jsx("span", { className: "fui-nav-switcher-status", children: description }) : null
    ] }),
    tag ?? (current ? /* @__PURE__ */ jsx(Check, { className: "fui-nav-switcher-check", "aria-hidden": "true" }) : null)
  ] }) });
}
function NavSwitcherSeparator({ className, ...props }) {
  return /* @__PURE__ */ jsx("li", { "aria-hidden": "true", "data-slot": "nav-switcher-separator", className: classes("fui-nav-switcher-rule", className), ...props });
}
export {
  NavSwitcher,
  NavSwitcherContent,
  NavSwitcherItem,
  NavSwitcherSeparator,
  NavSwitcherTrigger
};
