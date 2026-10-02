"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { Button } from "./controls.js";
import { Popover, PopoverTrigger, PopoverContent, PopoverHeader, PopoverTitle, PopoverDescription } from "./popover.js";
import { classes } from "./shared.js";
function StatusPopover({
  title,
  description,
  icon,
  label,
  attention = false,
  compact = false,
  side = "bottom",
  align = "end",
  keepMounted,
  className,
  children,
  ...root
}) {
  const tone = attention === true ? "danger" : attention || void 0;
  return /* @__PURE__ */ jsx(
    "span",
    {
      className: classes("fui-status-popover", className),
      "data-attention": tone,
      "data-compact": compact || void 0,
      children: /* @__PURE__ */ jsxs(Popover, { ...root, children: [
        /* @__PURE__ */ jsx(
          PopoverTrigger,
          {
            render: label ? /* @__PURE__ */ jsxs(
              Button,
              {
                variant: "ghost",
                size: "lg",
                className: "fui-status-popover-trigger",
                title: description,
                children: [
                  icon,
                  /* @__PURE__ */ jsxs("span", { className: "fui-sr-only", children: [
                    title,
                    ": "
                  ] }),
                  /* @__PURE__ */ jsx("span", { className: compact ? "fui-sr-only" : "fui-status-popover-label", children: label })
                ]
              }
            ) : /* @__PURE__ */ jsx(
              Button,
              {
                variant: "ghost",
                size: "icon-lg",
                className: "fui-status-popover-trigger",
                "aria-label": `${title}: ${description}`,
                title: description,
                children: icon
              }
            )
          }
        ),
        /* @__PURE__ */ jsxs(
          PopoverContent,
          {
            side,
            align,
            keepMounted,
            className: "fui-status-popover-content",
            children: [
              /* @__PURE__ */ jsxs(PopoverHeader, { children: [
                /* @__PURE__ */ jsx(PopoverTitle, { children: title }),
                /* @__PURE__ */ jsx(PopoverDescription, { children: description })
              ] }),
              children
            ]
          }
        )
      ] })
    }
  );
}
export {
  StatusPopover
};
