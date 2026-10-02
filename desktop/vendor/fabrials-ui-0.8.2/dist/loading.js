"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { useState, useEffect, useContext, createContext } from "react";
import { classes } from "./shared.js";
const LoadingContext = createContext(false);
function useLoading() {
  return useContext(LoadingContext);
}
function Loading({ when, label = "Loading", className, children, ...props }) {
  const [previous, setPrevious] = useState(when);
  const [settling, setSettling] = useState(false);
  if (previous !== when) {
    setPrevious(when);
    setSettling(!when);
  }
  useEffect(() => {
    if (!settling) return;
    let frame = requestAnimationFrame(() => {
      frame = requestAnimationFrame(() => setSettling(false));
    });
    return () => cancelAnimationFrame(frame);
  }, [settling]);
  return /* @__PURE__ */ jsxs(
    "div",
    {
      "data-slot": "loading",
      "data-fui-loading": when ? "" : void 0,
      "data-fui-loading-settle": settling ? "" : void 0,
      "aria-busy": when || void 0,
      className: classes("fui-loading", className),
      ...props,
      children: [
        /* @__PURE__ */ jsx("span", { role: "status", className: "fui-sr-only", children: when ? label : "" }),
        /* @__PURE__ */ jsx("div", { className: "fui-loading-content", inert: when, children: /* @__PURE__ */ jsx(LoadingContext.Provider, { value: when, children }) })
      ]
    }
  );
}
const WORDS = [5, 3, 7, 4, 9, 2, 6, 4, 8, 3, 5, 6, 3, 7, 4, 5];
function placeholderText(length, seed = 0) {
  let text = "";
  for (let i = 0; text.length < length; i += 1) {
    const size = WORDS[(i + seed) % WORDS.length];
    text += (text ? " " : "") + "x".repeat(size);
  }
  return text.slice(0, Math.max(0, length));
}
function placeholderList(count, make) {
  return Array.from({ length: Math.max(0, count) }, (_, index) => make(index));
}
export {
  Loading,
  placeholderList,
  placeholderText,
  useLoading
};
