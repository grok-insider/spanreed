"use client";
import { jsxs, jsx } from "react/jsx-runtime";
import { useRef, useState, useCallback, useEffect, useContext, createContext } from "react";
import { ConfirmDialog } from "./confirm-dialog.js";
function plainText(options) {
  if (options.fallbackText) return options.fallbackText;
  const parts = [options.title, options.description].filter((part) => typeof part === "string" || typeof part === "number").map(String);
  return parts.join("\n\n") || "Are you sure?";
}
const windowConfirm = (options) => {
  if (typeof window === "undefined" || typeof window.confirm !== "function") return Promise.resolve(false);
  return Promise.resolve(window.confirm(plainText(options)));
};
const ConfirmContext = createContext(null);
function ConfirmProvider({ children }) {
  const active = useRef(null);
  const queue = useRef([]);
  const [shown, setShown] = useState(null);
  const [open, setOpen] = useState(false);
  const counter = useRef(0);
  const settle = useCallback((from) => {
    const entry = active.current;
    if (!entry || entry !== from) return;
    active.current = null;
    entry.resolve(entry.confirmed);
    const next = queue.current.shift();
    if (next) {
      active.current = next;
      setShown(next);
    } else {
      setOpen(false);
    }
  }, []);
  const ask = useCallback((options2) => {
    return new Promise((resolve) => {
      const focused = typeof document === "undefined" ? null : document.activeElement;
      const entry = {
        id: counter.current += 1,
        options: options2,
        resolve,
        opener: focused instanceof HTMLElement && focused !== document.body ? focused : null,
        confirmed: false
      };
      if (active.current) {
        queue.current.push(entry);
        return;
      }
      active.current = entry;
      setShown(entry);
      setOpen(true);
    });
  }, []);
  useEffect(
    () => () => {
      active.current?.resolve(false);
      active.current = null;
      for (const entry of queue.current) entry.resolve(false);
      queue.current = [];
    },
    []
  );
  const options = shown?.options;
  const opener = shown?.opener ?? null;
  return /* @__PURE__ */ jsxs(ConfirmContext.Provider, { value: ask, children: [
    children,
    options ? /* @__PURE__ */ jsx(
      ConfirmDialog,
      {
        open,
        onOpenChange: (next) => {
          if (!next && shown) settle(shown);
        },
        title: options.title,
        description: options.description,
        details: options.details,
        confirmLabel: options.confirmLabel ?? "Confirm",
        cancelLabel: options.cancelLabel,
        destructive: options.destructive,
        finalFocus: options.finalFocus ?? (() => opener?.isConnected ? opener : null),
        onConfirm: () => {
          if (shown && active.current === shown) shown.confirmed = true;
        }
      },
      shown?.id
    ) : null
  ] });
}
function useConfirm() {
  return useContext(ConfirmContext) ?? windowConfirm;
}
export {
  ConfirmProvider,
  useConfirm
};
