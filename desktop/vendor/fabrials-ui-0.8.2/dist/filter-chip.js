"use client";
import { jsx, jsxs, Fragment } from "react/jsx-runtime";
import { mergeProps } from "@base-ui/react/merge-props";
import { useRender } from "@base-ui/react/use-render";
import { X } from "lucide-react";
import { classes } from "./shared.js";
function defaultClearLabel(label, value) {
  return `Clear ${label.toLowerCase()} filter (${value})`;
}
function FilterChipContent({ label, value }) {
  return /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsxs("span", { className: "fui-filter-chip-label", children: [
      label,
      ":"
    ] }),
    /* @__PURE__ */ jsx("span", { className: "fui-filter-chip-value", children: value }),
    /* @__PURE__ */ jsx(X, { "aria-hidden": true, className: "fui-filter-chip-icon" })
  ] });
}
function FilterChipLink({ label, value, clearLabel, className, render, ...props }) {
  return useRender({
    defaultTagName: "a",
    render,
    props: mergeProps(
      {
        className: classes("fui-filter-chip", className),
        "aria-label": clearLabel ?? defaultClearLabel(label, value),
        children: /* @__PURE__ */ jsx(FilterChipContent, { label, value })
      },
      props
    )
  });
}
function FilterChipButton({ label, value, clearLabel, className, onRemove, ...props }) {
  return /* @__PURE__ */ jsx(
    "button",
    {
      type: "button",
      className: classes("fui-filter-chip", className),
      "aria-label": clearLabel ?? defaultClearLabel(label, value),
      ...props,
      onClick: onRemove,
      children: /* @__PURE__ */ jsx(FilterChipContent, { label, value })
    }
  );
}
function FilterChip(props) {
  if (props.onRemove) return /* @__PURE__ */ jsx(FilterChipButton, { ...props });
  return /* @__PURE__ */ jsx(FilterChipLink, { ...props });
}
export {
  FilterChip
};
