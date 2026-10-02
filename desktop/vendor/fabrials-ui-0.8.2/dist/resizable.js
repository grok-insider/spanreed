"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { useRef, useEffect, useCallback } from "react";
import { Panel, Separator, Group } from "react-resizable-panels";
import { useGroupRef, usePanelRef } from "react-resizable-panels";
import { classes } from "./shared.js";
function ResizablePanelGroup({
  className,
  direction,
  orientation,
  resizeTargetMinimumSize = { fine: 10, coarse: 24 },
  elementRef,
  onLayoutChanged,
  ...props
}) {
  const element = useRef(null);
  const resetting = useRef(false);
  const reach = Math.max(resizeTargetMinimumSize.fine, resizeTargetMinimumSize.coarse) / 2;
  useEffect(() => {
    let frame = 0;
    const mark = (event) => {
      const near = [...element.current?.querySelectorAll('[role="separator"]') ?? []].some((handle) => {
        const box = handle.getBoundingClientRect();
        return event.clientX >= box.left - reach && event.clientX <= box.right + reach && event.clientY >= box.top - reach && event.clientY <= box.bottom + reach;
      });
      if (!near) return;
      resetting.current = true;
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        frame = requestAnimationFrame(() => {
          resetting.current = false;
        });
      });
    };
    window.addEventListener("dblclick", mark, true);
    return () => {
      window.removeEventListener("dblclick", mark, true);
      cancelAnimationFrame(frame);
    };
  }, [reach]);
  const setElement = useCallback(
    (node) => {
      element.current = node;
      if (typeof elementRef === "function") elementRef(node);
      else if (elementRef) elementRef.current = node;
    },
    [elementRef]
  );
  return /* @__PURE__ */ jsx(
    Group,
    {
      "data-slot": "resizable-panel-group",
      className: classes("fui-resizable-group", className),
      orientation: orientation ?? direction,
      resizeTargetMinimumSize,
      elementRef: setElement,
      onLayoutChanged: onLayoutChanged ? (layout, meta) => onLayoutChanged(layout, resetting.current ? { ...meta, isUserInteraction: true } : meta) : void 0,
      ...props
    }
  );
}
const ResizablePanel = Panel;
function ResizableHandle({ className, label, withHandle = false, children, ...props }) {
  return /* @__PURE__ */ jsxs(Separator, { "data-slot": "resizable-handle", "aria-label": label, className: classes("fui-resizable-handle", className), ...props, children: [
    withHandle ? /* @__PURE__ */ jsx("span", { className: "fui-resizable-grip", "aria-hidden": "true" }) : null,
    children
  ] });
}
export {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
  useGroupRef as useResizableGroupRef,
  usePanelRef as useResizablePanelRef
};
