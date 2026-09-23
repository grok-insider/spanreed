"use client";
import { jsx } from "react/jsx-runtime";
import { Collapsible as Collapsible$1 } from "@base-ui/react/collapsible";
function Collapsible({ ...props }) {
  return /* @__PURE__ */ jsx(Collapsible$1.Root, { "data-slot": "collapsible", ...props });
}
function CollapsibleTrigger({ ...props }) {
  return /* @__PURE__ */ jsx(Collapsible$1.Trigger, { "data-slot": "collapsible-trigger", ...props });
}
function CollapsibleContent({ ...props }) {
  return /* @__PURE__ */ jsx(Collapsible$1.Panel, { "data-slot": "collapsible-content", ...props });
}
export {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger
};
