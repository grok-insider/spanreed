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
  sideOffset = 4,
  align = "center",
  alignOffset = 4,
  ...props
}) {
  return /* @__PURE__ */ jsx(PreviewCard.Portal, { "data-slot": "hover-card-portal", children: /* @__PURE__ */ jsx(
    PreviewCard.Positioner,
    {
      align,
      alignOffset,
      side,
      sideOffset,
      className: "isolate z-50",
      children: /* @__PURE__ */ jsx(
        PreviewCard.Popup,
        {
          "data-slot": "hover-card-content",
          className: classes(
            "z-50 w-64 origin-(--transform-origin) rounded-lg bg-popover p-2.5 text-sm text-popover-foreground shadow-md ring-1 ring-foreground/10 outline-hidden duration-100 data-[side=bottom]:slide-in-from-top-2 data-[side=inline-end]:slide-in-from-left-2 data-[side=inline-start]:slide-in-from-right-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95",
            className
          ),
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
