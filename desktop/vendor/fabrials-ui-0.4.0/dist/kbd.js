"use client";
import { jsx } from "react/jsx-runtime";
import { classes } from "./shared.js";
function Kbd({ className, ...props }) {
  return /* @__PURE__ */ jsx("kbd", { className: classes("fui-kbd", className), ...props });
}
export {
  Kbd
};
