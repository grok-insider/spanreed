"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { Command as Command$1 } from "cmdk";
import { SearchIcon, CheckIcon } from "lucide-react";
import { classes } from "./shared.js";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from "./dialog.js";
function Command({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Command$1,
    {
      "data-slot": "command",
      className: classes("fui-command", className),
      ...props
    }
  );
}
function CommandDialog({
  title = "Command palette",
  description = "Search for a page or an action.",
  children,
  className,
  showCloseButton = false,
  ...props
}) {
  return /* @__PURE__ */ jsx(Dialog, { ...props, children: /* @__PURE__ */ jsxs(
    DialogContent,
    {
      className: classes("fui-command-dialog", className),
      showCloseButton,
      children: [
        /* @__PURE__ */ jsxs(DialogHeader, { className: "fui-sr-only", children: [
          /* @__PURE__ */ jsx(DialogTitle, { children: title }),
          /* @__PURE__ */ jsx(DialogDescription, { children: description })
        ] }),
        children
      ]
    }
  ) });
}
function CommandInput({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsxs("div", { "data-slot": "command-input-wrapper", className: "fui-command-input-wrapper", children: [
    /* @__PURE__ */ jsx(SearchIcon, { "aria-hidden": true, className: "fui-command-input-icon" }),
    /* @__PURE__ */ jsx(
      Command$1.Input,
      {
        "data-slot": "command-input",
        className: classes("fui-command-input", className),
        ...props
      }
    )
  ] });
}
function CommandList({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Command$1.List,
    {
      "data-slot": "command-list",
      className: classes("fui-command-list", className),
      ...props
    }
  );
}
function CommandEmpty({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Command$1.Empty,
    {
      "data-slot": "command-empty",
      className: classes("fui-command-empty", className),
      ...props
    }
  );
}
function CommandGroup({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Command$1.Group,
    {
      "data-slot": "command-group",
      className: classes("fui-command-group", className),
      ...props
    }
  );
}
function CommandSeparator({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Command$1.Separator,
    {
      "data-slot": "command-separator",
      className: classes("fui-command-separator", className),
      ...props
    }
  );
}
function CommandItem({
  className,
  children,
  ...props
}) {
  return /* @__PURE__ */ jsxs(
    Command$1.Item,
    {
      "data-slot": "command-item",
      className: classes("fui-command-item", className),
      ...props,
      children: [
        children,
        /* @__PURE__ */ jsx(CheckIcon, { "aria-hidden": true, className: "fui-command-check" })
      ]
    }
  );
}
function CommandShortcut({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "span",
    {
      "data-slot": "command-shortcut",
      className: classes("fui-command-shortcut", className),
      ...props
    }
  );
}
export {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
  CommandShortcut
};
