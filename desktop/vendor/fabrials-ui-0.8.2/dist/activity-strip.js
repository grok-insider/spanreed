"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { classes } from "./shared.js";
function activityLevel(value, max) {
  if (!(value > 0) || !(max > 0)) return 0;
  return Math.min(4, Math.max(1, Math.ceil(value / max * 4)));
}
function ActivityStrip({
  cells,
  caption,
  max,
  size = "md",
  startLabel,
  endLabel,
  formatValue = (value) => String(value),
  className,
  ...props
}) {
  const top = max ?? Math.max(0, ...cells.map((cell) => cell.value));
  return /* @__PURE__ */ jsxs("figure", { "data-slot": "activity-strip", "data-size": size, className: classes("fui-activity-strip", className), ...props, children: [
    /* @__PURE__ */ jsx("div", { className: "fui-activity-cells", "aria-hidden": true, children: cells.map((cell, index) => /* @__PURE__ */ jsx(
      "span",
      {
        className: "fui-activity-cell",
        "data-level": activityLevel(cell.value, top),
        "data-tone": cell.tone,
        title: `${cell.label}: ${formatValue(cell.value)}`
      },
      `${cell.label}-${index}`
    )) }),
    startLabel || endLabel ? /* @__PURE__ */ jsxs("figcaption", { className: "fui-activity-labels", "aria-hidden": true, children: [
      /* @__PURE__ */ jsx("span", { children: startLabel }),
      /* @__PURE__ */ jsx("span", { children: endLabel })
    ] }) : null,
    /* @__PURE__ */ jsxs("table", { className: "fui-sr-only", children: [
      /* @__PURE__ */ jsx("caption", { children: caption }),
      /* @__PURE__ */ jsx("tbody", { children: cells.map((cell, index) => /* @__PURE__ */ jsxs("tr", { children: [
        /* @__PURE__ */ jsx("th", { scope: "row", children: cell.label }),
        /* @__PURE__ */ jsx("td", { children: formatValue(cell.value) })
      ] }, `${cell.label}-${index}`)) })
    ] })
  ] });
}
export {
  ActivityStrip,
  activityLevel
};
