"use client";
import { jsx } from "react/jsx-runtime";
import { classes } from "./shared.js";
import { Button, Input, Textarea } from "./controls.js";
function InputGroup({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      "data-slot": "input-group",
      role: "group",
      className: classes("fui-input-group", className),
      ...props
    }
  );
}
function InputGroupAddon({
  className,
  align = "inline-start",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      role: "group",
      "data-slot": "input-group-addon",
      "data-align": align ?? "inline-start",
      className: classes("fui-input-group-addon", className),
      onClick: (event) => {
        if (event.target.closest("button")) return;
        event.currentTarget.parentElement?.querySelector("input, textarea")?.focus();
      },
      ...props
    }
  );
}
function InputGroupButton({
  className,
  type = "button",
  variant = "ghost",
  size = "xs",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Button,
    {
      type,
      variant,
      size: size ?? "xs",
      className: classes("fui-input-group-button", className),
      ...props
    }
  );
}
function InputGroupText({ className, ...props }) {
  return /* @__PURE__ */ jsx("span", { className: classes("fui-input-group-text", className), ...props });
}
function InputGroupInput({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Input,
    {
      "data-slot": "input-group-control",
      className: classes("fui-input-group-control", className),
      ...props
    }
  );
}
function InputGroupTextarea({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Textarea,
    {
      "data-slot": "input-group-control",
      className: classes("fui-input-group-control", className),
      ...props
    }
  );
}
export {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
  InputGroupText,
  InputGroupTextarea
};
