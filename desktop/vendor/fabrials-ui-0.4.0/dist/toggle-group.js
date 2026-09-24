"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { Toggle } from "@base-ui/react/toggle";
import { ToggleGroup as ToggleGroup$1 } from "@base-ui/react/toggle-group";
import { Monitor, Sun, Moon } from "lucide-react";
import { classes } from "./shared.js";
function ToggleGroup({
  className,
  size = "default",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    ToggleGroup$1,
    {
      "data-slot": "toggle-group",
      "data-size": size,
      className: classes("fui-toggle-group", className),
      ...props
    }
  );
}
function ToggleGroupItem({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Toggle,
    {
      "data-slot": "toggle-group-item",
      className: classes("fui-toggle", className),
      ...props
    }
  );
}
const themeOptions = [
  { value: "system", icon: Monitor },
  { value: "light", icon: Sun },
  { value: "dark", icon: Moon }
];
function ThemeSwitcher({
  value,
  onValueChange,
  labels = { system: "System", light: "Light", dark: "Dark" },
  showLabels = false,
  label = "Theme",
  className
}) {
  return /* @__PURE__ */ jsx(
    ToggleGroup,
    {
      "aria-label": label,
      size: "sm",
      value: [value],
      onValueChange: (next) => {
        const selected = next[0];
        if (selected) onValueChange(selected);
      },
      className: classes("fui-theme-switcher", className),
      "data-labels": showLabels || void 0,
      children: themeOptions.map(({ value: option, icon: Icon }) => /* @__PURE__ */ jsxs(
        ToggleGroupItem,
        {
          value: option,
          "aria-label": showLabels ? void 0 : String(labels[option]),
          title: showLabels ? void 0 : String(labels[option]),
          children: [
            /* @__PURE__ */ jsx(Icon, { "aria-hidden": true }),
            showLabels ? /* @__PURE__ */ jsx("span", { children: labels[option] }) : null
          ]
        },
        option
      ))
    }
  );
}
export {
  ThemeSwitcher,
  ToggleGroup,
  ToggleGroupItem
};
