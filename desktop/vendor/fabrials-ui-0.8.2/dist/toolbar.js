"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { useRef, useState, useEffect } from "react";
import { Toolbar as Toolbar$1 } from "@base-ui/react/toolbar";
import { keyShortcutsValue, IconTooltipContent } from "./icon-tooltip.js";
import { Tooltip, TooltipTrigger } from "./menu.js";
import { classes } from "./shared.js";
function moveToEdge(event) {
  if (event.key !== "Home" && event.key !== "End") return;
  if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
  const root = event.currentTarget;
  const target = event.target;
  if (!root.contains(target) || target.closest("input, textarea, select, [contenteditable]:not([contenteditable='false'])")) return;
  const items = Array.from(root.querySelectorAll("[tabindex]")).filter(
    (item) => item.closest('[role="toolbar"]') === root && !item.hasAttribute("disabled") && /^-?[01]$/.test(item.getAttribute("tabindex") ?? "") && getComputedStyle(item).display !== "none"
  );
  const next = event.key === "Home" ? items[0] : items[items.length - 1];
  if (!next) return;
  event.preventDefault();
  next.focus();
}
function Toolbar({ className, variant = "plain", sticky = false, contain, onKeyDown, ...props }) {
  return /* @__PURE__ */ jsx(
    Toolbar$1.Root,
    {
      onKeyDown: (event) => {
        onKeyDown?.(event);
        moveToEdge(event);
      },
      "data-slot": "toolbar",
      "data-variant": variant,
      "data-sticky": sticky && variant === "bar" ? "" : void 0,
      "data-contain": contain ?? variant === "bar" ? "" : void 0,
      className: classes("fui-toolbar", className),
      ...props
    }
  );
}
function ToolbarGroup({ className, ...props }) {
  return /* @__PURE__ */ jsx(Toolbar$1.Group, { "data-slot": "toolbar-group", className: classes("fui-toolbar-group", className), ...props });
}
function ToolbarSeparator({
  className,
  tier,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Toolbar$1.Separator,
    {
      "data-slot": "toolbar-separator",
      "data-tier": tier === "low" ? tier : void 0,
      className: classes("fui-toolbar-separator", className),
      ...props
    }
  );
}
function useLayoutHidden(ref, enabled) {
  const [hidden, setHidden] = useState(false);
  useEffect(() => {
    const element = ref.current;
    if (!enabled || !element) return;
    const measure = () => setHidden(getComputedStyle(element).display === "none");
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [enabled, ref]);
  return hidden;
}
function ToolbarButton({
  label,
  children,
  tooltip = true,
  shortcut,
  size = "icon-lg",
  variant = "ghost",
  reveal,
  tier = "high",
  className,
  disabled,
  focusableWhenDisabled = true,
  ref,
  ...props
}) {
  const own = useRef(null);
  const hidden = useLayoutHidden(own, tier !== "high");
  const button = /* @__PURE__ */ jsxs(
    Toolbar$1.Button,
    {
      "aria-keyshortcuts": keyShortcutsValue(shortcut),
      ...props,
      "data-slot": "button",
      "data-variant": variant,
      "data-size": size,
      "data-tier": tier === "high" ? void 0 : tier,
      "data-reveal": reveal,
      className: classes("fui-button", "fui-toolbar-button", className),
      disabled: disabled || hidden,
      focusableWhenDisabled: hidden ? false : focusableWhenDisabled,
      ref: (node) => {
        own.current = node;
        if (typeof ref === "function") return ref(node);
        if (ref) ref.current = node;
      },
      children: [
        children,
        /* @__PURE__ */ jsx("span", { className: "fui-toolbar-label", children: label })
      ]
    }
  );
  if (tooltip === false) return button;
  return /* @__PURE__ */ jsxs(Tooltip, { children: [
    /* @__PURE__ */ jsx(TooltipTrigger, { render: button }),
    /* @__PURE__ */ jsx(IconTooltipContent, { label: tooltip === true ? label : tooltip, shortcut })
  ] });
}
export {
  Toolbar,
  ToolbarButton,
  ToolbarGroup,
  ToolbarSeparator
};
