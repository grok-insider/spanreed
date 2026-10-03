"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { ScrollArea as ScrollArea$1 } from "@base-ui/react/scroll-area";
import { classes } from "./shared.js";
function ScrollArea({
  className,
  children,
  ...props
}) {
  return /* @__PURE__ */ jsxs(
    ScrollArea$1.Root,
    {
      "data-slot": "scroll-area",
      className: classes("fui-scroll-area", className),
      ...props,
      children: [
        /* @__PURE__ */ jsx(
          ScrollArea$1.Viewport,
          {
            "data-slot": "scroll-area-viewport",
            className: "fui-scroll-area-viewport",
            children
          }
        ),
        /* @__PURE__ */ jsx(ScrollBar, {}),
        /* @__PURE__ */ jsx(ScrollArea$1.Corner, {})
      ]
    }
  );
}
function ScrollBar({
  className,
  orientation = "vertical",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    ScrollArea$1.Scrollbar,
    {
      "data-slot": "scroll-area-scrollbar",
      "data-orientation": orientation,
      orientation,
      className: classes("fui-scrollbar", className),
      ...props,
      children: /* @__PURE__ */ jsx(
        ScrollArea$1.Thumb,
        {
          "data-slot": "scroll-area-thumb",
          className: "fui-scrollbar-thumb"
        }
      )
    }
  );
}
export {
  ScrollArea,
  ScrollBar
};
