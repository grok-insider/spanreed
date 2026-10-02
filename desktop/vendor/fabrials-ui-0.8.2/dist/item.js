"use client";
import { jsx } from "react/jsx-runtime";
import { useContext, createContext } from "react";
import { mergeProps } from "@base-ui/react/merge-props";
import { useRender } from "@base-ui/react/use-render";
import { classes } from "./shared.js";
const attrs = (value) => value;
const InsideGroup = createContext(false);
function ItemGroup({ className, bordered = false, ...props }) {
  return /* @__PURE__ */ jsx(InsideGroup.Provider, { value: true, children: /* @__PURE__ */ jsx("ul", { "data-slot": "item-group", "data-bordered": bordered ? "" : void 0, className: classes("fui-item-group", className), ...props }) });
}
function Item({ className, variant = "default", size = "default", current, selected, unread, stretch, render, ...props }) {
  const inGroup = useContext(InsideGroup);
  return useRender({
    defaultTagName: inGroup ? "li" : "div",
    render,
    props: mergeProps(
      attrs({
        className: classes("fui-item", className),
        "data-slot": "item",
        "data-variant": variant,
        "data-size": size === "default" ? void 0 : size,
        "data-current": current ? "" : void 0,
        "data-selected": selected ? "" : void 0,
        "data-unread": unread ? "" : void 0,
        "data-stretch": stretch ? "" : void 0
      }),
      props
    )
  });
}
function ItemLink({ current, render, className, ...props }) {
  const value = current === true ? "true" : current === false || current === void 0 ? void 0 : current;
  return useRender({
    defaultTagName: "a",
    render,
    props: mergeProps(
      attrs({ className: classes("fui-item-link", className), "data-slot": "item-link", "aria-current": value }),
      props
    )
  });
}
function ItemMedia({ variant = "default", className, render, ...props }) {
  return useRender({
    defaultTagName: "div",
    render,
    props: mergeProps(attrs({ className: classes("fui-item-media", className), "data-slot": "item-media", "data-variant": variant }), props)
  });
}
function ItemContent({ className, render, ...props }) {
  return useRender({ defaultTagName: "div", render, props: mergeProps(attrs({ className: classes("fui-item-content", className), "data-slot": "item-content" }), props) });
}
function ItemTitle({ className, render, ...props }) {
  return useRender({ defaultTagName: "div", render, props: mergeProps(attrs({ className: classes("fui-item-title", className), "data-slot": "item-title" }), props) });
}
function ItemDescription({ className, render, ...props }) {
  return useRender({ defaultTagName: "p", render, props: mergeProps(attrs({ className: classes("fui-item-description", className), "data-slot": "item-description" }), props) });
}
function ItemActions({ className, render, ...props }) {
  return useRender({ defaultTagName: "div", render, props: mergeProps(attrs({ className: classes("fui-item-actions", className), "data-slot": "item-actions" }), props) });
}
function ItemControl({ className, render, ...props }) {
  return useRender({ defaultTagName: "span", render, props: mergeProps(attrs({ className: classes("fui-item-control", className), "data-slot": "item-control" }), props) });
}
function ItemHeader({ className, render, ...props }) {
  return useRender({ defaultTagName: "div", render, props: mergeProps(attrs({ className: classes("fui-item-header", className), "data-slot": "item-header" }), props) });
}
function ItemFooter({ className, render, ...props }) {
  return useRender({ defaultTagName: "div", render, props: mergeProps(attrs({ className: classes("fui-item-footer", className), "data-slot": "item-footer" }), props) });
}
function ItemCheck({ className, render, ...props }) {
  return useRender({ defaultTagName: "label", render, props: mergeProps(attrs({ className: classes("fui-item-check", className), "data-slot": "item-check" }), props) });
}
function ItemUnread({ className, render, ...props }) {
  return useRender({ defaultTagName: "span", render, props: mergeProps(attrs({ className: classes("fui-item-unread", className), "data-slot": "item-unread", "aria-hidden": true }), props) });
}
function ItemSeparator({ className, ...props }) {
  const inGroup = useContext(InsideGroup);
  const hook = classes("fui-item-separator", className);
  return inGroup ? /* @__PURE__ */ jsx("li", { "aria-hidden": "true", "data-slot": "item-separator", className: hook, ...props }) : /* @__PURE__ */ jsx("div", { role: "separator", "data-slot": "item-separator", className: hook, ...props });
}
export {
  Item,
  ItemActions,
  ItemCheck,
  ItemContent,
  ItemControl,
  ItemDescription,
  ItemFooter,
  ItemGroup,
  ItemHeader,
  ItemLink,
  ItemMedia,
  ItemSeparator,
  ItemTitle,
  ItemUnread
};
