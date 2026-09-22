"use client";
import { jsx } from "react/jsx-runtime";
import { Menu } from "@base-ui/react/menu";
import { Tooltip as Tooltip$1 } from "@base-ui/react/tooltip";
import { classes } from "./shared.js";
const DropdownMenu = Menu.Root;
const DropdownMenuTrigger = Menu.Trigger;
const DropdownMenuGroup = Menu.Group;
function DropdownMenuContent({
  className,
  align = "end",
  sideOffset = 6,
  ...props
}) {
  return /* @__PURE__ */ jsx(Menu.Portal, { children: /* @__PURE__ */ jsx(
    Menu.Positioner,
    {
      className: "fui-positioner",
      align,
      sideOffset,
      children: /* @__PURE__ */ jsx(Menu.Popup, { className: classes("fui-menu", className), ...props })
    }
  ) });
}
function DropdownMenuItem({
  className,
  destructive = false,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Menu.Item,
    {
      className: classes("fui-menu-item", className),
      "data-destructive": destructive || void 0,
      ...props
    }
  );
}
function DropdownMenuLabel({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Menu.GroupLabel,
    {
      className: classes("fui-menu-label", className),
      ...props
    }
  );
}
function DropdownMenuSeparator({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Menu.Separator,
    {
      className: classes("fui-separator", className),
      ...props
    }
  );
}
const TooltipProvider = Tooltip$1.Provider;
const Tooltip = Tooltip$1.Root;
const TooltipTrigger = Tooltip$1.Trigger;
function TooltipContent({
  className,
  sideOffset = 6,
  side = "top",
  align = "center",
  ...props
}) {
  return /* @__PURE__ */ jsx(Tooltip$1.Portal, { children: /* @__PURE__ */ jsx(
    Tooltip$1.Positioner,
    {
      className: "fui-positioner",
      sideOffset,
      side,
      align,
      children: /* @__PURE__ */ jsx(
        Tooltip$1.Popup,
        {
          className: classes("fui-tooltip", className),
          ...props
        }
      )
    }
  ) });
}
export {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger
};
