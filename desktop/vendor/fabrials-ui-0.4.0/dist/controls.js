"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { Button as Button$1 } from "@base-ui/react/button";
import { Checkbox as Checkbox$1 } from "@base-ui/react/checkbox";
import { Switch as Switch$1 } from "@base-ui/react/switch";
import { LoaderCircle, Minus, Check } from "lucide-react";
import { classes } from "./shared.js";
function Button({
  className,
  variant = "default",
  size = "default",
  loading = false,
  disabled,
  focusableWhenDisabled,
  children,
  ...props
}) {
  return /* @__PURE__ */ jsxs(
    Button$1,
    {
      "data-slot": "button",
      className: classes("fui-button", className),
      "data-variant": variant,
      "data-size": size,
      "data-loading": loading || void 0,
      "aria-busy": loading || void 0,
      disabled: disabled || loading,
      focusableWhenDisabled: focusableWhenDisabled ?? loading,
      ...props,
      children: [
        loading ? /* @__PURE__ */ jsx(LoaderCircle, { "aria-hidden": true, className: "fui-button-spinner fui-spin" }) : null,
        children
      ]
    }
  );
}
function Input({ className, ...props }) {
  return /* @__PURE__ */ jsx("input", { className: classes("fui-input", className), ...props });
}
function Textarea({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "textarea",
    {
      className: classes("fui-input", "fui-textarea", className),
      ...props
    }
  );
}
function NativeSelect({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "select",
    {
      className: classes("fui-input", "fui-native-select", className),
      ...props
    }
  );
}
function NativeCheckbox({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "input",
    {
      className: classes("fui-native-checkbox", className),
      ...props,
      type: "checkbox"
    }
  );
}
function Label({ className, ...props }) {
  return /* @__PURE__ */ jsx("label", { className: classes("fui-label", className), ...props });
}
function Checkbox({
  className,
  indeterminate,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Checkbox$1.Root,
    {
      className: classes("fui-checkbox", className),
      indeterminate,
      ...props,
      children: /* @__PURE__ */ jsx(Checkbox$1.Indicator, { className: "fui-control-indicator", children: indeterminate ? /* @__PURE__ */ jsx(Minus, { "aria-hidden": true, size: 12, strokeWidth: 3 }) : /* @__PURE__ */ jsx(Check, { "aria-hidden": true, size: 12, strokeWidth: 3 }) })
    }
  );
}
function Switch({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(Switch$1.Root, { className: classes("fui-switch", className), ...props, children: /* @__PURE__ */ jsx(Switch$1.Thumb, { className: "fui-switch-thumb" }) });
}
export {
  Button,
  Checkbox,
  Input,
  Label,
  NativeCheckbox,
  NativeSelect,
  Switch,
  Textarea
};
