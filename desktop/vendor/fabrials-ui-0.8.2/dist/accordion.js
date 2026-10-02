"use client";
import { jsx, jsxs } from "react/jsx-runtime";
import { Accordion as Accordion$1 } from "@base-ui/react/accordion";
import { ChevronDown } from "lucide-react";
import { useContext, useId, createElement, createContext } from "react";
import { classes } from "./shared.js";
const VariantContext = createContext("default");
function Accordion({
  className,
  variant = "default",
  keepMounted,
  ...props
}) {
  return /* @__PURE__ */ jsx(VariantContext.Provider, { value: variant, children: /* @__PURE__ */ jsx(
    Accordion$1.Root,
    {
      "data-slot": "accordion",
      "data-variant": variant === "rows" ? "rows" : void 0,
      keepMounted: keepMounted ?? (variant === "rows" ? true : void 0),
      className: classes("fui-accordion", className),
      ...props
    }
  ) });
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
  headingLevel,
  headingRef,
  icon,
  aside,
  ...props
}) {
  const variant = useContext(VariantContext);
  const asideId = useId();
  const row = variant === "rows" || aside !== void 0;
  const focusable = headingRef !== void 0 || variant === "rows";
  const header = /* @__PURE__ */ jsx(
    Accordion$1.Header,
    {
      className: "fui-accordion-header",
      ref: headingRef,
      render: headingLevel && headingLevel !== 3 ? createElement(`h${headingLevel}`) : void 0,
      tabIndex: focusable ? -1 : void 0,
      onFocus: focusable ? (event) => {
        if (event.target === event.currentTarget)
          event.currentTarget.querySelector('[data-slot="accordion-trigger"]')?.focus();
      } : void 0,
      children: /* @__PURE__ */ jsxs(
        Accordion$1.Trigger,
        {
          "data-slot": "accordion-trigger",
          className: classes("fui-accordion-trigger", className),
          "aria-describedby": aside !== void 0 ? asideId : void 0,
          ...props,
          children: [
            icon ? /* @__PURE__ */ jsx("span", { className: "fui-accordion-icon", "aria-hidden": true, children: icon }) : null,
            row ? /* @__PURE__ */ jsx("span", { className: "fui-accordion-title", children }) : children,
            row ? null : /* @__PURE__ */ jsx(ChevronDown, { "aria-hidden": true, className: "fui-accordion-chevron" })
          ]
        }
      )
    }
  );
  if (!row) return header;
  return /* @__PURE__ */ jsxs("div", { className: "fui-accordion-head", children: [
    header,
    aside !== void 0 ? /* @__PURE__ */ jsx("span", { id: asideId, className: "fui-accordion-aside", children: aside }) : null,
    /* @__PURE__ */ jsx(ChevronDown, { "aria-hidden": true, className: "fui-accordion-chevron" })
  ] });
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
