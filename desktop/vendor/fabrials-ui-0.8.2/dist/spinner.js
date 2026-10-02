"use client";
import { jsx } from "react/jsx-runtime";
import { LoaderCircle } from "lucide-react";
import { classes } from "./shared.js";
function Spinner({
  className,
  label = "Loading",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    LoaderCircle,
    {
      "data-slot": "spinner",
      role: "status",
      "aria-label": label,
      className: classes("fui-spinner", "fui-spin", className),
      ...props
    }
  );
}
export {
  Spinner
};
