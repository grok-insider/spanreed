"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { useRef, useEffect } from "react";
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
function NativeSelectOption(props) {
  return /* @__PURE__ */ jsx("option", { ...props });
}
function NativeSelectOptGroup(props) {
  return /* @__PURE__ */ jsx("optgroup", { ...props });
}
function NativeChoice({
  className,
  label,
  labelClassName,
  type,
  ...props
}) {
  const input = /* @__PURE__ */ jsx(
    "input",
    {
      className: classes(type === "radio" ? "fui-native-radio" : "fui-native-checkbox", className),
      ...props,
      type
    }
  );
  if (label == null) return input;
  return /* @__PURE__ */ jsxs("label", { className: classes("fui-native-choice", labelClassName), children: [
    input,
    /* @__PURE__ */ jsx("span", { children: label })
  ] });
}
function setRef(ref, node) {
  if (typeof ref === "function") return ref(node);
  if (ref) ref.current = node;
}
function NativeCheckbox({
  indeterminate,
  onChange,
  ref,
  ...props
}) {
  const own = useRef(null);
  const wanted = useRef(indeterminate);
  useEffect(() => {
    wanted.current = indeterminate;
    if (own.current && indeterminate !== void 0) own.current.indeterminate = indeterminate;
  });
  return /* @__PURE__ */ jsx(
    NativeChoice,
    {
      ...props,
      type: "checkbox",
      "data-indeterminate": indeterminate ? "" : void 0,
      ref: (node) => {
        own.current = node;
        return setRef(ref, node);
      },
      onChange: (event) => {
        onChange?.(event);
        if (indeterminate === void 0) return;
        queueMicrotask(() => {
          if (own.current) own.current.indeterminate = !!wanted.current;
        });
      }
    }
  );
}
function NativeRadio(props) {
  return /* @__PURE__ */ jsx(NativeChoice, { ...props, type: "radio" });
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
  NativeRadio,
  NativeSelect,
  NativeSelectOptGroup,
  NativeSelectOption,
  Switch,
  Textarea
};
