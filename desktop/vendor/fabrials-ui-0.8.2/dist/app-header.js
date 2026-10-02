"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { mergeProps } from "@base-ui/react/merge-props";
import { useRender } from "@base-ui/react/use-render";
import { ChevronDown } from "lucide-react";
import { Button } from "./controls.js";
import { classes } from "./shared.js";
const attrs = (value) => value;
function AppHeader({ brand, navigation, command, actions, sticky = false, className, ...props }) {
  return /* @__PURE__ */ jsx("header", { "data-slot": "app-header", "data-sticky": sticky ? "" : void 0, className: classes("fui-app-header", className), ...props, children: /* @__PURE__ */ jsxs("div", { className: "fui-app-header-inner", children: [
    brand,
    navigation,
    /* @__PURE__ */ jsxs("div", { className: "fui-app-header-actions", children: [
      command ? /* @__PURE__ */ jsx("div", { className: "fui-app-header-command", children: command }) : null,
      actions
    ] })
  ] }) });
}
function AppHeaderBrand({ render, className, ...props }) {
  return useRender({
    defaultTagName: "a",
    render,
    props: mergeProps(attrs({ className: classes("fui-app-header-brand", className), "data-slot": "app-header-brand" }), props)
  });
}
function AppHeaderLogo({ className, alt = "", ...props }) {
  return /* @__PURE__ */ jsx("img", { alt, className: classes("fui-app-header-logo", className), ...props });
}
function AppHeaderNav({ label, className, ...props }) {
  return /* @__PURE__ */ jsx("nav", { "data-slot": "app-header-nav", "aria-label": label, className: classes("fui-app-header-nav", className), ...props });
}
function AppHeaderLink({ current = false, render, className, ...props }) {
  return useRender({
    defaultTagName: "a",
    render,
    props: mergeProps(
      attrs({ className: classes("fui-app-header-link", className), "data-slot": "app-header-link", "aria-current": current ? "page" : void 0 }),
      props
    )
  });
}
function AppHeaderAction({ className, variant = "ghost", size = "lg", ...props }) {
  return /* @__PURE__ */ jsx(Button, { "data-slot": "app-header-action", variant, size, className: classes("fui-app-header-action", className), ...props });
}
function AppHeaderLabel({ className, ...props }) {
  return /* @__PURE__ */ jsx("span", { "data-slot": "app-header-label", className: classes("fui-app-header-label", className), ...props });
}
function AppHeaderCaret({ className, ...props }) {
  return /* @__PURE__ */ jsx(ChevronDown, { "aria-hidden": "true", className: classes("fui-app-header-caret", className), ...props });
}
function AppHeaderPlaceholder({ className, ...props }) {
  return /* @__PURE__ */ jsx("span", { "aria-hidden": "true", "data-slot": "app-header-placeholder", className: classes("fui-app-header-placeholder", className), ...props });
}
export {
  AppHeader,
  AppHeaderAction,
  AppHeaderBrand,
  AppHeaderCaret,
  AppHeaderLabel,
  AppHeaderLink,
  AppHeaderLogo,
  AppHeaderNav,
  AppHeaderPlaceholder
};
