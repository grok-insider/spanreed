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
  "var(--chart-5)"
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
  return /* @__PURE__ */ jsxs("div", { ref: frameRef, className: classes("grid min-w-0 gap-3", className), children: [
    /* @__PURE__ */ jsx("div", { className: "w-full min-w-0", style: { height }, children: frameWidth > 0 ? /* @__PURE__ */ jsx(
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
              strokeWidth: item.dashed ? 2 : 1.5,
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
    painted.length > 1 ? /* @__PURE__ */ jsxs("div", { className: "flex flex-wrap gap-x-3 gap-y-1", children: [
      painted.map((item) => {
        const shown = !hidden.has(item.key);
        return /* @__PURE__ */ jsxs("span", { className: "inline-flex items-center", children: [
          /* @__PURE__ */ jsxs(
            "button",
            {
              type: "button",
              "aria-pressed": shown,
              "aria-label": shown ? `Hide ${item.label}` : `Show ${item.label}`,
              className: "inline-flex min-h-11 items-center gap-2 rounded-lg px-1.5 text-xs focus-visible:outline-2 focus-visible:outline-offset-2",
              onClick: () => toggle(item.key),
              children: [
                /* @__PURE__ */ jsx("span", { "aria-hidden": true, className: "size-2.5 rounded-full", style: { background: item.color } }),
                /* @__PURE__ */ jsx("span", { className: shown ? "text-foreground" : "text-muted-foreground line-through", children: item.label })
              ]
            }
          ),
          /* @__PURE__ */ jsx(
            "button",
            {
              type: "button",
              "aria-label": `Show only ${item.label}`,
              className: "min-h-11 rounded-lg px-1.5 text-[11px] text-muted-foreground",
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
          className: "min-h-11 rounded-lg px-2 text-xs font-medium",
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
    /* @__PURE__ */ jsx("p", { className: "font-medium", children: String(title ?? "") }),
    rows.length ? /* @__PURE__ */ jsx("ul", { className: "grid gap-1", children: rows.map((item) => /* @__PURE__ */ jsxs("li", { className: "flex items-center justify-between gap-4", children: [
      /* @__PURE__ */ jsxs("span", { className: "flex min-w-0 items-center gap-2", children: [
        /* @__PURE__ */ jsx("span", { "aria-hidden": true, className: "size-2 shrink-0 rounded-full", style: { background: item.color } }),
        /* @__PURE__ */ jsx("span", { className: "truncate", children: String(item.name ?? "") })
      ] }),
      /* @__PURE__ */ jsx("span", { className: "font-mono", children: yFormat(numeric(item.value)) })
    ] }, String(item.name))) }) : /* @__PURE__ */ jsx("p", { children: "No value" }),
    payload.length > 1 ? /* @__PURE__ */ jsxs("p", { className: "flex justify-between gap-4 border-t pt-2 font-mono", children: [
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
