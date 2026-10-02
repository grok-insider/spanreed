"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { Select as Select$1 } from "@base-ui/react/select";
import { Check, ChevronDown, ChevronUp } from "lucide-react";
import { classes } from "./shared.js";
const Select = Select$1.Root;
const SelectValue = Select$1.Value;
const SelectGroup = Select$1.Group;
function SelectLabel({ className, ...props }) {
  return /* @__PURE__ */ jsx(Select$1.GroupLabel, { className: classes("fui-menu-label", className), ...props });
}
function SelectSeparator({ className, ...props }) {
  return /* @__PURE__ */ jsx(Select$1.Separator, { className: classes("fui-menu-separator", "fui-separator", className), ...props });
}
function SelectScrollUpButton({ className, ...props }) {
  return /* @__PURE__ */ jsx(Select$1.ScrollUpArrow, { className: classes("fui-menu-scroll-arrow", className), ...props, children: /* @__PURE__ */ jsx(ChevronUp, { "aria-hidden": true, size: 16 }) });
}
function SelectScrollDownButton({ className, ...props }) {
  return /* @__PURE__ */ jsx(Select$1.ScrollDownArrow, { className: classes("fui-menu-scroll-arrow", className), ...props, children: /* @__PURE__ */ jsx(ChevronDown, { "aria-hidden": true, size: 16 }) });
}
function SelectTrigger({
  className,
  children,
  size,
  ...props
}) {
  return /* @__PURE__ */ jsxs(
    Select$1.Trigger,
    {
      className: classes("fui-input", "fui-select-trigger", className),
      "data-size": size,
      ...props,
      children: [
        children,
        /* @__PURE__ */ jsx(Select$1.Icon, { children: /* @__PURE__ */ jsx(ChevronDown, { "aria-hidden": true, size: 16 }) })
      ]
    }
  );
}
function SelectContent({
  className,
  children,
  align = "center",
  ...props
}) {
  return /* @__PURE__ */ jsx(Select$1.Portal, { children: /* @__PURE__ */ jsx(
    Select$1.Positioner,
    {
      className: "fui-positioner",
      sideOffset: 6,
      align,
      alignItemWithTrigger: false,
      children: /* @__PURE__ */ jsx(Select$1.Popup, { className: classes("fui-menu", className), ...props, children: /* @__PURE__ */ jsx(Select$1.List, { children }) })
    }
  ) });
}
function SelectItem({
  className,
  children,
  ...props
}) {
  return /* @__PURE__ */ jsxs(Select$1.Item, { className: classes("fui-menu-item", className), ...props, children: [
    /* @__PURE__ */ jsx(Select$1.ItemText, { children }),
    /* @__PURE__ */ jsx(Select$1.ItemIndicator, { className: "fui-menu-indicator", children: /* @__PURE__ */ jsx(Check, { "aria-hidden": true, size: 16 }) })
  ] });
}
export {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectScrollDownButton,
  SelectScrollUpButton,
  SelectSeparator,
  SelectTrigger,
  SelectValue
};
