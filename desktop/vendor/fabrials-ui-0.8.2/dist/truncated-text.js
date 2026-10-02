"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { useRef, useState } from "react";
import { Tooltip, TooltipTrigger, TooltipContent } from "./menu.js";
import { classes } from "./shared.js";
function TruncatedText({
  children,
  className,
  side = "top",
  sideOffset = 6,
  align = "center"
}) {
  const ref = useRef(null);
  const [open, setOpen] = useState(false);
  return /* @__PURE__ */ jsxs(
    Tooltip,
    {
      open,
      disableHoverablePopup: true,
      onOpenChange: (next) => {
        const element = ref.current;
        if (next && (!element || element.scrollWidth <= element.clientWidth)) return;
        setOpen(next);
      },
      children: [
        /* @__PURE__ */ jsx(
          TooltipTrigger,
          {
            render: /* @__PURE__ */ jsx("span", { ref, className: classes("fui-truncated-text", className) }),
            children
          }
        ),
        /* @__PURE__ */ jsx(
          TooltipContent,
          {
            className: "fui-truncated-text-tooltip",
            side,
            sideOffset,
            align,
            children
          }
        )
      ]
    }
  );
}
export {
  TruncatedText
};
