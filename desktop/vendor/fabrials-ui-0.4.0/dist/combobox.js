"use client";
import { jsx } from "react/jsx-runtime";
import { Combobox as Combobox$1 } from "@base-ui/react/combobox";
import { classes } from "./shared.js";
function Input({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    Combobox$1.Input,
    {
      className: classes("fui-input", className),
      ...props
    }
  );
}
function Popup({ className, ...props }) {
  return /* @__PURE__ */ jsx(Combobox$1.Popup, { className: classes("fui-menu", className), ...props });
}
function Item({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    Combobox$1.Item,
    {
      className: classes("fui-menu-item", className),
      ...props
    }
  );
}
function Chips({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    Combobox$1.Chips,
    {
      className: classes("fui-combobox-chips", className),
      ...props
    }
  );
}
function Chip({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    Combobox$1.Chip,
    {
      className: classes("fui-combobox-chip", className),
      ...props
    }
  );
}
const Combobox = { ...Combobox$1, Input, Popup, Item, Chips, Chip };
export {
  Combobox
};
