"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { NavigationMenu as NavigationMenu$1 } from "@base-ui/react/navigation-menu";
import { ChevronDownIcon } from "lucide-react";
import { classes } from "./shared.js";
function NavigationMenu({
  align = "start",
  className,
  children,
  ...props
}) {
  return /* @__PURE__ */ jsxs(
    NavigationMenu$1.Root,
    {
      "data-slot": "navigation-menu",
      className: classes("fui-nav-menu", className),
      ...props,
      children: [
        children,
        /* @__PURE__ */ jsx(NavigationMenuPositioner, { align })
      ]
    }
  );
}
function NavigationMenuList({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    NavigationMenu$1.List,
    {
      "data-slot": "navigation-menu-list",
      className: classes("fui-nav-menu-list", className),
      ...props
    }
  );
}
function NavigationMenuItem({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    NavigationMenu$1.Item,
    {
      "data-slot": "navigation-menu-item",
      className: classes("fui-nav-menu-item", className),
      ...props
    }
  );
}
function navigationMenuTriggerStyle(options = {}) {
  return classes("fui-nav-menu-trigger", options.className);
}
function NavigationMenuTrigger({
  className,
  children,
  ...props
}) {
  return /* @__PURE__ */ jsxs(
    NavigationMenu$1.Trigger,
    {
      "data-slot": "navigation-menu-trigger",
      className: classes("fui-nav-menu-trigger", className),
      ...props,
      children: [
        children,
        /* @__PURE__ */ jsx(ChevronDownIcon, { "aria-hidden": true, className: "fui-nav-menu-chevron" })
      ]
    }
  );
}
function NavigationMenuContent({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    NavigationMenu$1.Content,
    {
      "data-slot": "navigation-menu-content",
      className: classes("fui-nav-menu-content", className),
      ...props
    }
  );
}
function NavigationMenuPositioner({
  className,
  side = "bottom",
  sideOffset = 8,
  align = "start",
  alignOffset = 0,
  ...props
}) {
  return /* @__PURE__ */ jsx(NavigationMenu$1.Portal, { children: /* @__PURE__ */ jsx(
    NavigationMenu$1.Positioner,
    {
      side,
      sideOffset,
      align,
      alignOffset,
      className: classes("fui-positioner", "fui-nav-menu-positioner", className),
      ...props,
      children: /* @__PURE__ */ jsx(NavigationMenu$1.Popup, { className: "fui-nav-menu-popup", children: /* @__PURE__ */ jsx(NavigationMenu$1.Viewport, { className: "fui-nav-menu-viewport" }) })
    }
  ) });
}
function NavigationMenuLink({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    NavigationMenu$1.Link,
    {
      "data-slot": "navigation-menu-link",
      className: classes("fui-nav-menu-link", className),
      ...props
    }
  );
}
function NavigationMenuIndicator({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    NavigationMenu$1.Icon,
    {
      "data-slot": "navigation-menu-indicator",
      className: classes("fui-nav-menu-indicator", className),
      ...props,
      children: /* @__PURE__ */ jsx("span", { className: "fui-nav-menu-indicator-arrow" })
    }
  );
}
export {
  NavigationMenu,
  NavigationMenuContent,
  NavigationMenuIndicator,
  NavigationMenuItem,
  NavigationMenuLink,
  NavigationMenuList,
  NavigationMenuPositioner,
  NavigationMenuTrigger,
  navigationMenuTriggerStyle
};
