"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { ChevronDown } from "lucide-react";
import { NativeCheckbox } from "./controls.js";
import { Popover, PopoverTrigger, PopoverContent } from "./popover.js";
import { classes } from "./shared.js";
function MultiSelect({
  id,
  label,
  options,
  value,
  onValueChange,
  placeholder = "Any",
  disabled = false,
  className
}) {
  const labelId = `${id}-label`;
  const selected = new Set(value);
  const summary = summaryLabel(options, value, placeholder);
  const toggle = (option) => {
    onValueChange(selected.has(option) ? value.filter((item) => item !== option) : [...value, option]);
  };
  return /* @__PURE__ */ jsxs("div", { className: classes("fui-multi-select", className), children: [
    /* @__PURE__ */ jsx("span", { className: "fui-multi-select-label", id: labelId, children: label }),
    /* @__PURE__ */ jsxs(Popover, { children: [
      /* @__PURE__ */ jsxs(
        PopoverTrigger,
        {
          "aria-labelledby": labelId,
          className: "fui-input fui-select-trigger fui-multi-select-trigger",
          disabled: disabled || options.length === 0,
          id,
          children: [
            /* @__PURE__ */ jsx("span", { children: options.length === 0 ? placeholder : summary }),
            /* @__PURE__ */ jsx(ChevronDown, { "aria-hidden": true, size: 16 })
          ]
        }
      ),
      /* @__PURE__ */ jsxs(PopoverContent, { align: "start", className: "fui-multi-select-popup", children: [
        /* @__PURE__ */ jsx("div", { "aria-labelledby": labelId, className: "fui-multi-select-menu", role: "group", children: options.map((option) => /* @__PURE__ */ jsxs("label", { className: "fui-multi-select-option", children: [
          /* @__PURE__ */ jsx(NativeCheckbox, { checked: selected.has(option.value), onChange: () => toggle(option.value) }),
          /* @__PURE__ */ jsx("span", { children: option.label })
        ] }, option.value)) }),
        value.length > 0 ? /* @__PURE__ */ jsxs("button", { className: "fui-multi-select-clear", onClick: () => onValueChange([]), type: "button", children: [
          "Clear ",
          label.toLocaleLowerCase()
        ] }) : null
      ] })
    ] })
  ] });
}
function summaryLabel(options, value, placeholder) {
  if (value.length === 0) return placeholder;
  if (value.length === 1) return options.find((option) => option.value === value[0])?.label ?? value[0];
  return `${value.length} selected`;
}
export {
  MultiSelect
};
