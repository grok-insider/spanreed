"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { Search } from "lucide-react";
import { Fragment } from "react";
import { Button } from "./controls.js";
import { KbdGroup, Kbd } from "./kbd.js";
import { classes } from "./shared.js";
function shortcutOf(keys) {
  const last = keys[keys.length - 1];
  if (!last || last === "mod") return void 0;
  const chord = keys.slice(0, -1);
  if (chord.length === 1 && chord[0] === "mod") return `Control+${last} Meta+${last}`;
  return keys.map((key) => key === "mod" ? "Control" : key).join("+");
}
function CommandTrigger({
  icon,
  label,
  name,
  keys,
  sequence = false,
  separator = "then",
  shortcut,
  compact = false,
  variant = "secondary",
  className,
  ...props
}) {
  const derived = shortcut === false ? void 0 : shortcut ?? (keys && !sequence ? shortcutOf(keys) : void 0);
  return /* @__PURE__ */ jsxs(
    Button,
    {
      variant,
      size: "lg",
      className: classes("fui-command-trigger", className),
      "data-compact": compact || void 0,
      "aria-label": name ?? (typeof label === "string" ? label : void 0),
      "aria-keyshortcuts": derived,
      ...props,
      children: [
        icon ?? /* @__PURE__ */ jsx(Search, { "aria-hidden": true }),
        /* @__PURE__ */ jsx("span", { className: "fui-command-trigger-label", children: label }),
        keys && keys.length > 0 && /* @__PURE__ */ jsx(KbdGroup, { className: "fui-command-trigger-keys", sequence, separator, "aria-hidden": true, children: keys.map((key, index) => /* @__PURE__ */ jsx(Fragment, { children: key === "mod" ? /* @__PURE__ */ jsx(Kbd, { mod: true }) : /* @__PURE__ */ jsx(Kbd, { children: key }) }, index)) })
      ]
    }
  );
}
function CommandOptionList({ className, ...props }) {
  return /* @__PURE__ */ jsx("ul", { role: "listbox", tabIndex: -1, className: classes("fui-command-options", className), ...props });
}
function CommandOption({
  icon,
  label,
  detail,
  group,
  reason,
  keys,
  active = false,
  disabled = false,
  className,
  ...props
}) {
  return /* @__PURE__ */ jsxs(
    "li",
    {
      role: "option",
      "aria-selected": active && !disabled,
      "aria-disabled": disabled,
      "data-active": active || void 0,
      "data-icon": icon ? "" : void 0,
      className: classes("fui-command-item", "fui-command-option", className),
      ...props,
      children: [
        icon,
        /* @__PURE__ */ jsxs("span", { className: "fui-command-option-text", children: [
          /* @__PURE__ */ jsxs("span", { className: "fui-command-option-main", children: [
            /* @__PURE__ */ jsx("span", { className: "fui-command-option-label", dir: "auto", children: label }),
            detail ? /* @__PURE__ */ jsx("span", { className: "fui-command-detail", dir: "auto", children: detail }) : null
          ] }),
          group ? /* @__PURE__ */ jsx("span", { className: "fui-command-option-group", dir: "auto", children: group }) : null,
          reason ? /* @__PURE__ */ jsx("span", { className: "fui-command-reason", dir: "auto", children: reason }) : null
        ] }),
        keys ? /* @__PURE__ */ jsx("span", { className: "fui-command-option-keys", children: keys }) : null
      ]
    }
  );
}
export {
  CommandOption,
  CommandOptionList,
  CommandTrigger
};
