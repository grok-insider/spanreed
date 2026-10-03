"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { classes } from "./shared.js";
function FieldSet({ className, ...props }) {
  return /* @__PURE__ */ jsx("fieldset", { "data-slot": "field-set", className: classes("fui-fieldset", className), ...props });
}
function FieldLegend({
  className,
  variant = "title",
  divider = true,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "legend",
    {
      "data-slot": "field-legend",
      "data-variant": variant,
      "data-divider": divider ? "" : void 0,
      className: classes("fui-field-legend", className),
      ...props
    }
  );
}
function FieldGroup({
  className,
  layout = "stack",
  ...props
}) {
  return /* @__PURE__ */ jsx("div", { "data-slot": "field-group", "data-layout": layout, className: classes("fui-field-group", className), ...props });
}
function NativeRadioGroup({
  legend,
  hideLegend = false,
  layout = "stack",
  className,
  children,
  ...props
}) {
  return /* @__PURE__ */ jsxs(FieldSet, { "data-slot": "native-radio-group", className: classes("fui-native-radio-group", className), ...props, children: [
    /* @__PURE__ */ jsx(FieldLegend, { variant: "label", divider: false, className: hideLegend ? "fui-sr-only" : void 0, children: legend }),
    /* @__PURE__ */ jsx("div", { className: "fui-native-radio-options", "data-layout": layout, children })
  ] });
}
export {
  FieldGroup,
  FieldLegend,
  FieldSet,
  NativeRadioGroup
};
