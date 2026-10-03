"use client";
import { jsxs, jsx, Fragment } from "react/jsx-runtime";
import { useRef, useState, useEffect, useMemo } from "react";
import { ResponsiveContainer, LineChart, Line, BarChart, Bar, CartesianGrid, XAxis, YAxis, Tooltip } from "recharts";
import { classes } from "./shared.js";
const PALETTE = [
  "var(--chart-1)",
  "var(--chart-2)",
  "var(--chart-3)",
  "var(--chart-4)",
  "var(--chart-5)",
  "var(--chart-6)"
];
function SeriesChart({
  data,
  xKey,
  series,
  kind = "bar",
  stacked = false,
  lineType = "monotone",
  yFormat = (value) => String(value),
  titleKey,
  caption,
  height = 288,
  className
}) {
  const frameRef = useRef(null);
  const [frameWidth, setFrameWidth] = useState(0);
  useEffect(() => {
    const element = frameRef.current;
    if (!element) return;
    const update = () => setFrameWidth(Math.round(element.clientWidth));
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const [hidden, setHidden] = useState(/* @__PURE__ */ new Set());
  const painted = useMemo(
    () => series.map((item, index) => ({
      ...item,
      label: item.label ?? item.key,
      color: item.color ?? PALETTE[index % PALETTE.length]
    })),
    [series]
  );
  const visible = painted.filter((item) => !hidden.has(item.key));
  function toggle(key) {
    setHidden((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      if (next.size === painted.length) return /* @__PURE__ */ new Set();
      return next;
    });
  }
  const axes = /* @__PURE__ */ jsxs(Fragment, { children: [
    /* @__PURE__ */ jsx(CartesianGrid, { vertical: false, stroke: "var(--border)" }),
    /* @__PURE__ */ jsx(
      XAxis,
      {
        dataKey: xKey,
        axisLine: false,
        tickLine: false,
        interval: "preserveStartEnd",
        tick: { fill: "var(--muted-foreground)", fontSize: 12 }
      }
    ),
    /* @__PURE__ */ jsx(
      YAxis,
      {
        axisLine: false,
        tickLine: false,
        width: 72,
        tick: { fill: "var(--muted-foreground)", fontSize: 12 },
        tickFormatter: (value) => yFormat(value)
      }
    ),
    /* @__PURE__ */ jsx(
      Tooltip,
      {
        cursor: { fill: "var(--muted)", opacity: 0.45 },
        content: (props) => /* @__PURE__ */ jsx(
          SeriesTooltip,
          {
            active: props.active,
            label: props.label,
            payload: props.payload,
            titleKey,
            yFormat
          }
        )
      }
    )
  ] });
  return /* @__PURE__ */ jsxs("div", { ref: frameRef, className: classes("fui-chart", className), children: [
    /* @__PURE__ */ jsx("div", { className: "fui-chart-frame", style: { height }, children: frameWidth > 0 ? /* @__PURE__ */ jsx(
      ResponsiveContainer,
      {
        width: frameWidth,
        height,
        initialDimension: { width: frameWidth, height },
        children: kind === "line" ? /* @__PURE__ */ jsxs(LineChart, { data, margin: { top: 8, right: 8, left: 0, bottom: 0 }, children: [
          axes,
          visible.map((item) => /* @__PURE__ */ jsx(
            Line,
            {
              type: lineType === "step" ? "stepAfter" : "monotone",
              dataKey: item.key,
              name: item.label,
              stroke: item.color,
              strokeWidth: 2,
              strokeDasharray: item.dashed ? "5 4" : void 0,
              dot: false,
              connectNulls: true,
              isAnimationActive: false
            },
            item.key
          ))
        ] }) : /* @__PURE__ */ jsxs(BarChart, { data, margin: { top: 8, right: 4, left: 0, bottom: 0 }, barCategoryGap: "22%", children: [
          axes,
          visible.map((item) => /* @__PURE__ */ jsx(
            Bar,
            {
              dataKey: item.key,
              name: item.label,
              fill: item.color,
              stackId: stacked ? "series" : void 0,
              radius: stacked ? void 0 : [4, 4, 0, 0],
              maxBarSize: 48,
              isAnimationActive: false
            },
            item.key
          ))
        ] })
      }
    ) : null }),
    /* @__PURE__ */ jsxs("table", { className: "fui-sr-only", children: [
      /* @__PURE__ */ jsx("caption", { children: caption }),
      /* @__PURE__ */ jsx("thead", { children: /* @__PURE__ */ jsxs("tr", { children: [
        /* @__PURE__ */ jsx("th", { children: "Label" }),
        painted.map((item) => /* @__PURE__ */ jsx("th", { children: item.label }, item.key))
      ] }) }),
      /* @__PURE__ */ jsx("tbody", { children: data.map((row, index) => /* @__PURE__ */ jsxs("tr", { children: [
        /* @__PURE__ */ jsx("th", { children: String(titleKey ? row[titleKey] ?? row[xKey] : row[xKey] ?? "") }),
        painted.map((item) => /* @__PURE__ */ jsx("td", { children: yFormat(numeric(row[item.key])) }, item.key))
      ] }, `${String(row[xKey] ?? index)}`)) })
    ] }),
    painted.length > 1 ? /* @__PURE__ */ jsxs("div", { className: "fui-chart-legend", children: [
      painted.map((item) => {
        const shown = !hidden.has(item.key);
        return /* @__PURE__ */ jsxs("span", { className: "fui-chart-legend-item", children: [
          /* @__PURE__ */ jsxs(
            "button",
            {
              type: "button",
              "aria-pressed": shown,
              "aria-label": shown ? `Hide ${item.label}` : `Show ${item.label}`,
              className: "fui-chart-legend-toggle",
              onClick: () => toggle(item.key),
              children: [
                /* @__PURE__ */ jsx("span", { "aria-hidden": true, className: "fui-chart-swatch", style: { background: item.color } }),
                /* @__PURE__ */ jsx("span", { "data-hidden": shown ? void 0 : "", children: item.label })
              ]
            }
          ),
          /* @__PURE__ */ jsx(
            "button",
            {
              type: "button",
              "aria-label": `Show only ${item.label}`,
              className: "fui-chart-legend-only",
              onClick: () => setHidden(new Set(painted.filter((entry) => entry.key !== item.key).map((entry) => entry.key))),
              children: "Only"
            }
          )
        ] }, item.key);
      }),
      visible.length !== painted.length ? /* @__PURE__ */ jsx(
        "button",
        {
          type: "button",
          className: "fui-chart-legend-reset",
          onClick: () => setHidden(/* @__PURE__ */ new Set()),
          children: "Show all"
        }
      ) : null
    ] }) : null
  ] });
}
function SeriesTooltip({
  active,
  payload,
  label,
  titleKey,
  yFormat
}) {
  if (!active || !payload?.length) return null;
  const row = payload[0]?.payload;
  const title = titleKey && row ? row[titleKey] : label;
  const rows = payload.filter((item) => numeric(item.value) !== 0);
  const total = payload.reduce((sum, item) => sum + numeric(item.value), 0);
  return /* @__PURE__ */ jsxs("div", { className: "fui-chart-tip", children: [
    /* @__PURE__ */ jsx("p", { className: "fui-chart-tip-title", children: String(title ?? "") }),
    rows.length ? /* @__PURE__ */ jsx("ul", { className: "fui-chart-tip-list", children: rows.map((item) => /* @__PURE__ */ jsxs("li", { className: "fui-chart-tip-row", children: [
      /* @__PURE__ */ jsxs("span", { className: "fui-chart-tip-name", children: [
        /* @__PURE__ */ jsx("span", { "aria-hidden": true, className: "fui-chart-swatch", style: { background: item.color } }),
        /* @__PURE__ */ jsx("span", { children: String(item.name ?? "") })
      ] }),
      /* @__PURE__ */ jsx("span", { className: "fui-chart-tip-value", children: yFormat(numeric(item.value)) })
    ] }, String(item.name))) }) : /* @__PURE__ */ jsx("p", { children: "No value" }),
    payload.length > 1 ? /* @__PURE__ */ jsxs("p", { className: "fui-chart-tip-total", children: [
      /* @__PURE__ */ jsx("span", { children: "Total" }),
      /* @__PURE__ */ jsx("span", { children: yFormat(total) })
    ] }) : null
  ] });
}
function numeric(value) {
  const next = typeof value === "number" ? value : Number(value ?? 0);
  return Number.isFinite(next) ? next : 0;
}
export {
  SeriesChart
};
