"use client";
import { jsxs, Fragment, jsx } from "react/jsx-runtime";
import { mergeProps } from "@base-ui/react/merge-props";
import { useRender } from "@base-ui/react/use-render";
import { classes } from "./shared.js";
function NavTabs({ label, className, children, ...props }) {
  return /* @__PURE__ */ jsx("nav", { "aria-label": label, "data-slot": "nav-tabs", className: classes("fui-nav-tabs", className), ...props, children: /* @__PURE__ */ jsx("div", { className: "fui-tabs-list", "data-variant": "underline", children }) });
}
function NavTab({ current = false, count, render, className, children, ...props }) {
  return useRender({
    defaultTagName: "a",
    render,
    props: mergeProps(
      {
        className: classes("fui-tab", "fui-nav-tab", className),
        "aria-current": current ? "page" : void 0,
        children: /* @__PURE__ */ jsxs(Fragment, { children: [
          children,
          count !== void 0 && count !== null && count !== "" ? /* @__PURE__ */ jsx("span", { className: "fui-nav-tab-count", children: count }) : null
        ] })
      },
      { ...props, ...current ? { "data-active": "" } : {} }
    )
  });
}
export {
  NavTab,
  NavTabs
};
