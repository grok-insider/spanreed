"use client";
import { jsx } from "react/jsx-runtime";
import { classes } from "./shared.js";
function ShimmerText({
  children,
  as: Component = "p",
  className,
  duration = 2,
  spread = 2,
  style,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Component,
    {
      className: classes("fui-shimmer-text", className),
      style: {
        "--fui-shimmer-spread": `${children.length * spread}px`,
        "--fui-shimmer-duration": `${duration}s`,
        ...style
      },
      ...props,
      children
    }
  );
}
export {
  ShimmerText
};
