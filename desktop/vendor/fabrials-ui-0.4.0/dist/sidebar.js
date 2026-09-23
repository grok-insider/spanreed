"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import * as React from "react";
import { mergeProps } from "@base-ui/react/merge-props";
import { useRender } from "@base-ui/react/use-render";
import { PanelLeftIcon } from "lucide-react";
import { classes } from "./shared.js";
import { Input, Button } from "./controls.js";
import { Skeleton, Separator } from "./display.js";
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetDescription } from "./dialog.js";
import { TooltipTrigger, Tooltip, TooltipContent } from "./menu.js";
function useIsMobile() {
  const [isMobile, setIsMobile] = React.useState(false);
  React.useEffect(() => {
    const query = window.matchMedia("(max-width: 767px)");
    const update = () => setIsMobile(query.matches);
    update();
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);
  return isMobile;
}
const SIDEBAR_COOKIE_NAME = "sidebar_state";
const SIDEBAR_COOKIE_MAX_AGE = 60 * 60 * 24 * 7;
const SIDEBAR_KEYBOARD_SHORTCUT = "b";
const SidebarContext = React.createContext(null);
function useSidebar() {
  const context = React.useContext(SidebarContext);
  if (!context) {
    throw new Error("useSidebar must be used within a SidebarProvider.");
  }
  return context;
}
function SidebarProvider({
  defaultOpen = true,
  open: openProp,
  onOpenChange: setOpenProp,
  className,
  style,
  children,
  ...props
}) {
  const isMobile = useIsMobile();
  const [openMobile, setOpenMobile] = React.useState(false);
  const [_open, _setOpen] = React.useState(defaultOpen);
  const open = openProp ?? _open;
  const setOpen = React.useCallback(
    (value) => {
      const openState = typeof value === "function" ? value(open) : value;
      if (setOpenProp) setOpenProp(openState);
      else _setOpen(openState);
      document.cookie = `${SIDEBAR_COOKIE_NAME}=${openState}; path=/; max-age=${SIDEBAR_COOKIE_MAX_AGE}`;
    },
    [setOpenProp, open]
  );
  const toggleSidebar = React.useCallback(() => {
    return isMobile ? setOpenMobile((value) => !value) : setOpen((value) => !value);
  }, [isMobile, setOpen, setOpenMobile]);
  React.useEffect(() => {
    const handleKeyDown = (event) => {
      if (event.key === SIDEBAR_KEYBOARD_SHORTCUT && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        toggleSidebar();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [toggleSidebar]);
  const state = open ? "expanded" : "collapsed";
  const contextValue = React.useMemo(
    () => ({
      state,
      open,
      setOpen,
      isMobile,
      openMobile,
      setOpenMobile,
      toggleSidebar
    }),
    [state, open, setOpen, isMobile, openMobile, setOpenMobile, toggleSidebar]
  );
  return /* @__PURE__ */ jsx(SidebarContext.Provider, { value: contextValue, children: /* @__PURE__ */ jsx(
    "div",
    {
      "data-slot": "sidebar-wrapper",
      "data-state": state,
      style,
      className: classes("fui-sidebar-wrapper", "group/sidebar-wrapper", className),
      ...props,
      children
    }
  ) });
}
function Sidebar({
  side = "left",
  variant = "sidebar",
  collapsible = "offcanvas",
  className,
  children,
  dir,
  ...props
}) {
  const { isMobile, state, openMobile, setOpenMobile } = useSidebar();
  if (collapsible === "none") {
    return /* @__PURE__ */ jsx(
      "div",
      {
        "data-slot": "sidebar",
        "data-collapsible": "none",
        className: classes("fui-sidebar-static", className),
        ...props,
        children
      }
    );
  }
  if (isMobile) {
    return /* @__PURE__ */ jsx(Sheet, { open: openMobile, onOpenChange: setOpenMobile, ...props, children: /* @__PURE__ */ jsxs(
      SheetContent,
      {
        dir,
        "data-sidebar": "sidebar",
        "data-slot": "sidebar",
        "data-mobile": "true",
        className: "fui-sidebar-sheet",
        showCloseButton: false,
        side,
        children: [
          /* @__PURE__ */ jsxs(SheetHeader, { className: "fui-sr-only", children: [
            /* @__PURE__ */ jsx(SheetTitle, { children: "Navigation" }),
            /* @__PURE__ */ jsx(SheetDescription, { children: "Primary navigation for this workspace." })
          ] }),
          /* @__PURE__ */ jsx("div", { className: "fui-sidebar-inner", children })
        ]
      }
    ) });
  }
  return /* @__PURE__ */ jsxs(
    "div",
    {
      className: "fui-sidebar group peer",
      "data-state": state,
      "data-collapsible": state === "collapsed" ? collapsible : "",
      "data-variant": variant,
      "data-side": side,
      "data-slot": "sidebar",
      children: [
        /* @__PURE__ */ jsx("div", { "data-slot": "sidebar-gap", className: "fui-sidebar-gap" }),
        /* @__PURE__ */ jsx(
          "div",
          {
            "data-slot": "sidebar-container",
            "data-side": side,
            className: classes("fui-sidebar-container", className),
            ...props,
            children: /* @__PURE__ */ jsx(
              "div",
              {
                "data-sidebar": "sidebar",
                "data-slot": "sidebar-inner",
                className: "fui-sidebar-inner",
                children
              }
            )
          }
        )
      ]
    }
  );
}
function SidebarTrigger({
  className,
  onClick,
  children,
  ...props
}) {
  const { toggleSidebar, isMobile, openMobile, open } = useSidebar();
  return /* @__PURE__ */ jsxs(
    Button,
    {
      "data-sidebar": "trigger",
      "data-slot": "sidebar-trigger",
      variant: "ghost",
      size: "icon-sm",
      "aria-expanded": isMobile ? openMobile : open,
      className: classes("fui-sidebar-trigger", className),
      onClick: (event) => {
        onClick?.(event);
        toggleSidebar();
      },
      ...props,
      children: [
        children ?? /* @__PURE__ */ jsx(PanelLeftIcon, { "aria-hidden": true }),
        /* @__PURE__ */ jsx("span", { className: "fui-sr-only", children: "Toggle navigation" })
      ]
    }
  );
}
function SidebarRail({ className, ...props }) {
  const { toggleSidebar } = useSidebar();
  return /* @__PURE__ */ jsx(
    "button",
    {
      "data-sidebar": "rail",
      "data-slot": "sidebar-rail",
      "aria-label": "Toggle navigation",
      tabIndex: -1,
      onClick: toggleSidebar,
      title: "Toggle navigation",
      className: classes("fui-sidebar-rail", className),
      ...props
    }
  );
}
function SidebarInset({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "main",
    {
      "data-slot": "sidebar-inset",
      className: classes("fui-sidebar-inset", className),
      ...props
    }
  );
}
function SidebarInput({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Input,
    {
      "data-slot": "sidebar-input",
      "data-sidebar": "input",
      className: classes("fui-sidebar-input", className),
      ...props
    }
  );
}
function SidebarHeader({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      "data-slot": "sidebar-header",
      "data-sidebar": "header",
      className: classes("fui-sidebar-header", className),
      ...props
    }
  );
}
function SidebarFooter({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      "data-slot": "sidebar-footer",
      "data-sidebar": "footer",
      className: classes("fui-sidebar-footer", className),
      ...props
    }
  );
}
function SidebarSeparator({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Separator,
    {
      "data-slot": "sidebar-separator",
      "data-sidebar": "separator",
      className: classes("fui-sidebar-separator", className),
      ...props
    }
  );
}
function SidebarContent({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      "data-slot": "sidebar-content",
      "data-sidebar": "content",
      className: classes("fui-sidebar-content", className),
      ...props
    }
  );
}
function SidebarGroup({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      "data-slot": "sidebar-group",
      "data-sidebar": "group",
      className: classes("fui-sidebar-group", className),
      ...props
    }
  );
}
function SidebarGroupLabel({
  className,
  render,
  ...props
}) {
  return useRender({
    defaultTagName: "div",
    props: mergeProps(
      { className: classes("fui-sidebar-group-label", className) },
      props
    ),
    render,
    state: { slot: "sidebar-group-label", sidebar: "group-label" }
  });
}
function SidebarGroupAction({
  className,
  render,
  ...props
}) {
  return useRender({
    defaultTagName: "button",
    props: mergeProps(
      { className: classes("fui-sidebar-group-action", className) },
      props
    ),
    render,
    state: { slot: "sidebar-group-action", sidebar: "group-action" }
  });
}
function SidebarGroupContent({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      "data-slot": "sidebar-group-content",
      "data-sidebar": "group-content",
      className: classes("fui-sidebar-group-content", className),
      ...props
    }
  );
}
function SidebarMenu({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "ul",
    {
      "data-slot": "sidebar-menu",
      "data-sidebar": "menu",
      className: classes("fui-sidebar-menu", className),
      ...props
    }
  );
}
function SidebarMenuItem({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "li",
    {
      "data-slot": "sidebar-menu-item",
      "data-sidebar": "menu-item",
      className: classes("fui-sidebar-menu-item", "group/menu-item", className),
      ...props
    }
  );
}
function SidebarMenuButton({
  render,
  isActive = false,
  variant = "default",
  size = "default",
  tooltip,
  className,
  ...props
}) {
  const { isMobile, state } = useSidebar();
  const comp = useRender({
    defaultTagName: "button",
    props: mergeProps(
      {
        className: classes(
          "fui-sidebar-menu-button",
          "peer/menu-button group/menu-button",
          className
        ),
        "aria-current": isActive ? "page" : void 0
      },
      props
    ),
    render: !tooltip ? render : /* @__PURE__ */ jsx(TooltipTrigger, { render }),
    state: {
      slot: "sidebar-menu-button",
      sidebar: "menu-button",
      size,
      variant,
      active: isActive
    }
  });
  if (!tooltip) return comp;
  const content = typeof tooltip === "string" ? { children: tooltip } : tooltip;
  return /* @__PURE__ */ jsxs(Tooltip, { children: [
    comp,
    /* @__PURE__ */ jsx(
      TooltipContent,
      {
        side: "right",
        align: "center",
        hidden: state !== "collapsed" || isMobile,
        ...content
      }
    )
  ] });
}
function SidebarMenuAction({
  className,
  render,
  showOnHover = false,
  ...props
}) {
  return useRender({
    defaultTagName: "button",
    props: mergeProps(
      { className: classes("fui-sidebar-menu-action", className) },
      props
    ),
    render,
    state: {
      slot: "sidebar-menu-action",
      sidebar: "menu-action",
      "show-on-hover": showOnHover
    }
  });
}
function SidebarMenuBadge({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      "data-slot": "sidebar-menu-badge",
      "data-sidebar": "menu-badge",
      className: classes("fui-sidebar-menu-badge", className),
      ...props
    }
  );
}
function SidebarMenuSkeleton({
  className,
  showIcon = false,
  ...props
}) {
  const id = React.useId();
  const width = `${50 + [...id].reduce((sum, char) => sum + char.charCodeAt(0), 0) % 40}%`;
  return /* @__PURE__ */ jsxs(
    "div",
    {
      "data-slot": "sidebar-menu-skeleton",
      "data-sidebar": "menu-skeleton",
      className: classes("fui-sidebar-menu-skeleton", className),
      ...props,
      children: [
        showIcon && /* @__PURE__ */ jsx(
          Skeleton,
          {
            className: "fui-sidebar-menu-skeleton-icon",
            "data-sidebar": "menu-skeleton-icon"
          }
        ),
        /* @__PURE__ */ jsx(
          Skeleton,
          {
            className: "fui-sidebar-menu-skeleton-text",
            "data-sidebar": "menu-skeleton-text",
            style: { maxWidth: width }
          }
        )
      ]
    }
  );
}
function SidebarMenuSub({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "ul",
    {
      "data-slot": "sidebar-menu-sub",
      "data-sidebar": "menu-sub",
      className: classes("fui-sidebar-menu-sub", className),
      ...props
    }
  );
}
function SidebarMenuSubItem({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "li",
    {
      "data-slot": "sidebar-menu-sub-item",
      "data-sidebar": "menu-sub-item",
      className: classes("fui-sidebar-menu-sub-item", className),
      ...props
    }
  );
}
function SidebarMenuSubButton({
  render,
  size = "md",
  isActive = false,
  className,
  ...props
}) {
  return useRender({
    defaultTagName: "a",
    props: mergeProps(
      {
        className: classes("fui-sidebar-menu-sub-button", className),
        "aria-current": isActive ? "page" : void 0
      },
      props
    ),
    render,
    state: {
      slot: "sidebar-menu-sub-button",
      sidebar: "menu-sub-button",
      size,
      active: isActive
    }
  });
}
export {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupAction,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarInput,
  SidebarInset,
  SidebarMenu,
  SidebarMenuAction,
  SidebarMenuBadge,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSkeleton,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
  SidebarProvider,
  SidebarRail,
  SidebarSeparator,
  SidebarTrigger,
  useSidebar
};
