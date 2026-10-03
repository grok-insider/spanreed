"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { Button } from "./controls.js";
import { keyShortcutsValue, IconTooltipContent } from "./icon-tooltip.js";
import { Tooltip, TooltipTrigger } from "./menu.js";
function IconButton({
  label,
  tooltip = true,
  shortcut,
  textName = false,
  size = "icon-lg",
  variant = "ghost",
  loading = false,
  children,
  ...props
}) {
  const button = /* @__PURE__ */ jsxs(
    Button,
    {
      "aria-keyshortcuts": keyShortcutsValue(shortcut),
      ...props,
      variant,
      size,
      loading,
      "aria-label": textName ? void 0 : label,
      children: [
        loading ? null : children,
        textName ? /* @__PURE__ */ jsx("span", { className: "fui-sr-only", children: label }) : null
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
  IconButton
};
