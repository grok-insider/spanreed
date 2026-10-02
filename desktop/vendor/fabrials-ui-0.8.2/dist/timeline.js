"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { classes } from "./shared.js";
function Timeline({ className, ...props }) {
  return /* @__PURE__ */ jsx("ol", { "data-slot": "timeline", className: classes("fui-timeline", className), ...props });
}
function TimelineItem({ icon, tone = "neutral", title, time, actions, fresh = false, children, className, ...props }) {
  return /* @__PURE__ */ jsxs("li", { "data-slot": "timeline-item", "data-tone": tone, "data-fresh": fresh || void 0, className: classes("fui-timeline-item", className), ...props, children: [
    /* @__PURE__ */ jsx("span", { className: "fui-timeline-marker", "aria-hidden": true, children: icon ?? /* @__PURE__ */ jsx("span", { className: "fui-timeline-dot" }) }),
    /* @__PURE__ */ jsxs("div", { className: "fui-timeline-body", children: [
      /* @__PURE__ */ jsxs("div", { className: "fui-timeline-header", children: [
        /* @__PURE__ */ jsx("span", { className: "fui-timeline-title", children: title }),
        time ? /* @__PURE__ */ jsx("span", { className: "fui-timeline-time", children: time }) : null,
        actions ? /* @__PURE__ */ jsx("span", { className: "fui-timeline-actions", children: actions }) : null
      ] }),
      children ? /* @__PURE__ */ jsx("div", { className: "fui-timeline-content", children }) : null
    ] })
  ] });
}
export {
  Timeline,
  TimelineItem
};
