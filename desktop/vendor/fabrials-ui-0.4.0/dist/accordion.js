"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { Accordion as Accordion$1 } from "@base-ui/react/accordion";
import { ChevronDown } from "lucide-react";
import { classes } from "./shared.js";
function Accordion({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Accordion$1.Root,
    {
      "data-slot": "accordion",
      className: classes("fui-accordion", className),
      ...props
    }
  );
}
function AccordionItem({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Accordion$1.Item,
    {
      "data-slot": "accordion-item",
      className: classes("fui-accordion-item", className),
      ...props
    }
  );
}
function AccordionTrigger({
  className,
  children,
  ...props
}) {
  return /* @__PURE__ */ jsx(Accordion$1.Header, { className: "fui-accordion-header", children: /* @__PURE__ */ jsxs(
    Accordion$1.Trigger,
    {
      "data-slot": "accordion-trigger",
      className: classes("fui-accordion-trigger", className),
      ...props,
      children: [
        children,
        /* @__PURE__ */ jsx(ChevronDown, { "aria-hidden": true, className: "fui-accordion-chevron" })
      ]
    }
  ) });
}
function AccordionContent({
  className,
  children,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    Accordion$1.Panel,
    {
      "data-slot": "accordion-content",
      className: classes("fui-accordion-panel", className),
      ...props,
      children: /* @__PURE__ */ jsx("div", { className: "fui-accordion-content", children })
    }
  );
}
export {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger
};
