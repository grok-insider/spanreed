"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { Menu } from "@base-ui/react/menu";
import { Tooltip as Tooltip$1 } from "@base-ui/react/tooltip";
import { Check, Dot } from "lucide-react";
import { classes } from "./shared.js";
const DropdownMenu = Menu.Root;
const DropdownMenuTrigger = Menu.Trigger;
const DropdownMenuGroup = Menu.Group;
const DropdownMenuRadioGroup = Menu.RadioGroup;
function DropdownMenuContent({
  className,
  align = "end",
  side = "bottom",
  sideOffset = 6,
  collisionAvoidance,
  ...props
}) {
  return /* @__PURE__ */ jsx(Menu.Portal, { children: /* @__PURE__ */ jsx(
    Menu.Positioner,
    {
      className: "fui-positioner",
      align,
      side,
      sideOffset,
      collisionAvoidance,
      children: /* @__PURE__ */ jsx(Menu.Popup, { className: classes("fui-menu", className), ...props })
    }
  ) });
}
function DropdownMenuItem({
  className,
  destructive = false,
  variant = "default",
  ...props
}) {
  const isDestructive = destructive || variant === "destructive";
  return /* @__PURE__ */ jsx(
    Menu.Item,
    {
      className: classes("fui-menu-item", className),
      "data-destructive": isDestructive || void 0,
      ...props
    }
  );
}
function DropdownMenuCheckboxItem({
  className,
  children,
  ...props
}) {
  return /* @__PURE__ */ jsxs(
    Menu.CheckboxItem,
    {
      className: classes("fui-menu-item", className),
      ...props,
      children: [
        children,
        /* @__PURE__ */ jsx(Menu.CheckboxItemIndicator, { className: "fui-menu-indicator", children: /* @__PURE__ */ jsx(Check, { "aria-hidden": true, size: 16 }) })
      ]
    }
  );
}
function DropdownMenuRadioItem({
  className,
  children,
  ...props
}) {
  return /* @__PURE__ */ jsxs(
    Menu.RadioItem,
    {
      className: classes("fui-menu-item", className),
      ...props,
      children: [
        children,
        /* @__PURE__ */ jsx(Menu.RadioItemIndicator, { className: "fui-menu-indicator", children: /* @__PURE__ */ jsx(Dot, { "aria-hidden": true, size: 20, strokeWidth: 4 }) })
      ]
    }
  );
}
function DropdownMenuShortcut({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "span",
    {
      "aria-hidden": true,
      className: classes("fui-menu-shortcut", className),
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
      className: classes("fui-separator", "fui-menu-separator", className),
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
      className: "fui-positioner fui-tooltip-positioner",
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
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger
};
