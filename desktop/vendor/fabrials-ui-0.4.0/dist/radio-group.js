"use client";
import { jsx } from "react/jsx-runtime";
import { Radio as Radio$1 } from "@base-ui/react/radio";
import { RadioGroup as RadioGroup$1 } from "@base-ui/react/radio-group";
import { classes } from "./shared.js";
function RadioGroup({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    RadioGroup$1,
    {
      "data-slot": "radio-group",
      className: classes("fui-radio-group", className),
      ...props
    }
  );
}
function Radio({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Radio$1.Root,
    {
      "data-slot": "radio",
      className: classes("fui-radio", className),
      ...props,
      children: /* @__PURE__ */ jsx(Radio$1.Indicator, { className: "fui-radio-indicator" })
    }
  );
}
export {
  Radio,
  RadioGroup
};
