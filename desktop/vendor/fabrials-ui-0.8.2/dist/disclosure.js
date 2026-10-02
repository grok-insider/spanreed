"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { ChevronDown } from "lucide-react";
import { classes } from "./shared.js";
function Disclosure({
  className,
  revealInvalid = true,
  onInvalidCapture,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "details",
    {
      "data-slot": "disclosure",
      className: classes("fui-disclosure", className),
      onInvalidCapture: (event) => {
        onInvalidCapture?.(event);
        if (revealInvalid && !event.currentTarget.open) event.currentTarget.open = true;
      },
      ...props
    }
  );
}
function DisclosureSummary({
  className,
  children,
  count,
  chevron = "end",
  size = "md",
  ...props
}) {
  const icon = /* @__PURE__ */ jsx(ChevronDown, { "aria-hidden": true, className: "fui-disclosure-chevron" });
  return /* @__PURE__ */ jsxs(
    "summary",
    {
      "data-slot": "disclosure-summary",
      "data-size": size,
      "data-chevron": chevron,
      className: classes("fui-disclosure-summary", className),
      ...props,
      children: [
        chevron === "start" ? icon : null,
        /* @__PURE__ */ jsx("span", { className: "fui-disclosure-label", children }),
        count != null && count !== false ? /* @__PURE__ */ jsx("span", { className: "fui-disclosure-count", children: count }) : null,
        chevron === "end" ? icon : null
      ]
    }
  );
}
function DisclosurePanel({ className, ...props }) {
  return /* @__PURE__ */ jsx("div", { "data-slot": "disclosure-panel", className: classes("fui-disclosure-panel", className), ...props });
}
export {
  Disclosure,
  DisclosurePanel,
  DisclosureSummary
};
