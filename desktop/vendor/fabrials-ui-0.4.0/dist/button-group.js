"use client";
import { jsx } from "react/jsx-runtime";
import { mergeProps } from "@base-ui/react/merge-props";
import { useRender } from "@base-ui/react/use-render";
import { classes } from "./shared.js";
import { Separator } from "./display.js";
function buttonGroupVariants({
  orientation = "horizontal",
  className
} = {}) {
  return classes("fui-button-group", `fui-button-group-${orientation ?? "horizontal"}`, className);
}
function ButtonGroup({
  className,
  orientation = "horizontal",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      role: "group",
      "data-slot": "button-group",
      "data-orientation": orientation ?? "horizontal",
      className: buttonGroupVariants({ orientation, className }),
      ...props
    }
  );
}
function ButtonGroupText({
  className,
  render,
  ...props
}) {
  return useRender({
    defaultTagName: "div",
    props: mergeProps(
      { className: classes("fui-button-group-text", className) },
      props
    ),
    render,
    state: { slot: "button-group-text" }
  });
}
function ButtonGroupSeparator({
  className,
  orientation = "vertical",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Separator,
    {
      "data-slot": "button-group-separator",
      orientation,
      className: classes("fui-button-group-separator", className),
      ...props
    }
  );
}
export {
  ButtonGroup,
  ButtonGroupSeparator,
  ButtonGroupText,
  buttonGroupVariants
};
