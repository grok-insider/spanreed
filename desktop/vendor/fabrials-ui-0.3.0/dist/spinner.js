"use client";
import { jsx } from "react/jsx-runtime";
import { Loader2Icon } from "lucide-react";
import { classes } from "./shared.js";
function Spinner({ className, ...props }) {
  return /* @__PURE__ */ jsx(Loader2Icon, { "data-slot": "spinner", role: "status", "aria-label": "Loading", className: classes("size-4 animate-spin", className), ...props });
}
export {
  Spinner
};
