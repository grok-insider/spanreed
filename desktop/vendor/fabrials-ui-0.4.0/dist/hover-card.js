"use client";
import { jsx } from "react/jsx-runtime";
import { PreviewCard } from "@base-ui/react/preview-card";
import { classes } from "./shared.js";
function HoverCard({ ...props }) {
  return /* @__PURE__ */ jsx(PreviewCard.Root, { "data-slot": "hover-card", ...props });
}
function HoverCardTrigger({ ...props }) {
  return /* @__PURE__ */ jsx(PreviewCard.Trigger, { "data-slot": "hover-card-trigger", ...props });
}
function HoverCardContent({
  className,
  side = "bottom",
  sideOffset = 6,
  align = "center",
  alignOffset = 0,
  ...props
}) {
  return /* @__PURE__ */ jsx(PreviewCard.Portal, { "data-slot": "hover-card-portal", children: /* @__PURE__ */ jsx(
    PreviewCard.Positioner,
    {
      align,
      alignOffset,
      side,
      sideOffset,
      className: "fui-positioner",
      children: /* @__PURE__ */ jsx(
        PreviewCard.Popup,
        {
          "data-slot": "hover-card-content",
          className: classes("fui-popover", "fui-hover-card", className),
          ...props
        }
      )
    }
  ) });
}
export {
  HoverCard,
  HoverCardContent,
  HoverCardTrigger
};
