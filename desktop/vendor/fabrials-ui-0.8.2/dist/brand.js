"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { classes } from "./shared.js";
function FabrialsGem({
  size = 20,
  gem,
  className,
  title,
  ...props
}) {
  return /* @__PURE__ */ jsxs(
    "svg",
    {
      viewBox: "0 0 24 24",
      width: size,
      height: size,
      "data-gem": gem,
      className: classes("fui-gem", className),
      role: title ? "img" : void 0,
      "aria-hidden": title ? void 0 : true,
      "aria-label": title,
      focusable: "false",
      ...props,
      children: [
        /* @__PURE__ */ jsx("path", { className: "fui-gem-crown-left", d: "M3.5 9 7.6 3.5h2.9L8.4 9Z" }),
        /* @__PURE__ */ jsx("path", { className: "fui-gem-table", d: "M10.5 3.5h3l2.1 5.5H8.4Z" }),
        /* @__PURE__ */ jsx("path", { className: "fui-gem-crown-right", d: "M13.5 3.5h2.9L20.5 9h-4.9Z" }),
        /* @__PURE__ */ jsx("path", { className: "fui-gem-pavilion-left", d: "M3.5 9h4.9L12 21Z" }),
        /* @__PURE__ */ jsx("path", { className: "fui-gem-pavilion", d: "M8.4 9h7.2L12 21Z" }),
        /* @__PURE__ */ jsx("path", { className: "fui-gem-pavilion-right", d: "M15.6 9h4.9L12 21Z" }),
        /* @__PURE__ */ jsx(
          "path",
          {
            className: "fui-gem-edge",
            d: "M7.6 3.5h8.8L20.5 9 12 21 3.5 9Z",
            fill: "none"
          }
        )
      ]
    }
  );
}
function ProductLockup({
  product,
  tagline,
  gem,
  mark,
  size = "md",
  className,
  ...props
}) {
  const gemSize = size === "lg" ? 28 : size === "sm" ? 18 : 22;
  return /* @__PURE__ */ jsxs(
    "span",
    {
      "data-slot": "product-lockup",
      "data-size": size,
      "data-gem": gem,
      className: classes("fui-lockup", className),
      ...props,
      children: [
        /* @__PURE__ */ jsx("span", { className: "fui-lockup-mark", children: mark ?? /* @__PURE__ */ jsx(FabrialsGem, { size: gemSize }) }),
        /* @__PURE__ */ jsxs("span", { className: "fui-lockup-text", children: [
          /* @__PURE__ */ jsx("span", { className: "fui-lockup-product", children: product }),
          tagline ? /* @__PURE__ */ jsx("span", { className: "fui-lockup-tagline", children: tagline }) : null
        ] })
      ]
    }
  );
}
export {
  FabrialsGem,
  ProductLockup
};
