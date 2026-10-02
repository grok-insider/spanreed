"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { Children, Fragment } from "react";
import { classes } from "./shared.js";
import { ModifierKeyText } from "./use-modifier-key.js";
function Kbd({
  className,
  mod = false,
  children,
  ...props
}) {
  return /* @__PURE__ */ jsx("kbd", { className: classes("fui-kbd", className), ...props, children: mod ? /* @__PURE__ */ jsx(ModifierKeyText, {}) : children });
}
function KbdGroup({
  className,
  sequence = false,
  separator = "then",
  children,
  ...props
}) {
  const keys = sequence ? Children.toArray(children) : null;
  return /* @__PURE__ */ jsx("kbd", { className: classes("fui-kbd-group", className), "data-sequence": sequence || void 0, ...props, children: keys ? keys.map((key, index) => /* @__PURE__ */ jsxs(Fragment, { children: [
    index > 0 ? /* @__PURE__ */ jsx("span", { className: "fui-kbd-separator", children: separator }) : null,
    key
  ] }, index)) : children });
}
export {
  Kbd,
  KbdGroup
};
