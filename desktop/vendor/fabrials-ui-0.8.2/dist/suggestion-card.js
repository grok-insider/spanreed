"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { Children } from "react";
import { classes } from "./shared.js";
function SuggestionCard({
  title,
  description,
  icon,
  className,
  type = "button",
  ...props
}) {
  return /* @__PURE__ */ jsxs("button", { type, className: classes("fui-suggestion-card", className), ...props, children: [
    icon ? /* @__PURE__ */ jsx("span", { "aria-hidden": true, className: "fui-suggestion-card-icon", children: icon }) : null,
    /* @__PURE__ */ jsxs("span", { className: "fui-suggestion-card-text", children: [
      /* @__PURE__ */ jsx("span", { className: "fui-suggestion-card-title", children: title }),
      description ? /* @__PURE__ */ jsx("span", { className: "fui-suggestion-card-description", children: description }) : null
    ] })
  ] });
}
function SuggestionGrid({
  columns = 2,
  className,
  style,
  children,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "ul",
    {
      className: classes("fui-suggestion-grid", className),
      style: { "--fui-suggestion-columns": columns, ...style },
      ...props,
      children: Children.map(
        children,
        (child) => child == null || child === false ? null : /* @__PURE__ */ jsx("li", { className: "fui-suggestion-grid-item", children: child })
      )
    }
  );
}
export {
  SuggestionCard,
  SuggestionGrid
};
