"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { Meter as Meter$1 } from "@base-ui/react/meter";
import { ArrowUpRight, ArrowDownRight, Minus } from "lucide-react";
import { classes } from "./shared.js";
function Stat({
  label,
  value,
  unit,
  delta,
  trend,
  deltaTone,
  hint,
  sparkline,
  loading = false,
  className,
  ...props
}) {
  const TrendIcon = trend === "up" ? ArrowUpRight : trend === "down" ? ArrowDownRight : Minus;
  const tone = deltaTone ?? (trend === "up" ? "positive" : trend === "down" ? "negative" : "neutral");
  return /* @__PURE__ */ jsxs(
    "div",
    {
      "data-slot": "stat",
      "aria-busy": loading || void 0,
      className: classes("fui-stat", className),
      ...props,
      children: [
        /* @__PURE__ */ jsx("p", { className: "fui-stat-label", children: label }),
        loading ? /* @__PURE__ */ jsx("span", { "aria-hidden": true, className: "fui-skeleton fui-stat-skeleton" }) : /* @__PURE__ */ jsxs("p", { className: "fui-stat-value", children: [
          value,
          unit ? /* @__PURE__ */ jsx("span", { className: "fui-stat-unit", children: unit }) : null
        ] }),
        delta || hint ? /* @__PURE__ */ jsxs("p", { className: "fui-stat-meta", children: [
          delta ? /* @__PURE__ */ jsxs("span", { className: "fui-stat-delta", "data-tone": tone, children: [
            trend ? /* @__PURE__ */ jsx(TrendIcon, { "aria-hidden": true, size: 14 }) : null,
            delta
          ] }) : null,
          hint ? /* @__PURE__ */ jsx("span", { className: "fui-stat-hint", children: hint }) : null
        ] }) : null,
        sparkline && sparkline.length > 1 && !loading ? /* @__PURE__ */ jsx(Sparkline, { data: sparkline, className: "fui-stat-sparkline" }) : null
      ]
    }
  );
}
function StatGroup({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      "data-slot": "stat-group",
      className: classes("fui-stat-group", className),
      ...props
    }
  );
}
function Sparkline({
  data,
  label,
  color = "var(--chart-1)",
  className,
  ...props
}) {
  const width = 120;
  const height = 32;
  const finite = data.map((value) => Number.isFinite(value) ? value : 0);
  const min = Math.min(...finite);
  const max = Math.max(...finite);
  const span = max - min || 1;
  const step = finite.length > 1 ? width / (finite.length - 1) : width;
  const points = finite.map(
    (value, index) => [index * step, height - 2 - (value - min) / span * (height - 4)]
  );
  const line = points.map(([x, y], index) => `${index ? "L" : "M"}${x.toFixed(2)} ${y.toFixed(2)}`).join(" ");
  return /* @__PURE__ */ jsxs(
    "svg",
    {
      viewBox: `0 0 ${width} ${height}`,
      preserveAspectRatio: "none",
      className: classes("fui-sparkline", className),
      role: label ? "img" : void 0,
      "aria-hidden": label ? void 0 : true,
      "aria-label": label,
      focusable: "false",
      style: { color },
      ...props,
      children: [
        /* @__PURE__ */ jsx(
          "path",
          {
            className: "fui-sparkline-area",
            d: `${line} L${width} ${height} L0 ${height} Z`
          }
        ),
        /* @__PURE__ */ jsx("path", { className: "fui-sparkline-line", d: line, vectorEffect: "non-scaling-stroke" })
      ]
    }
  );
}
function Meter({
  className,
  label,
  hint,
  tone,
  warnAt = 75,
  dangerAt = 90,
  showValue = true,
  value,
  min = 0,
  max = 100,
  ...props
}) {
  const percent = (value - min) / (max - min || 1) * 100;
  const resolved = tone ?? (percent >= dangerAt ? "danger" : percent >= warnAt ? "warning" : "neutral");
  return /* @__PURE__ */ jsxs(
    Meter$1.Root,
    {
      value,
      min,
      max,
      "data-tone": resolved,
      className: classes("fui-meter", className),
      ...props,
      children: [
        label || showValue ? /* @__PURE__ */ jsxs("div", { className: "fui-meter-header", children: [
          label ? /* @__PURE__ */ jsx(Meter$1.Label, { className: "fui-meter-label", children: label }) : /* @__PURE__ */ jsx("span", {}),
          showValue ? /* @__PURE__ */ jsx(Meter$1.Value, { className: "fui-meter-value" }) : null
        ] }) : null,
        /* @__PURE__ */ jsx(Meter$1.Track, { className: "fui-meter-track", children: /* @__PURE__ */ jsx(Meter$1.Indicator, { className: "fui-meter-indicator" }) }),
        hint ? /* @__PURE__ */ jsx("p", { className: "fui-meter-hint", children: hint }) : null
      ]
    }
  );
}
function StatusDot({
  tone = "neutral",
  label,
  hideLabel = false,
  pulse = false,
  className,
  ...props
}) {
  return /* @__PURE__ */ jsxs(
    "span",
    {
      "data-slot": "status",
      "data-tone": tone,
      "data-pulse": pulse || void 0,
      className: classes("fui-status", className),
      ...props,
      children: [
        /* @__PURE__ */ jsx("span", { "aria-hidden": true, className: "fui-status-dot" }),
        /* @__PURE__ */ jsx("span", { className: hideLabel ? "fui-sr-only" : "fui-status-label", children: label })
      ]
    }
  );
}
function DescriptionList({
  className,
  layout = "grid",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "dl",
    {
      "data-layout": layout,
      className: classes("fui-description-list", className),
      ...props
    }
  );
}
function DescriptionItem({ className, ...props }) {
  return /* @__PURE__ */ jsx("div", { className: classes("fui-description-item", className), ...props });
}
function DescriptionTerm({ className, ...props }) {
  return /* @__PURE__ */ jsx("dt", { className: classes("fui-description-term", className), ...props });
}
function DescriptionDetails({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx("dd", { className: classes("fui-description-details", className), ...props });
}
export {
  DescriptionDetails,
  DescriptionItem,
  DescriptionList,
  DescriptionTerm,
  Meter,
  Sparkline,
  Stat,
  StatGroup,
  StatusDot
};
