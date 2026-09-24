"use client";
import { jsx } from "react/jsx-runtime";
import { Popover as Popover$1 } from "@base-ui/react/popover";
import { classes } from "./shared.js";
function Popover({ ...props }) {
  return /* @__PURE__ */ jsx(Popover$1.Root, { "data-slot": "popover", ...props });
}
function PopoverTrigger({ ...props }) {
  return /* @__PURE__ */ jsx(Popover$1.Trigger, { "data-slot": "popover-trigger", ...props });
}
function PopoverContent({
  className,
  align = "center",
  alignOffset = 0,
  side = "bottom",
  sideOffset = 6,
  ...props
}) {
  return /* @__PURE__ */ jsx(Popover$1.Portal, { children: /* @__PURE__ */ jsx(
    Popover$1.Positioner,
    {
      align,
      alignOffset,
      side,
      sideOffset,
      className: "fui-positioner",
      children: /* @__PURE__ */ jsx(
        Popover$1.Popup,
        {
          "data-slot": "popover-content",
          className: classes("fui-popover", className),
          ...props
        }
      )
    }
  ) });
}
function PopoverHeader({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      "data-slot": "popover-header",
      className: classes("fui-popover-header", className),
      ...props
    }
  );
}
function PopoverTitle({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Popover$1.Title,
    {
      "data-slot": "popover-title",
      className: classes("fui-popover-title", className),
      ...props
    }
  );
}
function PopoverDescription({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Popover$1.Description,
    {
      "data-slot": "popover-description",
      className: classes("fui-description", className),
      ...props
    }
  );
}
export {
  Popover,
  PopoverContent,
  PopoverDescription,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger
};
